/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Functions that [initialize](Replica::initialize) and [start](Replica::start) a replica, as well as [the type](Replica) which
//! keeps the replica alive.
//!
//! HotStuff-rs works to safely replicate a state machine in multiple processes. In our terminology, these processes are
//! called 'replicas', and therefore the set of all replicas is called the 'replica set'. Each replica is uniquely identified
//! by an Ed25519 public key.
//!
//! ## Validators and Listeners
//!
//! Not every replica has to vote in consensus. Some operators may want to run replicas that merely keep up with consensus
//! decisions, without having weight in them. We call these replicas 'listeners', and call the replicas that vote in
//! consensus 'validators'.
//!
//! HotStuff-rs needs to know the full 'validator set' at all times to collect votes, but does not need to know the identity
//! of listeners. But for listeners to keep up with consensus decisions, they also need to receive progress messages.
//!
//! Concretely, this requires that the library user's [networking provider's](crate::networking) broadcast method send progress
//! messages to all peers it is connected to, and not only the validators. The library user is free to design and implement
//! their own mechanism for deciding which peers, besides those in the validator set, should be connected to the network.

use crate::algorithm::start_algorithm;
use crate::app::App;
use crate::event_bus::*;
use crate::events::*;
use crate::messages;
use crate::networking::{
    start_polling, Network, ProgressMessageStub, SyncClientStub, SyncServerStub,
};
use crate::pacemaker::Pacemaker;
use crate::state::{BlockTree, BlockTreeCamera, KVStore};
use crate::sync_server::start_sync_server;
use crate::types::ChainID;
use crate::types::{AppStateUpdates, ValidatorSetUpdates, SigningKey};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::Duration;
use typed_builder::TypedBuilder;

#[derive(TypedBuilder)]
pub struct Configuration {
    pub me: SigningKey,
    pub chain_id: ChainID,
    pub sync_trigger_timeout: Duration,
    pub sync_request_limit: u32,
    pub sync_response_timeout: Duration,
    pub progress_msg_buffer_capacity: u64,
    pub log_events: bool,
}

#[derive(TypedBuilder)]
pub struct ReplicaBuilder<K: KVStore, A: App<K> + 'static, N: Network + 'static, P: Pacemaker + 'static> {
    // Required parameters
    app: A,
    network: N,
    kv_store: K,
    pacemaker: P,
    configuration: Configuration,
    // Optional parameters
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<InsertBlockEvent>| vec![handler_ptr]))]
    on_insert_block: Vec<HandlerPtr<InsertBlockEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<CommitBlockEvent>| vec![handler_ptr]))]
    on_commit_block: Vec<HandlerPtr<CommitBlockEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<PruneBlockEvent>| vec![handler_ptr]))]
    on_prune_block: Vec<HandlerPtr<PruneBlockEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<UpdateHighestQCEvent>| vec![handler_ptr]))]
    on_update_highest_qc: Vec<HandlerPtr<UpdateHighestQCEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<UpdateLockedViewEvent>| vec![handler_ptr]))]
    on_update_locked_view: Vec<HandlerPtr<UpdateLockedViewEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<UpdateValidatorSetEvent>| vec![handler_ptr]))]
    on_update_validator_set: Vec<HandlerPtr<UpdateValidatorSetEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<ProposeEvent>| vec![handler_ptr]))]
    on_propose: Vec<HandlerPtr<ProposeEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<NudgeEvent>| vec![handler_ptr]))]
    on_nudge: Vec<HandlerPtr<NudgeEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<VoteEvent>| vec![handler_ptr]))]
    on_vote: Vec<HandlerPtr<VoteEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<NewViewEvent>| vec![handler_ptr]))]
    on_new_view: Vec<HandlerPtr<NewViewEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<ReceiveProposalEvent>| vec![handler_ptr]))]
    on_receive_proposal: Vec<HandlerPtr<ReceiveProposalEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<ReceiveNudgeEvent>| vec![handler_ptr]))]
    on_receive_nudge: Vec<HandlerPtr<ReceiveNudgeEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<ReceiveVoteEvent>| vec![handler_ptr]))]
    on_receive_vote: Vec<HandlerPtr<ReceiveVoteEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<ReceiveNewViewEvent>| vec![handler_ptr]))]
    on_receive_new_view: Vec<HandlerPtr<ReceiveNewViewEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<StartViewEvent>| vec![handler_ptr]))]
    on_start_view: Vec<HandlerPtr<StartViewEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<ViewTimeoutEvent>| vec![handler_ptr]))]
    on_view_timeout: Vec<HandlerPtr<ViewTimeoutEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<CollectQCEvent>| vec![handler_ptr]))]
    on_collect_qc: Vec<HandlerPtr<CollectQCEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<StartSyncEvent>| vec![handler_ptr]))]
    on_start_sync: Vec<HandlerPtr<StartSyncEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<EndSyncEvent>| vec![handler_ptr]))]
    on_end_sync: Vec<HandlerPtr<EndSyncEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<ReceiveSyncRequestEvent>| vec![handler_ptr]))]
    on_receive_sync_request: Vec<HandlerPtr<ReceiveSyncRequestEvent>>,
    #[builder(default = Vec::with_capacity(1), setter(transform = |handler_ptr: HandlerPtr<SendSyncResponseEvent>| vec![handler_ptr]))]
    on_send_sync_response: Vec<HandlerPtr<SendSyncResponseEvent>>,
}

