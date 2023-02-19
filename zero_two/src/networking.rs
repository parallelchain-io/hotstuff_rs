/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! HotStuff-rs' has modular peer-to-peer networking, with each peer identified by a PublicKey. Networking providers
//! interact with HotStuff-rs' threads through implementations of the [Network] trait.

use std::sync::mpsc::{self, Sender, Receiver, RecvTimeoutError, TryRecvError, RecvError};
use std::thread::{self, JoinHandle};
use std::time::{Instant, Duration};
use ed25519_dalek::PublicKey;

use crate::types::{PublicKeyBytes, ViewNumber, ValidatorSet, AppID};
use crate::messages::*;

pub trait Network: Clone + Send + 'static {
    fn update_validator_set(&mut self, validator_set: ValidatorSet);

    fn broadcast(&mut self, message: Message);

    /// Send a message to the specified peer without blocking.
    fn send(&mut self, peer: PublicKeyBytes, message: Message);

    /// Receive a message from any peer.
    ///
    /// # Important
    /// Messages returned from this function should be deserialized using the Message's implementation of TryFrom<Vec<u8>>,
    /// which checks some simple invariants.
    fn recv(&mut self) -> Option<(PublicKeyBytes, Message)>;
}

/// Spawn the poller thread, which polls the Network for messages and distributes them into the ProgressMessageFilter,
/// or SyncRequestReceiver, or the SyncResponseReceiver according to their variant.
pub(crate) fn start_polling<N: Network>(mut network: N, shutdown_signal: Receiver<()>) -> (
    JoinHandle<()>,
    Receiver<(PublicKeyBytes, ProgressMessage)>,
    Receiver<(PublicKeyBytes, SyncRequest)>,
    Receiver<(PublicKeyBytes, SyncResponse)>,
) {
    let (to_progress_msg_filter, progress_msg_from_poller) = mpsc::channel();
    let (to_sync_request_receiver, sync_request_from_poller) = mpsc::channel();
    let (to_sync_response_receiver, sync_response_from_poller) = mpsc::channel();

    let poller_thread = thread::spawn(move || {
        loop {
            match shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => panic!("Poller thread disconnected from main thread"),
            }

            if let Some((origin, msg)) = network.recv() {
                match msg {
                    Message::ProgressMessage(p_msg) => to_progress_msg_filter.send((origin, p_msg)).unwrap(),
                    Message::SyncMessage(s_msg) => match s_msg {
                        SyncMessage::SyncRequest(s_req) => to_sync_request_receiver.send((origin, s_req)).unwrap(),
                        SyncMessage::SyncResponse(s_res) => to_sync_response_receiver.send((origin, s_res)).unwrap(),
                    }
                }
            }

            thread::yield_now();
        }
    });
    todo!();
    let progress_message_filter = ProgressMessageFilter {
        recycler: to_progress_msg_filter.clone(),
        receiver: progress_msg_from_poller,
    };
    let sync_request_receiver = SyncRequestReceiver(sync_request_from_poller);
    let sync_response_receiver = SyncResponseReceiver(sync_response_from_poller);

    (
        poller_thread,
        progress_message_filter,
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
        todo!()
    }

    pub(crate) fn recv(
        &self, 
        app_id: AppID,
        cur_view: ViewNumber, 
        highest_qc_view: ViewNumber, 
        deadline: Instant
    ) -> Result<(PublicKeyBytes, ProgressMessage), ProgressMessageReceiveError> {
        todo!() 
    }

    pub(crate) fn send(&self, peer: &PublicKeyBytes, msg: &ProgressMessage) {
        todo!()
    }

    pub(crate) fn broadcast(&self, msg: &ProgressMessage) {
        todo!()
    }
}

pub(crate) enum ProgressMessageReceiveError {
    Timeout,
    ReceivedQuorumFromFuture,
}
pub(crate) struct SyncClientStub<N: Network> {
    network: N,
    responses: Receiver<(PublicKeyBytes, SyncResponse)>,
}

impl<N: Network> SyncClientStub<N> {
    pub(crate) fn new(network: N, responses: Receiver<(PublicKeyBytes, SyncResponse)>) -> SyncClientStub<N> {
        SyncClientStub { network, responses }
    }

    pub(crate) fn send_request(&self, peer: &PublicKeyBytes, msg: &SyncRequest) {
        todo!()
    }

    pub(crate) fn recv_response(&self, peer: &PublicKeyBytes) -> SyncResponse {
        todo!()
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
        todo!()
    }

    pub(crate) fn send_response(&self, peer: &PublicKeyBytes, msg: SyncResponse) {
        todo!()
    }
}
