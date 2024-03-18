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
use crate::types::basic::{BufferSize, ChainID, ViewNumber};
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

/// Spawn the poller thread, which polls the [Network] for messages and distributes them into receivers for:
/// 1. Progress messages (processed by the [Algorithm][crate::algorithm::Algorithm]), and
/// 2. Block sync requests (processed by [BlockSyncServer][crate::block_sync::server::BlockSyncServer]), and 
/// 3. Block sync responses (processed by [BlockSyncClient][crate::block_sync::client::BlockSyncClient]).
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

/// Handle for sending and broadcasting messages to the [Network].
/// Can be used to send or broadcast messages of message types that 
/// implement the [Into<Message>] trait. 
#[derive(Clone)]
pub(crate) struct SenderHandle<N: Network> {
    network: N,
}

impl<N: Network> SenderHandle<N> {
    pub(crate) fn new(network: N) -> Self {
        Self { network }
    }

    pub(crate) fn send<S: Into<Message>>(&mut self, peer: VerifyingKey, msg: S) {
        self.network.send(peer, msg.into())
    }

    pub(crate) fn broadcast<S: Into<Message>>(&mut self, msg: S) {
        self.network.broadcast(msg.into())
    }
}

/// Handle for informing the network provider about the validator set updates.
#[derive(Clone)]
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
/// This type's recv method only returns hotstuff messages for the current view, and caches messages from
/// future views for future consumption. This helps prevent interruptions to progress when replicas' views
/// are mostly synchronized but they enter views at slightly different times.
/// 
/// ### Pacemaker Messages
/// This type's recv method returns pacemaker messages for any view greater or equal to the current view.
/// It also caches all messages for view greater than the current view, for processing in the appropriate view
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
        msg_buffer_capacity: BufferSize,
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
    /// [BlockSyncTriggerMessage][crate::block_sync::messages::BlockSyncTriggerMessage] 
    /// messages are an exception since they do not have a view,
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

struct FailedToInsert;

struct MessageBuffer<M> {
    buffer_capacity: BufferSize,
    buffer: BTreeMap<ViewNumber, VecDeque<(VerifyingKey, M)>>,
    buffer_size: BufferSize,
}

impl<M> MessageBuffer<M> {
    fn new(buffer_capacity: BufferSize) -> Self {
        Self {
            buffer_capacity,
            buffer: BTreeMap::new(),
            buffer_size: BufferSize::new(0),
        }
    }

    /// Try inserting the message into the buffer.
    /// In case caching the message makes the buffer grow beyond its capacity, this function either:
    /// 1. If the message has the highest view among the views of messages currently in the buffer, then the message is dropped, or
    /// 2. Otherwise, just enough highest-viewed messages are removed from the buffer to make space for the new message.
    fn insert(msg: M, origin: VerifyingKey) -> Result<(), FailedToInsert> {
        todo!()
    }

    /// Given the number of bytes that need to be removed, removes just enough highest-viewed messages
    /// to free up (at least) the required number of bytes in the buffer.
    fn remove_highest_viewed_msgs(&mut self, bytes_to_remove: u64) {
        todo!()
    }
}

/// A receiving end for sync responses.
/// The [BlockSyncClientStub::recv_response] method returns the received response.
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

/// A receiving end for sync requests. 
/// The [BlockSyncServerStub::recv_request] method returns the received request.
pub(crate) struct BlockSyncServerStub<N: Network> {
    network: N,
    requests: Receiver<(VerifyingKey, BlockSyncRequest)>,
}

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
