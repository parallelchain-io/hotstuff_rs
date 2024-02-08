/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! [Trait definition](Network) for pluggable peer-to-peer networking, as well as the internal types and functions
//! that replicas use to interact with the network.
//!
//! HotStuff-rs' has modular peer-to-peer networking, with each peer reachable by their [VerifyingKey](ed25519_dalek::VerifyingKey). 
//! Networking providers interact with HotStuff-rs' threads through implementations of the [Network] trait. This trait has five methods
//! that collectively allow peers to exchange progress protocol and sync protocol messages.  

use std::collections::{BTreeMap, VecDeque};
use std::mem;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::messages::*;
use crate::types::{Certificate, ChainID, QuorumCertificate, ValidatorSet, ValidatorSetUpdates, VerifyingKey, ViewNumber};

pub trait Network: Clone + Send {
    /// Informs the network provider the validator set on wake-up.
    fn init_validator_set(&mut self, validator_set: ValidatorSet);

    /// Informs the networking provider of updates to the validator set.
    fn update_validator_set(&mut self, updates: ValidatorSetUpdates);

    /// Send a message to all peers (including listeners) without blocking.
    fn broadcast(&mut self, message: Message);

    /// Send a message to the specified peer without blocking.
    fn send(&mut self, peer: VerifyingKey, message: Message);

    /// Receive a message from any peer. Returns immediately with a None if no message is available now.
    fn recv(&mut self) -> Option<(VerifyingKey, Message)>;
}

