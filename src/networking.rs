/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! [Trait definition](Network) for pluggable peer-to-peer networking, as well as the internal types and functions
//! that replicas use to interact with the network.
//!
//! HotStuff-rs' has modular peer-to-peer networking, with each peer reachable by their PublicKey. Networking providers
//! interact with HotStuff-rs' threads through implementations of the [Network] trait. This trait has five methods
//! that collectively allow peers to exchange progress protocol and sync protocol messages.  

use std::collections::{BTreeMap, VecDeque};
use std::mem;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::messages::*;
use crate::types::{ChainID, PublicKey, ValidatorSet, ValidatorSetUpdates, ViewNumber};

pub trait Network: Clone + Send {
    /// Informs the network provider the validator set on wake-up.
    fn init_validator_set(&mut self, validator_set: ValidatorSet);

    /// Informs the networking provider of updates to the validator set.
    fn update_validator_set(&mut self, updates: ValidatorSetUpdates);

    /// Send a message to all peers (including listeners) without blocking.
    fn broadcast(&mut self, message: Message);

    /// Send a message to the specified peer without blocking.
    fn send(&mut self, peer: PublicKey, message: Message);

    /// Receive a message from any peer. Returns immediately with a None if no message is available now.
    ///
    /// # Safety
    /// the returned PublicKeyBytes must be a valid Ed25519 public key; i.e., it must be convertible to a
    /// [PublicKey](crate::types::PublicKey) using [from_bytes](crate::types::PublicKey::from_bytes) without errors.
    fn recv(&mut self) -> Option<(PublicKey, Message)>;
}

/// Spawn the poller thread, which polls the Network for messages and distributes them into receivers for
/// progress messages, sync requests, and sync responses.
pub(crate) fn start_polling<N: Network + 'static>(
    mut network: N,
    shutdown_signal: Receiver<()>,
) -> (
    JoinHandle<()>,
    Receiver<(PublicKey, ProgressMessage)>,
    Receiver<(PublicKey, SyncRequest)>,
    Receiver<(PublicKey, SyncResponse)>,
) {
    let (to_progress_msg_receiver, progress_msg_receiver) = mpsc::channel();
    let (to_sync_request_receiver, sync_request_receiver) = mpsc::channel();
    let (to_sync_response_receiver, sync_response_receiver) = mpsc::channel();

    let poller_thread = thread::spawn(move || loop {
        match shutdown_signal.try_recv() {
            Ok(()) => return,
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => {
                panic!("Poller thread disconnected from main thread")
            }
        }

        if let Some((origin, msg)) = network.recv() {
            match msg {
                Message::ProgressMessage(p_msg) => {
                    let _ = to_progress_msg_receiver.send((origin, p_msg));
                }
                Message::SyncMessage(s_msg) => match s_msg {
                    SyncMessage::SyncRequest(s_req) => {
                        let _ = to_sync_request_receiver.send((origin, s_req));
                    }
                    SyncMessage::SyncResponse(s_res) => {
                        let _ = to_sync_response_receiver.send((origin, s_res));
                    }
                },
            }
        } else {
            thread::yield_now()
        }
    });
    (
        poller_thread,
        progress_msg_receiver,
        sync_request_receiver,
        sync_response_receiver,
    )
}

/// A sending and receiving end for progress messages.
///
/// This type's [ViewAwareProgressMessageReceiver::recv] method only returns messages from the specified
/// current view, and caches messages from future views for later consumption.
///
/// This helps prevents interruptions to progress when replicas' views are mostly synchronized but they
/// enter views at slightly different times.
pub(crate) struct ProgressMessageStub<N: Network> {
    network: N,
    receiver: Receiver<(PublicKey, ProgressMessage)>,
    msg_buffer_capacity: u64,
    msg_buffer: BTreeMap<ViewNumber, VecDeque<(PublicKey, ProgressMessage)>>,
    msg_buffer_size: u64,
}

impl<N: Network> ProgressMessageStub<N> {
    pub(crate) fn new(
        network: N,
        receiver: Receiver<(PublicKey, ProgressMessage)>,
        msg_buffer_capacity: u64,
    ) -> ProgressMessageStub<N> {
        Self {
            network,
            receiver,
            msg_buffer_capacity,
            msg_buffer: BTreeMap::new(),
            msg_buffer_size: 0,
        }
    }

