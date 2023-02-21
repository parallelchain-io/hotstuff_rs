/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! HotStuff-rs works to safely replicate a state machine in multiple processes. In our terminology, these processes are
//! called 'replicas', and therefore the set of all replicas is the 'replica set'. Each replica is uniquely identified
//! by an Ed25519 public key.
//! 
//! Not every replica has to vote in consensus. Some operators may want to run replicas that merely keep up to consensus
//! decisions, without having weight in them. We call these replicas 'listeners', and call the replicas that vote in
//! consensus 'validators'. HotStuff-rs needs to know the full 'validator set' at all times to collect votes, but does not
//! need to know replicas. 

use std::marker::PhantomData;
use std::thread::JoinHandle;
use std::sync::mpsc::{self, Sender};
use ed25519_dalek::Keypair;
use crate::app::App;
use crate::networking::{start_polling, Network, ProgressMessageStub, SyncClientStub, SyncServerStub};
use crate::algorithm::start_algorithm;
use crate::sync_server::start_sync_server;
use crate::pacemaker::Pacemaker;
use crate::state::{BlockTree, BlockTreeCamera, KVStore, KVGet};
use crate::types::{AppStateUpdates, ValidatorSetUpdates};
use crate::messages;

pub struct Replica<S: KVGet, K: KVStore<S>> {
    block_tree_camera: BlockTreeCamera<S, K>,
    poller: JoinHandle<()>,
    poller_shutdown: Sender<()>,
    algorithm: JoinHandle<()>,
    algorithm_shutdown: Sender<()>,
    sync_server: JoinHandle<()>,
    sync_server_shutdown: Sender<()>,
    phantom_data: PhantomData<S>,
}

impl<S: KVGet, K: KVStore<S>> Replica<S, K> {
    pub fn initialize(
        kv_store: K,
        initial_app_state: AppStateUpdates, 
        initial_validator_set: ValidatorSetUpdates
    ) {
        let block_tree = BlockTree::new(kv_store);
        block_tree.initialize(&initial_app_state, &initial_validator_set);
    }

    pub fn start(
        app: impl App<S>,
        keypair: Keypair,
        network: impl Network,
        kv_store: K,
        pacemaker: impl Pacemaker,
    ) -> Replica<S, K> {
        let block_tree = BlockTree::new(kv_store.clone());

        let (poller_shutdown, poller_shutdown_receiver) = mpsc::channel();
        let (poller, progress_msgs, sync_requests, sync_responses) = start_polling(network, poller_shutdown_receiver);
        let pm_stub = ProgressMessageStub::new(network.clone(), progress_msgs);
        let sync_server_stub = SyncServerStub::new(sync_requests, network.clone());
        let sync_client_stub = SyncClientStub::new(network.clone(), sync_responses);

        let (sync_server_shutdown, sync_server_shutdown_receiver) = mpsc::channel();
        let sync_server = start_sync_server(block_tree, sync_server_stub, sync_server_shutdown_receiver);

        let (algorithm_shutdown, algorithm_shutdown_receiver) = mpsc::channel();
        let algorithm = start_algorithm(app, messages::Keypair::new(keypair), block_tree, pacemaker, pm_stub, sync_client_stub, algorithm_shutdown_receiver);

        let block_tree_camera = BlockTreeCamera::new(kv_store);
        let replica = Replica {
            block_tree_camera,
            poller,
            poller_shutdown,
            algorithm,
            algorithm_shutdown,
            sync_server,
            sync_server_shutdown,
            phantom_data: PhantomData,
        };

        replica
    }

    pub fn block_tree_camera(&self) -> BlockTreeCamera<S, K> {
        self.block_tree_camera.clone()
    }
}

impl<S: KVGet, K: KVStore<S>> Drop for Replica<S, K> {
    fn drop(&mut self) {
        self.algorithm_shutdown.send(());
        self.algorithm.join();

        self.sync_server_shutdown.send(());
        self.sync_server.join();

        self.poller_shutdown.send(());
        self.poller.join();
    }
}

