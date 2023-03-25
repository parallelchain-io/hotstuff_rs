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
//! Concretely, this requires that the library user's [networking provider's broadcast method](crate::networking) send progress
//! messages to all peers it is connected to, and not only the validators. The library user is free to design and implement
//! their own mechanism for deciding which peers, besides those in the validator set, should be connected to the network. 

use std::thread::JoinHandle;
use std::sync::mpsc::{self, Sender};
use ed25519_dalek::Keypair;
use crate::app::App;
use crate::networking::{start_polling, Network, ProgressMessageStub, SyncClientStub, SyncServerStub};
use crate::algorithm::start_algorithm;
use crate::sync_server::start_sync_server;
use crate::pacemaker::Pacemaker;
use crate::state::{BlockTree, BlockTreeCamera, KVStore};
use crate::types::{AppStateUpdates, ValidatorSetUpdates};
use crate::messages;

pub struct Replica<K: KVStore> {
    block_tree_camera: BlockTreeCamera<K>,
    poller: Option<JoinHandle<()>>,
    poller_shutdown: Sender<()>,
    algorithm: Option<JoinHandle<()>>,
    algorithm_shutdown: Sender<()>,
    sync_server: Option<JoinHandle<()>>,
    sync_server_shutdown: Sender<()>,
}

impl<K: KVStore> Replica<K> {
    pub fn initialize(
        kv_store: K,
        initial_app_state: AppStateUpdates, 
        initial_validator_set: ValidatorSetUpdates
    ) {
        let mut block_tree = BlockTree::new(kv_store);
        block_tree.initialize(&initial_app_state, &initial_validator_set);
    }

    pub fn start(
        app: impl App<K> + 'static,
        keypair: Keypair,
        mut network: impl Network + 'static,
        kv_store: K,
        pacemaker: impl Pacemaker + 'static,
    ) -> Replica<K> {
        let block_tree = BlockTree::new(kv_store.clone());

        network.init_validator_set(block_tree.committed_validator_set());
        let (poller_shutdown, poller_shutdown_receiver) = mpsc::channel();
        let (poller, progress_msgs, sync_requests, sync_responses) = start_polling(network.clone(), poller_shutdown_receiver);
        let pm_stub = ProgressMessageStub::new(network.clone(), progress_msgs);
        let sync_server_stub = SyncServerStub::new(sync_requests, network.clone());
        let sync_client_stub = SyncClientStub::new(network, sync_responses);

        let (sync_server_shutdown, sync_server_shutdown_receiver) = mpsc::channel();
        let sync_server = start_sync_server(BlockTreeCamera::new(kv_store.clone()), sync_server_stub, sync_server_shutdown_receiver, pacemaker.sync_request_limit());

        let (algorithm_shutdown, algorithm_shutdown_receiver) = mpsc::channel();
        let algorithm = start_algorithm(app, messages::Keypair::new(keypair), block_tree, pacemaker, pm_stub, sync_client_stub, algorithm_shutdown_receiver);

        let replica = Replica {
            block_tree_camera: BlockTreeCamera::new(kv_store.clone()),
            poller: Some(poller),
            poller_shutdown,
            algorithm: Some(algorithm),
            algorithm_shutdown,
            sync_server: Some(sync_server),
            sync_server_shutdown,
        };

        replica
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

        self.algorithm_shutdown.send(()).unwrap();
        self.algorithm.take().unwrap().join().unwrap();

        self.sync_server_shutdown.send(()).unwrap();
        self.sync_server.take().unwrap().join().unwrap();

        self.poller_shutdown.send(()).unwrap();
        self.poller.take().unwrap().join().unwrap();
    }
}