    // Receive a message matching the given chain id and current view. Messages from cur_view + 1 are cached for future
    // consumption, while messages from other views are dropped immediately.
    pub(crate) fn recv(
        &mut self,
        chain_id: ChainID,
        cur_view: ViewNumber,
        deadline: Instant,
    ) -> Result<(PublicKey, ProgressMessage), ProgressMessageReceiveError> {
        // Clear buffer of messages with views lower than the current one.
        self.msg_buffer = self.msg_buffer.split_off(&cur_view);

        // Try to get buffered messages for the current view.
        if let Some(msg_queue) = self.msg_buffer.get_mut(&cur_view) {
            if let Some(msg) = msg_queue.pop_front() {
                return Ok(msg);
            }
        }

        // Try to get messages from the poller.
        while Instant::now() < deadline {
            match self.receiver.recv_timeout(deadline - Instant::now()) {
                Ok((sender, msg)) => {
                    if msg.chain_id() != chain_id {
                        continue;
                    }

                    // Inform the caller that we've received a QC from the future.
                    let received_qc_from_future = match &msg {
                        ProgressMessage::Proposal(Proposal { block, .. }) => {
                            block.justify.view > cur_view
                        }
                        ProgressMessage::Nudge(Nudge { justify, .. }) => justify.view > cur_view,
                        ProgressMessage::NewView(NewView { highest_qc, .. }) => {
                            highest_qc.view > cur_view
                        }
                        _ => false,
                    };
                    if received_qc_from_future {
                        return Err(ProgressMessageReceiveError::ReceivedQCFromFuture);
                    }

                    // Return the message if its for the current view.
                    if msg.view() == cur_view {
                        return Ok((sender, msg));
                    }
                    // Cache the message if its for a future view 
                    // (unless the buffer is overloaded in which case we need to make space or ignore the message).
                    else if msg.view() > cur_view {

                        let bytes_requested = mem::size_of::<PublicKey>() as u64 + Self::size_of_msg(&msg);
                        let overloaded_buffer = self.msg_buffer_size + bytes_requested > self.msg_buffer_capacity;
                        let cache_message_if_overloaded_buffer = self.msg_buffer.keys().max().is_some() && msg.view() < self.msg_buffer.keys().max().copied().unwrap();
                
                        // We only need to make space in the buffer if:
                        // (1) it will be overloaded after stroing the message, and 
                        // (2) we want to store this message in the buffer, i.e., if the message's view is lower than that of the highest-viewed message stored in the buffer
                        // (otherwise we ignore the message to avoid overloading the buffer)
                        if overloaded_buffer && cache_message_if_overloaded_buffer {
                            Self::remove_from_overloaded_buffer(&mut self.msg_buffer, bytes_requested);
                        };

                        // We only store the message in the buffer if either:
                        // (1) there is no risk of overloading the buffer upon storing this message, or
                        // (2) the buffer might be overloaded, but we have already made space for the new message
                        if !overloaded_buffer || (overloaded_buffer && cache_message_if_overloaded_buffer) {
                            let msg_queue = if let Some(msg_queue) = self.msg_buffer.get_mut(&msg.view())
                            {
                                msg_queue
                            } else {
                                self.msg_buffer.insert(msg.view(), VecDeque::new());
                                self.msg_buffer.get_mut(&msg.view()).unwrap()
                            };

                            self.msg_buffer_size += bytes_requested;
                            msg_queue.push_back((sender, msg));

                        };

                    }
                }
                Err(RecvTimeoutError::Timeout) => thread::yield_now(),

                // Safety: the algorithm thread (the only caller of this function) shuts down before the poller thread (the
                // sender side of this channel), so we will never be disconnected at this point.
                Err(RecvTimeoutError::Disconnected) => panic!(),
            }
        }

        Err(ProgressMessageReceiveError::Timeout)
    }

    pub(crate) fn send(&mut self, peer: PublicKey, msg: ProgressMessage) {
        self.network.send(peer, Message::ProgressMessage(msg))
    }

    pub(crate) fn broadcast(&mut self, msg: ProgressMessage) {
        self.network.broadcast(Message::ProgressMessage(msg))
    }

