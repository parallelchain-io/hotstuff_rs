/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implements the [BlockSyncServer], which is reponsible for:
//! 1. Periodically publishing the replica's highest known block (or highest_qc.block) by broadcasting
//!    an [AdvertiseBlock] message, and
//! 2. Responding to received sync requests.

use std::{sync::mpsc::{Receiver, Sender}, thread::{self, JoinHandle}};

use ed25519_dalek::VerifyingKey;

use crate::{events::Event, state::{block_tree_camera::BlockTreeCamera, kv_store::KVStore}};
use crate::networking::{BlockSyncServerStub, Network, SenderHandle};
use crate::types::{basic::ChainID, keypair::Keypair};

use super::messages::{AdvertiseBlock, BlockSyncRequest, BlockSyncResponse};

pub struct BlockSyncServer<N: Network + 'static, K: KVStore> {
    config: BlockSyncServerConfiguration,
    block_tree_camera: BlockTreeCamera<K>,
    receiver: BlockSyncServerStub<N>,
    sender: SenderHandle<N>,
    shutdown_signal: Receiver<()>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network + 'static, K: KVStore> BlockSyncServer<N, K> {

    pub(crate) fn new(
        config: BlockSyncServerConfiguration,
        block_tree_camera: BlockTreeCamera<K>,
        requests: Receiver<(VerifyingKey, BlockSyncRequest)>,
        network: N,
        shutdown_signal: Receiver<()>,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        Self { 
            config, 
            block_tree_camera,
            receiver: BlockSyncServerStub::new(network.clone(), requests),
            sender: SenderHandle::new(network),
            shutdown_signal,
            event_publisher
        }
    }

    pub(crate) fn start(mut self) -> JoinHandle<()> {

        thread::spawn(move || {

            todo!()

        })
    }
}

/// Immutable parameters that define the behaviour of the [BlockSyncServer].
pub(crate) struct BlockSyncServerConfiguration {
    pub(crate) chain_id: ChainID,
    pub(crate) keypair: Keypair,
    pub(crate) request_limit: u32,
}