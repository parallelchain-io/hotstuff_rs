use std::{collections::{HashMap, HashSet}, sync::mpsc::Sender, time::Duration};

use ed25519_dalek::VerifyingKey;

use crate::{events::Event, networking::{Network, SenderHandle, ValidatorSetUpdateHandle}, state::KVStore, types::basic::ChainID};

use super::messages::{BlockSyncRequest, BlockSyncTriggerMessage};

pub(crate) struct BlockSyncClient<N: Network, K: KVStore> {
    chain_id: ChainID,
    request_sender: SenderHandle<N, BlockSyncRequest>,
    validator_set_update_handle: ValidatorSetUpdateHandle<N>,
    request_limit: u32,
    response_timeout: Duration,
    block_sync_client_state: BlockSyncClientState,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network, K: KVStore> BlockSyncClient<N, K> {

    pub(crate) fn new(
        chain_id: ChainID,
        request_sender: SenderHandle<N, BlockSyncRequest>,
        validator_set_update_handle: ValidatorSetUpdateHandle<N>,
        request_limit: u32,
        response_timeout: Duration,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        Self {
            chain_id, 
            request_sender,
            validator_set_update_handle,
            request_limit,
            response_timeout,
            block_sync_client_state: BlockSyncClientState::initialize(),
            event_publisher
        }
    }

    pub(crate) fn on_receive_msg(msg: BlockSyncTriggerMessage, origin: VerifyingKey) {
        todo!()
    }

    // TODO: other methods

    // fn sync()
}

struct BlockSyncClientState {
    sync_servers: HashMap<VerifyingKey, u64>,
    blacklist: HashSet<VerifyingKey>,
    // todo: fields for timeout-based sync trigger
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