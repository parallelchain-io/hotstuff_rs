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
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use ed25519_dalek::VerifyingKey;

use crate::block_sync::messages::{BlockSyncRequest, BlockSyncResponse};
use crate::hotstuff::messages::HotStuffMessage;
use crate::messages::*;
use crate::pacemaker::messages::PacemakerMessage;
use crate::types::basic::{ChainID, ViewNumber};
use crate::types::validators::{ValidatorSet, ValidatorSetUpdates};

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

/// Spawn the poller thread, which polls the Network for messages and distributes them into receivers for:
/// 1. Progress messages (processed by the [Algorithm]), 
/// 2. Block sync requests (processed by [BlockSyncServer]), and 
/// 3. Block sync responses (processed by [BlockSyncClient]).
pub(crate) fn start_polling<N: Network + 'static>(
    mut network: N,
    shutdown_signal: Receiver<()>,
) -> (
    JoinHandle<()>,
    Receiver<(VerifyingKey, ProgressMessage)>,
    Receiver<(VerifyingKey, BlockSyncRequest)>,
    Receiver<(VerifyingKey, BlockSyncResponse)>,
) {
    todo!()
}

/// Handle for sending and broadcasting messages to the Network.
/// Can be instantiated for message types that implement the [Into<Message>] trait.
/// Each type that implements the trait encapsulates all types of messages that can be sent from a paricular object. 
pub(crate) struct SenderHandle<N: Network, S: Into<Message>> {
    network: N,
}

impl<N: Network, S: Into<Message>> SenderHandle<N, S> {
    pub(crate) fn new(network: N) -> Self {
        Self { network }
    }

    pub(crate) fn send(&mut self, peer: VerifyingKey, msg: S) {
        self.network.send(peer, Message::msg.into())
    }

    pub(crate) fn broadcast(&mut self, msg: ProgressMessage) {
        self.network.broadcast(Message::ProgressMessage(msg))
    }
}

/// Handle for informing the network provider about the validator set updates.
pub(crate) struct ValidatorSetUpdateHandle<N: Network> {
    network: N
}

impl<N: Network> ValidatorSetUpdateHandle<N> {
    pub(crate) fn new(network: N) -> Self {
        Self { network }
    }

    pub(crate) fn update_validator_set(&mut self, updates: ValidatorSetUpdates) {
        self.network.update_validator_set(updates)
    }
}

/// A receiving end for progress messages. Performs pre-processing of the received messages,
/// returning the messages immediately or storing them in the buffer.
///
/// All messages must match the chain id to be accepted.
///
/// ### HotStuff Messages
/// This type's recv method only returns hotstuff messages from the specified current view, and caches messages from
/// future views for later consumption. This helps prevents interruptions to progress when replicas' views
/// are mostly synchronized but they enter views at slightly different times.
/// 
/// ### Pacemaker Messages
/// This type's recv method returns pacemaker messages for any view greater or equal to the current view.
/// It also caches messages for view greater than the current view, for processing in the appropriate view
/// in case immediate processing is not possible.
///
/// ### BlockSyncTrigger Messages
/// This type's recv method returns block sync trigger messages immediately without caching.
///
/// ### Buffer management
/// If a message buffer grows beyond its maximum capacity, some highest-viewed messages might be removed 
/// from the buffer to make space for the new message.
pub(crate) struct ProgressMessageStub<N: Network> {
    network: N,
    receiver: Receiver<(VerifyingKey, ProgressMessage)>,
    hotstuff_msg_buffer: MessageBuffer<HotStuffMessage>,
    pacemaker_msg_buffer: MessageBuffer<PacemakerMessage>,
}

impl<N: Network> ProgressMessageStub<N> {
    pub(crate) fn new(
        network: N,
        receiver: Receiver<(VerifyingKey, ProgressMessage)>,
        msg_buffer_capacity: u64,
    ) -> ProgressMessageStub<N> {
        let hotstuff_msg_buffer: MessageBuffer<HotStuffMessage> = MessageBuffer::new(msg_buffer_capacity);
        let pacemaker_msg_buffer: MessageBuffer<PacemakerMessage> = MessageBuffer::new(msg_buffer_capacity);
        Self {
            network,
            receiver,
            hotstuff_msg_buffer,
            pacemaker_msg_buffer,
        }
    }

