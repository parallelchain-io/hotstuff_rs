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

use ed25519_dalek::VerifyingKey;

use crate::block_sync::messages::{BlockSyncMessage, BlockSyncRequest, BlockSyncResponse};
use crate::messages::*;
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
/// 1. Progress messages (processed by the [Algorithm][crate::algorithm::Algorithm]'s execute loop), and
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
                Message::BlockSyncMessage(s_msg) => match s_msg {
                    BlockSyncMessage::BlockSyncRequest(s_req) => {
                        let _ = to_sync_request_receiver.send((origin, s_req));
                    }
                    BlockSyncMessage::BlockSyncResponse(s_res) => {
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
/// All messages must match the chain id passed to the receiver to be accepted.
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
pub(crate) struct ProgressMessageStub {
    receiver: Receiver<(VerifyingKey, ProgressMessage)>,
    msg_buffer: ProgressMessageBuffer,
}

impl ProgressMessageStub {
    /// Create a fresh [ProgressMessageStub] with a given receiver end and buffer capacity.
    pub(crate) fn new(
        receiver: Receiver<(VerifyingKey, ProgressMessage)>,
        msg_buffer_capacity: BufferSize,
    ) -> ProgressMessageStub {
        let msg_buffer: ProgressMessageBuffer = ProgressMessageBuffer::new(msg_buffer_capacity);
        Self {
            receiver,
            msg_buffer,
        }
    }

    /// Receive a message matching the given chain id, and view >= current view (if any).
    /// Cache and/or return immediately, depending on the message type.
    /// Messages older than current view are dropped immediately.
    /// [BlockSyncTriggerMessage][crate::block_sync::messages::BlockSyncTriggerMessage] 
    /// messages do not have a view, and so they are returned immediately..
    pub(crate) fn recv(
        &mut self,
        chain_id: ChainID,
        cur_view: ViewNumber,
        deadline: Instant,
    ) -> Result<(VerifyingKey, ProgressMessage), ProgressMessageReceiveError> {
        // Clear buffer of messages with views lower than the current one.
        self.msg_buffer.remove_expired_msgs(cur_view);

        // Try to get buffered messages for the current view.
        if let Some((sender, msg)) = self.msg_buffer.get_msg(&cur_view) {
            return Ok((sender, msg))
        }

        // Try to get messages from the poller.
        while Instant::now() < deadline {
            match self.receiver.recv_timeout(deadline - Instant::now()) {
                Ok((sender, msg)) => {

                    if msg.chain_id() != chain_id {
                        continue;
                    }

                    // If the message is for a future view then cache it.
                    if msg.view().is_some_and(|view| view > cur_view) {
                        match msg.clone() {
                            ProgressMessage::HotStuffMessage(msg) => {self.msg_buffer.insert(msg, sender);},
                            ProgressMessage::PacemakerMessage(msg) => {self.msg_buffer.insert(msg, sender);},
                            _ => (),
                        }
                    }
                    
                    // Return the message if either:
                    // 1. It is a HotStuff message for the current view, or
                    // 2. If it is a Pacemaker message for the current view or a future view, or
                    // 3. If it is a BlockSyncTrigger message.
                    let return_msg = match &msg {
                        ProgressMessage::HotStuffMessage(hotstuff_msg) => hotstuff_msg.view() == cur_view,
                        ProgressMessage::PacemakerMessage(pacemaker_msg) => pacemaker_msg.view() >= cur_view,
                        ProgressMessage::BlockSyncTriggerMessage(_) => true,
                    };

                    if return_msg {
                        return Ok((sender, msg))
                    }

                },
                Err(RecvTimeoutError::Timeout) => thread::yield_now(),
                Err(RecvTimeoutError::Disconnected) => return Err(ProgressMessageReceiveError::Disconnected),
            }
        }

        Err(ProgressMessageReceiveError::Timeout)
    }

}

#[derive(Debug)]
pub(crate) enum ProgressMessageReceiveError {
    Timeout,
    Disconnected,
}

/// Message buffer intended for storing received [ProgressMessage]s for future views.
/// Its size is bounded by its capacity, and when the capacity is reached messages
/// for highest views may be removed.
struct ProgressMessageBuffer {
    buffer_capacity: BufferSize,
    buffer: BTreeMap<ViewNumber, VecDeque<(VerifyingKey, ProgressMessage)>>,
    buffer_size: BufferSize,
}

impl ProgressMessageBuffer {
    /// Create an empty message buffer.
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
    /// Returns whether the message was successfully inserted into the buffer.
    fn insert<M: Into<ProgressMessage> + Cacheable>(&mut self, msg: M, sender: VerifyingKey) -> bool {
        
        // Try to cache the message.
        let bytes_requested = mem::size_of::<VerifyingKey>() as u64 + msg.size();
        let new_buffer_size = self.buffer_size.get_int().checked_add(bytes_requested);
        let buffer_will_be_overloaded = new_buffer_size.is_none() || new_buffer_size.unwrap() > self.buffer_capacity.get_int();
        let cache_message_if_buffer_will_be_overloaded = self.buffer.keys().max().is_none()
                                                               || self.buffer.keys().max().is_some_and(|max_view| msg.view() < *max_view);

        // We only need to make space in the buffer if:
        // (1) It will be overloaded after storing the message, and 
        // (2) We want to store this message in the buffer, i.e., if the message's view is lower than that of the highest-viewed 
        //     message stored in the buffer
        // Otherwise we ignore the message to avoid overloading the buffer.
        if buffer_will_be_overloaded && cache_message_if_buffer_will_be_overloaded {
            self.remove_highest_viewed_msgs(bytes_requested);
        };

        // We only store the message in the buffer if either:
        // (1) There is no risk of overloading the buffer upon storing this message, or
        // (2) The buffer might be overloaded, but we have already made space for the new message.
        if !buffer_will_be_overloaded || (buffer_will_be_overloaded && cache_message_if_buffer_will_be_overloaded) {
            let msg_queue = 
                if let Some(msg_queue) = self.buffer.get_mut(&msg.view())
                {
                    msg_queue
                } else {
                    self.buffer.insert(msg.view(), VecDeque::new());
                    // Safety: this key has just been inserted.
                    self.buffer.get_mut(&msg.view()).unwrap()
                };

            self.buffer_size += bytes_requested;
            msg_queue.push_back((sender, msg.into()));
            return true
        };
        false
    }

    /// If there are messages for this view in the buffer, remove and return the message at the front of the queue.
    fn get_msg(&mut self, view: &ViewNumber) -> Option<(VerifyingKey, ProgressMessage)> {
        self.buffer.get_mut(view).map(|msg_queue| msg_queue.pop_front()).flatten()
    }

    /// Given the number of bytes that need to be removed, removes just enough highest-viewed messages
    /// to free up (at least) the required number of bytes in the buffer.
    fn remove_highest_viewed_msgs(&mut self, bytes_to_remove: u64) {
        let verifying_key_size = mem::size_of::<VerifyingKey>() as u64;
        
        let mut bytes_removed = 0;
        let mut views_removed = Vec::new();
        let mut msg_queues_iter = self.buffer.iter_mut().rev();

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

        self.buffer_size -= bytes_removed;
        
        // If some views in the message buffer have lost all messages as a result of the removal,
        // then also remove their corresponding keys from the buffer.
        views_removed.iter().for_each(|view| {let _ = self.buffer.remove(view);});
    }

    /// Remove all messages for views less than the current view.
    fn remove_expired_msgs(&mut self, cur_view: ViewNumber) {
        self.buffer = self.buffer.split_off(&cur_view)
    }
}

/// A receiving end for sync responses.
/// The [BlockSyncClientStub::recv_response] method returns the received response.
pub(crate) struct BlockSyncClientStub {
    responses: Receiver<(VerifyingKey, BlockSyncResponse)>,
}

impl BlockSyncClientStub {
    pub(crate) fn new(
        responses: Receiver<(VerifyingKey, BlockSyncResponse)>,
    ) -> BlockSyncClientStub {
        BlockSyncClientStub { responses }
    }

    /// Receive a [BlockSyncResponse] from a given peer.
    /// Waits for the response until the deadline is reached, and
    /// if no response is received it returns [BlockSyncResponseReceiveError::Timeout].
    pub(crate) fn recv_response(
        &self,
        peer: VerifyingKey,
        deadline: Instant,
    ) -> Result<BlockSyncResponse, BlockSyncResponseReceiveError> {
        while Instant::now() < deadline {
            match self.responses.recv_timeout(deadline - Instant::now()) {
                Ok((sender, sync_response)) => {
                    if sender == peer {
                        return Ok(sync_response);
                    }
                }
                Err(RecvTimeoutError::Timeout) => thread::yield_now(),
                Err(RecvTimeoutError::Disconnected) => return Err(BlockSyncResponseReceiveError::Disconnected),
            }
        }

        Err(BlockSyncResponseReceiveError::Timeout)
    }

}

#[derive(Debug)]
pub enum BlockSyncResponseReceiveError {
    Disconnected,
    Timeout,
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

    /// Receive a [BlockSyncRequest] if available, else return [BlockSyncRequestReceiveError::NotAvailable].
    pub(crate) fn recv_request(&self) -> Result<(VerifyingKey, BlockSyncRequest), BlockSyncRequestReceiveError> {
        match self.requests.try_recv() {
            Ok((origin, request)) => Ok((origin, request)),
            // Safety: the sync server thread (the only caller of this function) shuts down before the poller thread
            // (the sender side of this channel), so we will never be disconnected at this point.
            Err(TryRecvError::Disconnected) => Err(BlockSyncRequestReceiveError::Disconnected),
            Err(TryRecvError::Empty) => Err(BlockSyncRequestReceiveError::NotAvailable),
        }
    }
}

#[derive(Debug)]
pub enum BlockSyncRequestReceiveError {
    Disconnected,
    NotAvailable,
}
