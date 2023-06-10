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
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::messages::*;
use crate::types::{ChainID, PublicKeyBytes, ValidatorSet, ValidatorSetUpdates, ViewNumber};

pub trait Network: Clone + Send {
    /// Informs the network provider the validator set on wake-up.
    fn init_validator_set(&mut self, validator_set: ValidatorSet);

    /// Informs the networking provider of updates to the validator set.
    fn update_validator_set(&mut self, updates: ValidatorSetUpdates);

    /// Send a message to all peers (including listeners) without blocking.
    fn broadcast(&mut self, message: Message);

    /// Send a message to the specified peer without blocking.
    fn send(&mut self, peer: PublicKeyBytes, message: Message);

    /// Receive a message from any peer. Returns immediately with a None if no message is available now.
    /// 
    /// # Safety
    /// the returned PublicKeyBytes must be a valid Ed25519 public key; i.e., it must be convertible to a 
    /// [PublicKey](crate::types::PublicKey) using [from_bytes](crate::types::PublicKey::from_bytes) without errors.
    fn recv(&mut self) -> Option<(PublicKeyBytes, Message)>;
}

/// Spawn the poller thread, which polls the Network for messages and distributes them into receivers for
/// progress messages, sync requests, and sync responses.
pub(crate) fn start_polling<N: Network + 'static>(
    mut network: N,
    shutdown_signal: Receiver<()>,
) -> (
    JoinHandle<()>,
    Receiver<(PublicKeyBytes, ProgressMessage)>,
    Receiver<(PublicKeyBytes, SyncRequest)>,
    Receiver<(PublicKeyBytes, SyncResponse)>,
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
    receiver: Receiver<(PublicKeyBytes, ProgressMessage)>,
    msg_buffer: BTreeMap<ViewNumber, VecDeque<(PublicKeyBytes, ProgressMessage)>>,
}

impl<N: Network> ProgressMessageStub<N> {
    pub(crate) fn new(
        network: N,
        receiver: Receiver<(PublicKeyBytes, ProgressMessage)>,
    ) -> ProgressMessageStub<N> {
        Self {
            network,
            receiver,
            msg_buffer: BTreeMap::new(),
        }
    }

    // Receive a message matching the given chain id and current view. Messages from cur_view + 1 are cached for future
    // consumption, while messages from other views are dropped immediately.
    pub(crate) fn recv(
        &mut self,
        chain_id: ChainID,
        cur_view: ViewNumber,
        deadline: Instant,
    ) -> Result<(PublicKeyBytes, ProgressMessage), ProgressMessageReceiveError> {
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
                    // Cache the message if its for a future view.
                    else if msg.view() > cur_view {
                        let msg_queue = if let Some(msg_queue) = self.msg_buffer.get_mut(&cur_view)
                        {
                            msg_queue
                        } else {
                            self.msg_buffer.insert(cur_view, VecDeque::new());
                            self.msg_buffer.get_mut(&cur_view).unwrap()
                        };

                        msg_queue.push_back((sender, msg));
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

    pub(crate) fn send(&mut self, peer: PublicKeyBytes, msg: ProgressMessage) {
        self.network.send(peer, Message::ProgressMessage(msg))
    }

    pub(crate) fn broadcast(&mut self, msg: ProgressMessage) {
        self.network.broadcast(Message::ProgressMessage(msg))
    }

    pub(crate) fn update_validator_set(&mut self, updates: ValidatorSetUpdates) {
        self.network.update_validator_set(updates)
    }
}

pub(crate) enum ProgressMessageReceiveError {
    Timeout,
    ReceivedQCFromFuture,
}

pub(crate) struct SyncClientStub<N: Network> {
    network: N,
    responses: Receiver<(PublicKeyBytes, SyncResponse)>,
}

impl<N: Network> SyncClientStub<N> {
    pub(crate) fn new(
        network: N,
        responses: Receiver<(PublicKeyBytes, SyncResponse)>,
    ) -> SyncClientStub<N> {
        SyncClientStub { network, responses }
    }

    pub(crate) fn send_request(&mut self, peer: PublicKeyBytes, msg: SyncRequest) {
        self.network
            .send(peer, Message::SyncMessage(SyncMessage::SyncRequest(msg)));
    }

    pub(crate) fn recv_response(
        &self,
        peer: PublicKeyBytes,
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
    requests: Receiver<(PublicKeyBytes, SyncRequest)>,
    network: N,
}

impl<N: Network> SyncServerStub<N> {
    pub(crate) fn new(
        requests: Receiver<(PublicKeyBytes, SyncRequest)>,
        network: N,
    ) -> SyncServerStub<N> {
        SyncServerStub { requests, network }
    }

    pub(crate) fn recv_request(&self) -> Option<(PublicKeyBytes, SyncRequest)> {
        match self.requests.try_recv() {
            Ok((origin, request)) => Some((origin, request)),
            // Safety: the sync server thread (the only caller of this function) shuts down before the poller thread
            // (the sender side of this channel), so we will never be disconnected at this point.
            Err(TryRecvError::Disconnected) => panic!(),
            Err(TryRecvError::Empty) => None,
        }
    }

    pub(crate) fn send_response(&mut self, peer: PublicKeyBytes, msg: SyncResponse) {
        self.network
            .send(peer, Message::SyncMessage(SyncMessage::SyncResponse(msg)));
    }
}