impl<K: KVStore, A: App<K> + 'static, N: Network + 'static, P: Pacemaker + 'static> ReplicaBuilder<K, A, N, P> {

    pub fn start(mut self) -> Replica<K> {
        let block_tree = BlockTree::new(self.kv_store.clone());

        self.network.init_validator_set(block_tree.committed_validator_set());
        let (poller_shutdown, poller_shutdown_receiver) = mpsc::channel();
        let (poller, progress_msgs, sync_requests, sync_responses) =
            start_polling(self.network.clone(), poller_shutdown_receiver);
        let pm_stub = ProgressMessageStub::new(self.network.clone(), progress_msgs, self.configuration.progress_msg_buffer_capacity);
        let sync_server_stub = SyncServerStub::new(sync_requests, self.network.clone());
        let sync_client_stub = SyncClientStub::new(self.network, sync_responses);

        let mut event_handlers = EventHandlers {
            insert_block_handlers: self.on_insert_block,
            commit_block_handlers: self.on_commit_block,
            prune_block_handlers: self.on_prune_block,
            update_highest_qc_handlers: self.on_update_highest_qc,
            update_locked_view_handlers: self.on_update_locked_view,
            update_validator_set_handlers: self.on_update_validator_set,
            propose_handlers: self.on_propose,
            nudge_handlers: self.on_nudge,
            vote_handlers: self.on_vote,
            new_view_handlers: self.on_new_view,
            receive_proposal_handlers: self.on_receive_proposal,
            receive_nudge_handlers: self.on_receive_nudge,
            receive_vote_handlers: self.on_receive_vote,
            receive_new_view_handlers: self.on_receive_new_view,
            start_view_handlers: self.on_start_view,
            view_timeout_handlers: self.on_view_timeout,
            collect_qc_handlers: self.on_collect_qc,
            start_sync_handlers: self.on_start_sync,
            end_sync_handlers: self.on_end_sync,
            receive_sync_request_handlers: self.on_receive_sync_request,
            send_sync_response_handlers: self.on_send_sync_response
        };

        if self.configuration.log_events {
            event_handlers.add_logging_handlers();
        };

        let (event_publisher, event_subscriber) = 
            if !event_handlers.is_empty() {
                Some(mpsc::channel()).unzip()
            } else { (None, None) };

        let (sync_server_shutdown, sync_server_shutdown_receiver) = mpsc::channel();
        let sync_server = start_sync_server(
            BlockTreeCamera::new(self.kv_store.clone()),
            sync_server_stub,
            sync_server_shutdown_receiver,
            self.configuration.sync_request_limit,
            event_publisher.clone(),
        );

        let (algorithm_shutdown, algorithm_shutdown_receiver) = mpsc::channel();
        let algorithm = start_algorithm(
            self.app,
            messages::Keypair::new(self.configuration.me),
            self.configuration.chain_id,
            block_tree,
            self.pacemaker,
            pm_stub,
            sync_client_stub,
            algorithm_shutdown_receiver,
            event_publisher,
            self.configuration.sync_request_limit,
            self.configuration.sync_response_timeout
        );

        let (event_bus_shutdown, event_bus_shutdown_receiver) =
            if !event_handlers.is_empty() {
                Some(mpsc::channel()).unzip()
            } else { (None, None) };

        let event_bus = if !event_handlers.is_empty() {
            Some(
                start_event_bus(
                event_handlers,
                event_subscriber.unwrap(), // Safety: should be Some(...)
                event_bus_shutdown_receiver.unwrap(), // Safety: should be Some(...)
                )
            )
        } else {
            None
        };
        
        Replica {
            block_tree_camera: BlockTreeCamera::new(self.kv_store),
            poller: Some(poller),
            poller_shutdown,
            algorithm: Some(algorithm),
            algorithm_shutdown,
            sync_server: Some(sync_server),
            sync_server_shutdown,
            event_bus,
            event_bus_shutdown,
        }
    }
}

pub struct Replica<K: KVStore> {
    block_tree_camera: BlockTreeCamera<K>,
    poller: Option<JoinHandle<()>>,
    poller_shutdown: Sender<()>,
    algorithm: Option<JoinHandle<()>>,
    algorithm_shutdown: Sender<()>,
    sync_server: Option<JoinHandle<()>>,
    sync_server_shutdown: Sender<()>,
    event_bus: Option<JoinHandle<()>>,
    event_bus_shutdown: Option<Sender<()>>,
}

impl<K: KVStore> Replica<K> {
    pub fn initialize(
        kv_store: K,
        initial_app_state: AppStateUpdates,
        initial_validator_set: ValidatorSetUpdates,
    ) {
        let mut block_tree = BlockTree::new(kv_store);
        block_tree.initialize(&initial_app_state, &initial_validator_set);
    }

    pub fn block_tree_camera(&self) -> &BlockTreeCamera<K> {
        &self.block_tree_camera
    }
}

impl<K: KVStore> Drop for Replica<K> {
    fn drop(&mut self) {
        // Safety: the order of thread shutdown in this function is important, as the threads make assumptions about
        // the validity of their channels based on this. The algorithm and sync server threads receive messages from
        // the poller, and assumes that the poller will live longer than them.

        self.event_bus_shutdown.iter().for_each(|shutdown| shutdown.send(()).unwrap());
        if self.event_bus.is_some() {
            self.event_bus.take().unwrap().join().unwrap();
        }

        self.algorithm_shutdown.send(()).unwrap();
        self.algorithm.take().unwrap().join().unwrap();

        self.sync_server_shutdown.send(()).unwrap();
        self.sync_server.take().unwrap().join().unwrap();

        self.poller_shutdown.send(()).unwrap();
        self.poller.take().unwrap().join().unwrap();
    }
}
