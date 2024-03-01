/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Functions that implement the progress protocol and sync protocol.

use std::cmp::max;
use std::sync::mpsc::{Sender, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::{Instant, SystemTime};

use crate::app::App;
use crate::app::ProduceBlockResponse;
use crate::app::{ProduceBlockRequest, ValidateBlockRequest, ValidateBlockResponse};
use crate::block_sync::client::BlockSyncClient;
use crate::block_sync::messages::{BlockSyncRequest, BlockSyncResponse};
use crate::hotstuff::messages::HotStuffMessage;
use crate::messages::ProgressMessage;
use crate::pacemaker::messages::PacemakerMessage;
use crate::{events::*, pacemaker};
use crate::hotstuff::protocol::HotStuff;
use crate::networking::*;
use crate::pacemaker::protocol::Pacemaker;
use crate::state::*;
use crate::types::*;

use self::basic::ChainID;
use self::keypair::Keypair;

pub(crate) struct Algorithm<N: Network, K: KVStore> {
    pm_stub: ProgressMessageStub<N>,
    block_tree: BlockTree<K>,
    hotstuff: HotStuff<N, K>,
    pacemaker: Pacemaker<N, K>,
    block_sync_client: BlockSyncClient<N, K>,
    shutdown_signal: Receiver<()>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network, K: KVStore> Algorithm<N, K> {

    pub(crate) fn new(
        keypair: Keypair,
        chain_id: ChainID,
        mut block_tree: BlockTree<K>,
        network: N,
        progress_msg_receiver: Receiver<ProgressMessage>,
        block_sync_response_receiver: Receiver<BlockSyncResponse>,
        shutdown_signal: Receiver<()>,
        event_publisher: Option<Sender<Event>>,
        // ProgressMessageStub parameters:
        msg_buffer_capacity: u64,
        // SyncClient parameters:
        block_sync_request_limit: u32,
        block_sync_response_timeout: Duration,
        // Pacemaker parameters:
        epoch_length: u32,
        view_time: Duration,
    ) -> Self {

        let highest_view_with_progress = max(block_tree.highest_view_entered(), block_tree.highest_qc().view);
        let init_view = 
            if highest_view_with_progress == 0 {
                0
            } else {
                highest_view_with_progress + 1
            };
        let init_leader = Pacemaker::view_leader(init_view, &block_tree.committed_validator_set());

        let pm_stub = ProgressMessageStub::new(network, progress_msg_receiver, msg_buffer_capacity);
        let pacemaker_msg_sender: SenderHandle<N, PacemakerMessage> = SenderHandle::new(network);
        let hotstuff_msg_sender: SenderHandle<N, HotStuffMessage> = SenderHandle::new(network);
        let block_sync_request_sender: SenderHandle<N, BlockSyncRequest> = SenderHandle::new(network);
        let validator_set_update_handle = ValidatorSetUpdateHandle::new(network);

        let pacemaker = Pacemaker::new(
            chain_id, 
            keypair,
            pacemaker_msg_sender,
            &block_tree, 
            init_view,
            init_leader,
            epoch_length, 
            view_time,
            event_publisher.clone()
        );

        let hotstuff = HotStuff::new(
            chain_id,
            keypair,
            hotstuff_msg_sender,
            validator_set_update_handle.clone(),
            init_view,
            init_leader,
            event_publisher.clone()
        );

        let block_sync_client = BlockSyncClient::new(
            chain_id, 
            block_sync_request_sender, 
            validator_set_update_handle,
            block_sync_request_limit, 
            block_sync_response_timeout, 
            event_publisher
        );

        Self {
            pm_stub,
            block_tree,
            hotstuff,
            pacemaker,
            block_sync_client,
            shutdown_signal,
            event_publisher
        }
    }

    pub(crate) fn start(&mut self) -> JoinHandle<()> {

        thread::spawn(move || {

            // call execute

        })
    }
}