    pub(crate) fn update_validator_set(&mut self, updates: ValidatorSetUpdates) {
        self.network.update_validator_set(updates)
    }

    fn remove_from_overloaded_buffer(buffer: &mut BTreeMap<u64, VecDeque<(PublicKey, ProgressMessage)>>, bytes_to_remove: u64) {
        let public_key_size = mem::size_of::<PublicKey>() as u64;
        
        let mut bytes_removed = 0;
        let mut views_removed = Vec::new();
        let mut msg_queues_iter = buffer.iter_mut().rev(); // msg_queues from highest view to lowest view

        while bytes_removed < bytes_to_remove {
            if let Some((view, msg_queue)) = msg_queues_iter.next() {
                let removals = 
                    msg_queue.iter().rev()
                    .take_while(|(_, msg)| 
                        {
                            if bytes_removed < bytes_to_remove {
                                bytes_removed += Self::size_of_msg(msg) + public_key_size;
                                true
                            } else {
                                false
                            }
                        }
                    )
                    .count() as u64;
                let _ = (0..removals).into_iter().for_each(|_| {let _ = msg_queue.pop_back();});
                if msg_queue.is_empty() {
                    views_removed.push(*view)
                }
            } else {
                break
            }
        };

        views_removed.iter().for_each(|view| {let _ = buffer.remove(view);});

    }

    fn size_of_msg(msg: &ProgressMessage) -> u64 {
        match msg {
            ProgressMessage::Proposal(_) => mem::size_of::<Proposal>() as u64,
            ProgressMessage::Nudge(_) => mem::size_of::<Nudge>() as u64,
            ProgressMessage:: Vote(_) => mem::size_of::<Vote>() as u64,
            ProgressMessage::NewView(_) => mem::size_of::<NewView>() as u64,
        }
    }

}

pub(crate) enum ProgressMessageReceiveError {
    Timeout,
    ReceivedQCFromFuture,
}

pub(crate) struct SyncClientStub<N: Network> {
    network: N,
    responses: Receiver<(PublicKey, SyncResponse)>,
}

impl<N: Network> SyncClientStub<N> {
    pub(crate) fn new(
        network: N,
        responses: Receiver<(PublicKey, SyncResponse)>,
    ) -> SyncClientStub<N> {
        SyncClientStub { network, responses }
    }

    pub(crate) fn send_request(&mut self, peer: PublicKey, msg: SyncRequest) {
        self.network
            .send(peer, Message::SyncMessage(SyncMessage::SyncRequest(msg)));
    }

    pub(crate) fn recv_response(
        &self,
        peer: PublicKey,
        deadline: Instant,
    ) -> Option<SyncResponse> {
        while Instant::now() < deadline {
            match self.responses.recv_timeout(deadline - Instant::now()) {
                Ok((sender, sync_response)) => {
                    if sender == peer {
                        return Some(sync_response);
                    }
                }
                Err(RecvTimeoutError::Timeout) => thread::yield_now(),
                Err(RecvTimeoutError::Disconnected) => panic!(),
            }
        }

        None
    }

    pub(crate) fn update_validator_set(&mut self, updates: ValidatorSetUpdates) {
        self.network.update_validator_set(updates)
    }
}

pub(crate) struct SyncServerStub<N: Network> {
    requests: Receiver<(PublicKey, SyncRequest)>,
    network: N,
}

impl<N: Network> SyncServerStub<N> {
    pub(crate) fn new(
        requests: Receiver<(PublicKey, SyncRequest)>,
        network: N,
    ) -> SyncServerStub<N> {
        SyncServerStub { requests, network }
    }

    pub(crate) fn recv_request(&self) -> Option<(PublicKey, SyncRequest)> {
        match self.requests.try_recv() {
            Ok((origin, request)) => Some((origin, request)),
            // Safety: the sync server thread (the only caller of this function) shuts down before the poller thread
            // (the sender side of this channel), so we will never be disconnected at this point.
            Err(TryRecvError::Disconnected) => panic!(),
            Err(TryRecvError::Empty) => None,
        }
    }

    pub(crate) fn send_response(&mut self, peer: PublicKey, msg: SyncResponse) {
        self.network
            .send(peer, Message::SyncMessage(SyncMessage::SyncResponse(msg)));
    }
}