/// Spawn the poller thread, which polls the Network for messages and distributes them into receivers for
/// progress messages, sync requests, and sync responses.
pub(crate) fn start_polling<N: Network + 'static>(
    mut network: N,
    shutdown_signal: Receiver<()>,
) -> (
    JoinHandle<()>,
    Receiver<(VerifyingKey, ProgressMessage)>,
    Receiver<(VerifyingKey, SyncRequest)>,
    Receiver<(VerifyingKey, SyncResponse)>,
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
/// This type's recv method only returns messages from the specified current view, and caches messages from
/// future views for later consumption. This helps prevents interruptions to progress when replicas' views
/// are mostly synchronized but they enter views at slightly different times.
/// 
/// If the message buffer grows beyond its maximum capacity, some highest-viewed messages might be removed 
/// from the buffer to make space for the new message.
pub(crate) struct ProgressMessageStub<N: Network> {
    network: N,
    receiver: Receiver<(VerifyingKey, ProgressMessage)>,
    msg_buffer_capacity: u64,
    msg_buffer: BTreeMap<ViewNumber, VecDeque<(VerifyingKey, ProgressMessage)>>,
    msg_buffer_size: u64,
}

impl<N: Network> ProgressMessageStub<N> {
    pub(crate) fn new(
        network: N,
        receiver: Receiver<(VerifyingKey, ProgressMessage)>,
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

    /// Receive a message matching the given chain id and current view. Messages from cur_view + 1 are cached for future
    /// consumption, while messages from other views are dropped immediately. 
    /// 
    /// In case caching the message makes the buffer grow beyond its capacity, this function either:
    /// 1. If the message has the highest view among the views of messages currently in the buffer, then the message is dropped, or
    /// 2. Otherwise, just enough highest-viewed messages are removed from the buffer to make space for the new message.
    pub(crate) fn recv(
        &mut self,
        chain_id: ChainID,
        cur_view: ViewNumber,
        deadline: Instant,
    ) -> Result<(VerifyingKey, ProgressMessage), ProgressMessageReceiveError> {
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

                    // Return the message if its for the current view.
                    if msg.view() == cur_view {
                        return Ok((sender, msg));
                    }

                    // Cache the message if it is for a future view, 
                    // unless the buffer is overloaded in which case we need to make space or ignore the message.
                    else if msg.view() > cur_view {

                        // Check if the message contains a QC from the future.
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
                        
                        // Try to cache the message.
                        let bytes_requested = mem::size_of::<VerifyingKey>() as u64 + msg.size();
                        let new_buffer_size = self.msg_buffer_size.checked_add(bytes_requested);
                        let buffer_will_be_overloaded = new_buffer_size.is_none() || new_buffer_size.unwrap() > self.msg_buffer_capacity;
                        let cache_message_if_buffer_will_be_overloaded = self.msg_buffer.keys().max().is_none() || self.msg_buffer.keys().max().is_some_and(|max_view| msg.view() < *max_view);
                
                        // We only need to make space in the buffer if:
                        // (1) It will be overloaded after storing the message, and 
                        // (2) We want to store this message in the buffer, i.e., if the message's view is lower than that of the highest-viewed message stored in the buffer
                        // (otherwise we ignore the message to avoid overloading the buffer).
                        if buffer_will_be_overloaded && cache_message_if_buffer_will_be_overloaded {
                            self.remove_from_overloaded_buffer(bytes_requested);
                        };

                        // We only store the message in the buffer if either:
                        // (1) There is no risk of overloading the buffer upon storing this message, or
                        // (2) The buffer might be overloaded, but we have already made space for the new message.
                        if !buffer_will_be_overloaded || (buffer_will_be_overloaded && cache_message_if_buffer_will_be_overloaded) {
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

                        // Inform the caller if the received message contains a QC from the future.
                        if received_qc_from_future {
                            return Err(ProgressMessageReceiveError::ReceivedQCFromFuture);
                        }

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

    pub(crate) fn send(&mut self, peer: VerifyingKey, msg: ProgressMessage) {
        self.network.send(peer, Message::ProgressMessage(msg))
    }

    pub(crate) fn broadcast(&mut self, msg: ProgressMessage) {
        self.network.broadcast(Message::ProgressMessage(msg))
    }

    pub(crate) fn update_validator_set(&mut self, updates: ValidatorSetUpdates) {
        self.network.update_validator_set(updates)
    }

    /// Given the number of bytes that need to be removed, removes just enough highest-viewed messages
    /// to free up (at least) the required number of bytes in the buffer.
    fn remove_from_overloaded_buffer(&mut self, bytes_to_remove: u64) {
        let verifying_key_size = mem::size_of::<VerifyingKey>() as u64;
        
        let mut bytes_removed = 0;
        let mut views_removed = Vec::new();
        let mut msg_queues_iter = self.msg_buffer.iter_mut().rev(); // msg_queues from highest view to lowest view

        // Removes messages from the message buffer until the required number of bytes is freed.
        while bytes_removed < bytes_to_remove {
            // Take the message queue for the next highest view, and remove its messages until the required number of bytes is freed.
            if let Some((view, msg_queue)) = msg_queues_iter.next() {
                let removals = 
                    msg_queue.iter().rev()
                    .take_while(|(_, msg)| 
                        {
                            if bytes_removed < bytes_to_remove {
                                bytes_removed += msg.size() + verifying_key_size;
                                true
                            } else {
                                false
                            }
                        }
                    )
                    .count() as u64;
                let _ = (0..removals).into_iter().for_each(|_| { let _ = msg_queue.pop_back(); });
                if msg_queue.is_empty() {
                    views_removed.push(*view)
                }
            } else {
                break
            }
        };

        self.msg_buffer_size -= bytes_removed;
        
        // If some views in the message buffer have lost all messages as a result of the removal,
        // then also remove their corresponding keys from the buffer.
        views_removed.iter().for_each(|view| {let _ = self.msg_buffer.remove(view);});

    }

}

pub(crate) enum ProgressMessageReceiveError {
    Timeout,
    ReceivedQCFromFuture,
    ShouldAdvanceView{c: Certificate, sender: VerifyingKey}
}

pub(crate) struct SyncClientStub<N: Network> {
    network: N,
    responses: Receiver<(VerifyingKey, SyncResponse)>,
}

impl<N: Network> SyncClientStub<N> {
    pub(crate) fn new(
        network: N,
        responses: Receiver<(VerifyingKey, SyncResponse)>,
    ) -> SyncClientStub<N> {
        SyncClientStub { network, responses }
    }

    pub(crate) fn send_request(&mut self, peer: VerifyingKey, msg: SyncRequest) {
        self.network
            .send(peer, Message::SyncMessage(SyncMessage::SyncRequest(msg)));
    }

    pub(crate) fn recv_response(
        &self,
        peer: VerifyingKey,
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
    requests: Receiver<(VerifyingKey, SyncRequest)>,
    network: N,
}

/// A sending and receiving end for sync messages. 
/// The [SyncServerStub::recv_request] method returns the received request.
impl<N: Network> SyncServerStub<N> {
    pub(crate) fn new(
        requests: Receiver<(VerifyingKey, SyncRequest)>,
        network: N,
    ) -> SyncServerStub<N> {
        SyncServerStub { requests, network }
    }

    pub(crate) fn recv_request(&self) -> Option<(VerifyingKey, SyncRequest)> {
        match self.requests.try_recv() {
            Ok((origin, request)) => Some((origin, request)),
            // Safety: the sync server thread (the only caller of this function) shuts down before the poller thread
            // (the sender side of this channel), so we will never be disconnected at this point.
            Err(TryRecvError::Disconnected) => panic!(),
            Err(TryRecvError::Empty) => None,
        }
    }

    pub(crate) fn send_response(&mut self, peer: VerifyingKey, msg: SyncResponse) {
        self.network
            .send(peer, Message::SyncMessage(SyncMessage::SyncResponse(msg)));
    }
}