    /// Receive a message matching the given chain id, and view >= current view.
    /// Cache and/or return immediately, depending on the message type.
    /// Messages older than current view are dropped immediately.
    /// [BlockSyncTriggerMessage] messages are an exception since they do not have a view,
    /// and they are returned immediately, as long as the chain id is correct.
    pub(crate) fn recv(
        &mut self,
        chain_id: ChainID,
        cur_view: ViewNumber,
        deadline: Instant,
    ) -> Result<(VerifyingKey, ProgressMessage), ProgressMessageReceiveError> {
         todo!()
    }

}

pub(crate) enum ProgressMessageReceiveError {
    Timeout,
}

pub(crate) trait Cacheable {}
impl Cacheable for HotStuffMessage {}
impl Cacheable for PacemakerMessage {}

pub(crate) struct FailedToInsertError {}

pub(crate) struct MessageBuffer<C: Cacheable> {
    buffer_capacity: u64,
    buffer: BTreeMap<ViewNumber, VecDeque<(VerifyingKey, HotStuffMessage)>>,
    buffer_size: u64,
}

impl<C: Cacheable> MessageBuffer<C> {
    fn new(buffer_capacity: u64) {
        Self {
            buffer_capacity,
            buffer: BTreeMap::new(),
            buffer_size: 0,
        }
    }

    /// Try inserting the message into the buffer.
    /// In case caching the message makes the buffer grow beyond its capacity, this function either:
    /// 1. If the message has the highest view among the views of messages currently in the buffer, then the message is dropped, or
    /// 2. Otherwise, just enough highest-viewed messages are removed from the buffer to make space for the new message.
    fn insert_to_buffer(msg: HotStuffMessage, origin: VerifyingKey) -> Result<(), FailedToInsertError> {
        todo!()
    }

    /// Given the number of bytes that need to be removed, removes just enough highest-viewed messages
    /// to free up (at least) the required number of bytes in the buffer.
    fn remove_from_overloaded_buffer(&mut self, bytes_to_remove: u64) {
        todo!()
    }
}

pub(crate) struct BlockSyncClientStub<N: Network> {
    network: N,
    responses: Receiver<(VerifyingKey, BlockSyncResponse)>,
}

impl<N: Network> BlockSyncClientStub<N> {
    pub(crate) fn new(
        network: N,
        responses: Receiver<(VerifyingKey, BlockSyncResponse)>,
    ) -> BlockSyncClientStub<N> {
        BlockSyncClientStub { network, responses }
    }

    pub(crate) fn recv_response(
        &self,
        peer: VerifyingKey,
        deadline: Instant,
    ) -> Option<BlockSyncResponse> {
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

}

pub(crate) struct BlockSyncServerStub<N: Network> {
    network: N,
    requests: Receiver<(VerifyingKey, BlockSyncRequest)>,
}

/// A sending and receiving end for sync messages. 
/// The [SyncServerStub::recv_request] method returns the received request.
impl<N: Network> BlockSyncServerStub<N> {
    pub(crate) fn new(
        network: N,
        requests: Receiver<(VerifyingKey, BlockSyncRequest)>,
    ) -> BlockSyncServerStub<N> {
        BlockSyncServerStub { network, requests }
    }

    pub(crate) fn recv_request(&self) -> Option<(VerifyingKey, BlockSyncRequest)> {
        match self.requests.try_recv() {
            Ok((origin, request)) => Some((origin, request)),
            // Safety: the sync server thread (the only caller of this function) shuts down before the poller thread
            // (the sender side of this channel), so we will never be disconnected at this point.
            Err(TryRecvError::Disconnected) => panic!(),
            Err(TryRecvError::Empty) => None,
        }
    }
}
