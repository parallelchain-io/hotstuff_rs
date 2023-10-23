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
use crate::types::{AppStateUpdates, ValidatorSetUpdates, SigningKey, ChainID};
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
pub struct ReplicaSpec<K: KVStore, A: App<K> + 'static, N: Network + 'static, P: Pacemaker + 'static> {
    // Required parameters
    app: A,
    network: N,
    kv_store: K,
    pacemaker: P,
    configuration: Configuration,
    // Optional parameters
    #[builder(default, setter(transform = |handler: impl Fn(&InsertBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<InsertBlockEvent>)))]
    on_insert_block: Option<HandlerPtr<InsertBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&CommitBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<CommitBlockEvent>)))]
    on_commit_block: Option<HandlerPtr<CommitBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&PruneBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<PruneBlockEvent>)))]
    on_prune_block: Option<HandlerPtr<PruneBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateHighestQCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateHighestQCEvent>)))]
    on_update_highest_qc: Option<HandlerPtr<UpdateHighestQCEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateLockedViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateLockedViewEvent>)))]
    on_update_locked_view: Option<HandlerPtr<UpdateLockedViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateValidatorSetEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateValidatorSetEvent>)))]
    on_update_validator_set: Option<HandlerPtr<UpdateValidatorSetEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ProposeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ProposeEvent>)))]
    on_propose: Option<HandlerPtr<ProposeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&NudgeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<NudgeEvent>)))]
    on_nudge: Option<HandlerPtr<NudgeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&VoteEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<VoteEvent>)))]
    on_vote: Option<HandlerPtr<VoteEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&NewViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<NewViewEvent>)))]
    on_new_view: Option<HandlerPtr<NewViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveProposalEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveProposalEvent>)))]
    on_receive_proposal: Option<HandlerPtr<ReceiveProposalEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveNudgeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveNudgeEvent>)))]
    on_receive_nudge: Option<HandlerPtr<ReceiveNudgeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveVoteEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveVoteEvent>)))]
    on_receive_vote: Option<HandlerPtr<ReceiveVoteEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveNewViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveNewViewEvent>)))]
    on_receive_new_view: Option<HandlerPtr<ReceiveNewViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&StartViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<StartViewEvent>)))]
    on_start_view: Option<HandlerPtr<StartViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ViewTimeoutEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ViewTimeoutEvent>)))]
    on_view_timeout: Option<HandlerPtr<ViewTimeoutEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&CollectQCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<CollectQCEvent>)))]
    on_collect_qc: Option<HandlerPtr<CollectQCEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&StartSyncEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<StartSyncEvent>)))]
    on_start_sync: Option<HandlerPtr<StartSyncEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&EndSyncEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<EndSyncEvent>)))]
    on_end_sync: Option<HandlerPtr<EndSyncEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveSyncRequestEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveSyncRequestEvent>)))]
    on_receive_sync_request: Option<HandlerPtr<ReceiveSyncRequestEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&SendSyncResponseEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<SendSyncResponseEvent>)))]
    on_send_sync_response: Option<HandlerPtr<SendSyncResponseEvent>>,
}

impl<K: KVStore, A: App<K> + 'static, N: Network + 'static, P: Pacemaker + 'static> ReplicaSpec<K, A, N, P> {

    pub fn start(mut self) -> Replica<K> {
        let block_tree = BlockTree::new(self.kv_store.clone());

        self.network.init_validator_set(block_tree.committed_validator_set());
        let (poller_shutdown, poller_shutdown_receiver) = mpsc::channel();
        let (poller, progress_msgs, sync_requests, sync_responses) =
            start_polling(self.network.clone(), poller_shutdown_receiver);
        let pm_stub = ProgressMessageStub::new(self.network.clone(), progress_msgs, self.configuration.progress_msg_buffer_capacity);
        let sync_server_stub = SyncServerStub::new(sync_requests, self.network.clone());
        let sync_client_stub = SyncClientStub::new(self.network, sync_responses);

        let event_handlers = EventHandlers::new(
            self.configuration.log_events,
            self.on_insert_block,
            self.on_commit_block,
            self.on_prune_block,
            self.on_update_highest_qc,
            self.on_update_locked_view,
            self.on_update_validator_set,
            self.on_propose,
            self.on_nudge,
            self.on_vote,
            self.on_new_view,
            self.on_receive_proposal,
            self.on_receive_nudge,
            self.on_receive_vote,
            self.on_receive_new_view,
            self.on_start_view,
            self.on_view_timeout,
            self.on_collect_qc,
            self.on_start_sync,
            self.on_end_sync,
            self.on_receive_sync_request,
            self.on_send_sync_response
        );

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
            self.configuration.sync_response_timeout,
            self.configuration.sync_trigger_timeout
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
