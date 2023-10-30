/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Functions that [initialize](Replica::initialize) and [start](Replica::start) a replica, as well as [the type](Replica) which
//! keeps the replica alive.
//!
//! HotStuff-rs works to safely replicate a state machine in multiple processes. In our terminology, these processes are
//! called 'replicas', and therefore the set of all replicas is called the 'replica set'. Each replica is uniquely identified
//! by an Ed25519 public key (ed25519-dalek::VerifyingKey).
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
#[builder(doc)]
pub struct Configuration {
    #[builder(setter(doc = "Sets the replica's keypair, used to sign messages. Required."))]
    pub me: SigningKey,
    #[builder(setter(doc = "Sets the chain ID of the blockchain. Required."))]
    pub chain_id: ChainID,
    #[builder(setter(doc = "Sets the limit for the number of blocks that can be sent from the replica's sync server to a syncing peer. Required."))]
    pub sync_request_limit: u32,
    #[builder(setter(doc = "Sets the timeout for receiving sync responses from a peer (in seconds). Required."))]
    pub sync_response_timeout: Duration,
    #[builder(setter(doc = "Sets the maximum number of bytes that can be stored in the replica's message buffer at any given moment. Required."))]
    pub progress_msg_buffer_capacity: u64,
    #[builder(setter(doc = "Enables/disables logging. Required."))]
    pub log_events: bool,
}

#[derive(TypedBuilder)]
#[builder(doc)]
pub struct ReplicaSpec<K: KVStore, A: App<K> + 'static, N: Network + 'static, P: Pacemaker + 'static> {
    // Required parameters
    #[builder(setter(doc = "Sets the application code to be run on the blockchain. The argument must implement the App trait. This setter is required to start a replica."))]
    app: A,
    #[builder(setter(doc = "Sets the implementation of peer-to-peer networking. The argument must implement the Network trait. This setter is required to start a replica."))]
    network: N,
    #[builder(setter(doc = "Sets the implementation of the replica's Key-Value store. The argument must implement the KVStore trait. This setter is required to start a replica."))]
    kv_store: K,
    #[builder(setter(doc = "Sets the implementation of the pacemaker. The argument must implement the Pacemaker trait. This setter is required to start a replica."))]
    pacemaker: P,
    #[builder(setter(doc = "Sets the configuration, which contains the necessary parameters to run a replica. This setter is required to start a replica."))]
    configuration: Configuration,
    // Optional parameters
    #[builder(default, setter(transform = |handler: impl Fn(&InsertBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<InsertBlockEvent>),
    doc = "Registers a handler closure to be invoked after a block is inserted to the replica's BlockTree. This setter is optional."))]
    on_insert_block: Option<HandlerPtr<InsertBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&CommitBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<CommitBlockEvent>),
    doc = "Registers a handler closure to be invoked after a block is committed. This setter is optional."))]
    on_commit_block: Option<HandlerPtr<CommitBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&PruneBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<PruneBlockEvent>),
    doc = "Registers a handler closure to be invoked after a block is pruned, i.e., its siblings are removed from the replica's BlockTree. This setter is optional."))]
    on_prune_block: Option<HandlerPtr<PruneBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateHighestQCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateHighestQCEvent>),
    doc = "Registers a handler closure to be invoked after the replica updates its highest QC. This setter is optional."))]
    on_update_highest_qc: Option<HandlerPtr<UpdateHighestQCEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateLockedViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateLockedViewEvent>),
    doc = "Registers a handler closure to be invoked after the replica updates its locked view. This setter is optional."))]
    on_update_locked_view: Option<HandlerPtr<UpdateLockedViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateValidatorSetEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateValidatorSetEvent>),
    doc = "Registers a handler closure to be invoked after the replica updates its validator set. This setter is optional."))]
    on_update_validator_set: Option<HandlerPtr<UpdateValidatorSetEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ProposeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ProposeEvent>),
    doc = "Registers a handler closure to be invoked after the replica broadcasts a proposal for a block. This setter is optional."))]
    on_propose: Option<HandlerPtr<ProposeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&NudgeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<NudgeEvent>),
    doc = "Registers a handler closure to be invoked after the replica broadcasts a nudge for a block. This setter is optional."))]
    on_nudge: Option<HandlerPtr<NudgeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&VoteEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<VoteEvent>),
    doc = "Registers a handler closure to be invoked after the replica sends a vote. This setter is optional."))]
    on_vote: Option<HandlerPtr<VoteEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&NewViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<NewViewEvent>),
    doc = "Registers a handler closure to be invoked after the replica sends a new view message to the next leader. This setter is optional."))]
    on_new_view: Option<HandlerPtr<NewViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveProposalEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveProposalEvent>),
    doc = "Registers a handler closure to be invoked after the replica receives a proposal for a block. This setter is optional."))]
    on_receive_proposal: Option<HandlerPtr<ReceiveProposalEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveNudgeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveNudgeEvent>),
    doc = "Registers a handler closure to be invoked after the replica receives a nudge for a block. This setter is optional."))]
    on_receive_nudge: Option<HandlerPtr<ReceiveNudgeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveVoteEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveVoteEvent>),
    doc = "Registers a handler closure to be invoked after the replica receives a vote. This setter is optional."))]
    on_receive_vote: Option<HandlerPtr<ReceiveVoteEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveNewViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveNewViewEvent>),
    doc = "Registers a handler closure to be invoked after the replica receives a new view message. This setter is optional."))]
    on_receive_new_view: Option<HandlerPtr<ReceiveNewViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&StartViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<StartViewEvent>),
    doc = "Registers a handler closure to be invoked after the replica enters a new view. This setter is optional."))]
    on_start_view: Option<HandlerPtr<StartViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ViewTimeoutEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ViewTimeoutEvent>),
    doc = "Registers a handler closure to be invoked after the replica's view times out. This setter is optional."))]
    on_view_timeout: Option<HandlerPtr<ViewTimeoutEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&CollectQCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<CollectQCEvent>),
    doc = "Registers a handler closure to be invoked after the replica collects a new quorum certificate. This setter is optional."))]
    on_collect_qc: Option<HandlerPtr<CollectQCEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&StartSyncEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<StartSyncEvent>),
    doc = "Registers a handler closure to be invoked after the replica starts syncing, exiting the progress mode. This setter is optional."))]
    on_start_sync: Option<HandlerPtr<StartSyncEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&EndSyncEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<EndSyncEvent>),
    doc = "Registers a handler closure to be invoked after the replica finishes syncing, returning to progress mode. This setter is optional."))]
    on_end_sync: Option<HandlerPtr<EndSyncEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveSyncRequestEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveSyncRequestEvent>),
    doc = "Registers a handler closure to be invoked after the replica receives a sync request from a peer. This setter is optional."))]
    on_receive_sync_request: Option<HandlerPtr<ReceiveSyncRequestEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&SendSyncResponseEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<SendSyncResponseEvent>),
    doc = "Registers a handler closure to be invoked after the replica sends a sync response to a peer. This setter is optional."))]
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
