/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implements the [BlockSyncClient], which is reponsible for:
//! 1. Triggering block sync (when a replica can't make progress for long enough or sees evidence that others are ahead), and
//! 2. Managing the list of peers available as sync servers and a blacklist for sync servers that have provided incorrect information in the past, and
//! 3. Selecting a peer to sync with from the list of available peers, and
//! 4. The syncing process with a given peer.

use std::{collections::{HashMap, HashSet}, sync::mpsc::Sender, time::Duration};

use ed25519_dalek::VerifyingKey;

use crate::events::Event;
use crate::networking::{BlockSyncClientStub, Network, SenderHandle, ValidatorSetUpdateHandle};
use crate::state::{BlockTree, KVStore};
use crate::types::basic::ChainID;

use super::messages::{BlockSyncRequest, BlockSyncTriggerMessage};

pub(crate) struct BlockSyncClient<N: Network> {
    config: BlockSyncClientConfiguration,
    receiver: BlockSyncClientStub<N>,
    sender: SenderHandle<N>,
    validator_set_update_handle: ValidatorSetUpdateHandle<N>,
    block_sync_client_state: BlockSyncClientState,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network> BlockSyncClient<N> {

    pub(crate) fn new(
        config: BlockSyncClientConfiguration,
        receiver: BlockSyncClientStub<N>,
        sender: SenderHandle<N>,
        validator_set_update_handle: ValidatorSetUpdateHandle<N>,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        Self {
            config,
            receiver,
            sender,
            validator_set_update_handle,
            block_sync_client_state: BlockSyncClientState::initialize(),
            event_publisher
        }
    }

    pub(crate) fn on_receive_msg<K: KVStore>(&mut self, msg: BlockSyncTriggerMessage, origin: &VerifyingKey, block_tree: &BlockTree<K>) {
        todo!()
    }

    // TODO: other methods

    // fn sync()

    // fn tick()

}

/// Immutable parameters that define the behaviour of the [BlockSyncClient].
pub(crate) struct BlockSyncClientConfiguration {
    pub(crate) chain_id: ChainID,
    pub(crate) request_limit: u32,
    pub(crate) response_timeout: Duration,
}

struct BlockSyncClientState {
    sync_servers: HashMap<VerifyingKey, u64>,
    blacklist: HashSet<VerifyingKey>,
    // TODO: fields for timeout-based sync trigger
}

impl BlockSyncClientState {

    fn initialize() -> Self {
        Self {
            sync_servers: HashMap::new(),
            blacklist: HashSet::new(), 
        }
    }

    // TODO: other methods
}