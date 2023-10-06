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
use crate::types::{AppStateUpdates, ValidatorSetUpdates, PrivateKey};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

pub struct Replica<K: KVStore> {
    block_tree_camera: BlockTreeCamera<K>,
    poller: Option<JoinHandle<()>>,
    poller_shutdown: Sender<()>,
    algorithm: Option<JoinHandle<()>>,
    algorithm_shutdown: Sender<()>,
    sync_server: Option<JoinHandle<()>>,
    sync_server_shutdown: Sender<()>,
    event_bus: Option<JoinHandle<()>>,
    event_bus_shutdown: Sender<()>,
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

    pub fn start( //these ugly parameters are temporary, we will get rid of them later by defining a builder
        app: impl App<K> + 'static,
        private_key: PrivateKey,
        mut network: impl Network + 'static,
        kv_store: K,
        pacemaker: impl Pacemaker + 'static,
        insert_block_handlers: Vec<HandlerPtr<InsertBlockEvent>>,
        commit_block_handlers: Vec<HandlerPtr<CommitBlockEvent>>,
        prune_block_handlers: Vec<HandlerPtr<PruneBlockEvent>>,
        propose_handlers: Vec<HandlerPtr<ProposeEvent>>,
        receive_proposal_handlers: Vec<HandlerPtr<ReceiveProposalEvent>>,
        nudge_handlers: Vec<HandlerPtr<NudgeEvent>>,
        receive_nudge_handlers: Vec<HandlerPtr<ReceiveNudgeEvent>>,
        vote_handlers: Vec<HandlerPtr<VoteEvent>>,
        receive_vote_handlers: Vec<HandlerPtr<ReceiveVoteEvent>>,
        new_view_handlers: Vec<HandlerPtr<NewViewEvent>>,
        receive_new_view_handlers: Vec<HandlerPtr<ReceiveNewViewEvent>>,
        start_view_handlers: Vec<HandlerPtr<StartViewEvent>>,
        view_timeout_handlers: Vec<HandlerPtr<ViewTimeoutEvent>>,
        start_sync_handlers: Vec<HandlerPtr<StartSyncEvent>>,
        end_sync_handlers: Vec<HandlerPtr<EndSyncEvent>>,
    ) -> Replica<K> {
        let block_tree = BlockTree::new(kv_store.clone());

        network.init_validator_set(block_tree.committed_validator_set());
        let (poller_shutdown, poller_shutdown_receiver) = mpsc::channel();
        let (poller, progress_msgs, sync_requests, sync_responses) =
            start_polling(network.clone(), poller_shutdown_receiver);
        let pm_stub = ProgressMessageStub::new(network.clone(), progress_msgs);
        let sync_server_stub = SyncServerStub::new(sync_requests, network.clone());
        let sync_client_stub = SyncClientStub::new(network, sync_responses);

        let (sync_server_shutdown, sync_server_shutdown_receiver) = mpsc::channel();
        let sync_server = start_sync_server(
            BlockTreeCamera::new(kv_store.clone()),
            sync_server_stub,
            sync_server_shutdown_receiver,
            pacemaker.sync_request_limit(),
        );

        let (algorithm_shutdown, algorithm_shutdown_receiver) = mpsc::channel();
        let (event_publisher, event_subscriber) = mpsc::channel();
        let algorithm = start_algorithm(
            app,
            messages::Keypair::new(private_key),
            block_tree,
            pacemaker,
            pm_stub,
            sync_client_stub,
            algorithm_shutdown_receiver,
            event_publisher,
        );

        let (event_bus_shutdown, event_bus_shutdown_receiver) = mpsc::channel();

        let event_handlers = EventHandlers {
            insert_block_handlers,
            commit_block_handlers,
            prune_block_handlers,
            propose_handlers,
            receive_proposal_handlers,
            nudge_handlers,
            receive_nudge_handlers,
            vote_handlers,
            receive_vote_handlers,
            new_view_handlers,
            receive_new_view_handlers,
            start_view_handlers,
            view_timeout_handlers,
            start_sync_handlers,
            end_sync_handlers
        };

        let event_bus = start_event_bus(
            event_handlers,
            event_subscriber,
            event_bus_shutdown_receiver,
        );
        
        Replica {
            block_tree_camera: BlockTreeCamera::new(kv_store),
            poller: Some(poller),
            poller_shutdown,
            algorithm: Some(algorithm),
            algorithm_shutdown,
            sync_server: Some(sync_server),
            sync_server_shutdown,
            event_bus: Some(event_bus),
            event_bus_shutdown,
        }
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

        self.event_bus_shutdown.send(()).unwrap();
        self.event_bus.take().unwrap().join().unwrap();

        self.algorithm_shutdown.send(()).unwrap();
        self.algorithm.take().unwrap().join().unwrap();

        self.sync_server_shutdown.send(()).unwrap();
        self.sync_server.take().unwrap().join().unwrap();

        self.poller_shutdown.send(()).unwrap();
        self.poller.take().unwrap().join().unwrap();
    }
}
