/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! [Trait definition](Network) for pluggable peer-to-peer networking, as well as the internal types and functions
//! that replicas use to interact with the network.
//!
//! HotStuff-rs' has modular peer-to-peer networking, with each peer reachable by their PublicKey. Networking providers
//! interact with HotStuff-rs' threads through implementations of the [Network] trait. This trait has five methods
//! that collectively allow peers to exchange progress protocol and sync protocol messages.  

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::types::{PublicKeyBytes, ViewNumber, ValidatorSet, AppID, ValidatorSetUpdates};
use crate::messages::*;

pub trait Network: Clone + Send {
    /// Informs the network provider the validator set on wake-up.
    fn init_validator_set(&mut self, validator_set: ValidatorSet);

    /// Informs the networking provider of updates to the validator set.
    fn update_validator_set(&mut self, updates: ValidatorSetUpdates);

    /// Send a message to all peers (including listeners).
    fn broadcast(&mut self, message: Message);

    /// Send a message to the specified peer without blocking.
    fn send(&mut self, peer: PublicKeyBytes, message: Message);

    /// Receive a message from any peer.
    fn recv(&mut self) -> Option<(PublicKeyBytes, Message)>;
}

/// Spawn the poller thread, which polls the Network for messages and distributes them into receivers for
/// progress messages, sync requests, and sync responses.
pub(crate) fn start_polling<N: Network + 'static>(mut network: N, shutdown_signal: Receiver<()>) -> (
    JoinHandle<()>,
    Receiver<(PublicKeyBytes, ProgressMessage)>,
    Receiver<(PublicKeyBytes, SyncRequest)>,
    Receiver<(PublicKeyBytes, SyncResponse)>,
) {
    let (to_progress_msg_receiver, progress_msg_receiver) = mpsc::channel();
    let (to_sync_request_receiver, sync_request_receiver) = mpsc::channel();
    let (to_sync_response_receiver, sync_response_receiver) = mpsc::channel();

    let poller_thread = thread::spawn(move || {
        loop {
            match shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => panic!("Poller thread disconnected from main thread"),
            }

            if let Some((origin, msg)) = network.recv() {
                match msg {
                    Message::ProgressMessage(p_msg) => to_progress_msg_receiver.send((origin, p_msg)).unwrap(),
                    Message::SyncMessage(s_msg) => match s_msg {
                        SyncMessage::SyncRequest(s_req) => to_sync_request_receiver.send((origin, s_req)).unwrap(),
                        SyncMessage::SyncResponse(s_res) => to_sync_response_receiver.send((origin, s_res)).unwrap(),
                    }
                }
            } else {
                thread::yield_now()
            }
        }
    });
    (
        poller_thread,
        progress_msg_receiver,
        sync_request_receiver,
        sync_response_receiver,
    )
}

pub(crate) struct ProgressMessageStub<N: Network> {
    network: N,
    receiver: Receiver<(PublicKeyBytes, ProgressMessage)>,
}

impl<N: Network> ProgressMessageStub<N> {
    pub(crate) fn new(network: N, receiver: Receiver<(PublicKeyBytes, ProgressMessage)>) -> ProgressMessageStub<N> {
        Self {
            network,
            receiver
        }
    }

    // Receive a message matching the given app id and current view.
    pub(crate) fn recv(
        &mut self, 
        app_id: AppID,
        cur_view: ViewNumber, 
        deadline: Instant
    ) -> Result<(PublicKeyBytes, ProgressMessage), ProgressMessageReceiveError> {
        while Instant::now() < deadline {
            match self.receiver.recv_timeout(deadline - Instant::now()) {
                Ok((sender, msg)) => {
                    if msg.app_id() != app_id {
                        continue
                    }

                    let received_qc_from_future = match &msg {
                        ProgressMessage::Proposal(Proposal { block, .. }) => block.justify.view > cur_view,
                        ProgressMessage::Nudge(Nudge { justify, .. }) => justify.view > cur_view,
                        ProgressMessage::NewView(NewView { highest_qc, .. }) => highest_qc.view > cur_view,
                        _ => false,
                    };

                    if received_qc_from_future {
                        return Err(ProgressMessageReceiveError::ReceivedQCFromFuture)
                    } else {
                        if msg.view() == cur_view {
                            return Ok((sender, msg))
                        }
                    }
                },
                Err(RecvTimeoutError::Timeout) => thread::yield_now(),
                Err(RecvTimeoutError::Disconnected) => panic!()
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
    pub(crate) fn new(network: N, responses: Receiver<(PublicKeyBytes, SyncResponse)>) -> SyncClientStub<N> {
        SyncClientStub { network, responses }
    }

    pub(crate) fn send_request(&mut self, peer: PublicKeyBytes, msg: SyncRequest) {
        self.network.send(peer, Message::SyncMessage(SyncMessage::SyncRequest(msg)));
    }

    pub(crate) fn recv_response(&self, peer: PublicKeyBytes, deadline: Instant) -> Option<SyncResponse> {
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
    pub(crate) fn new(requests: Receiver<(PublicKeyBytes, SyncRequest)>, network: N) -> SyncServerStub<N> {
        SyncServerStub { requests, network }
    }

    pub(crate) fn recv_request(&self) -> (PublicKeyBytes, SyncRequest) {
        self.requests.recv().unwrap()
    }

    pub(crate) fn send_response(&mut self, peer: PublicKeyBytes, msg: SyncResponse) {
        self.network.send(peer, Message::SyncMessage(SyncMessage::SyncResponse(msg)));
    }
}
