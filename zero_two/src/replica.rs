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

use ed25519_dalek::Keypair;
use crate::app::App;
use crate::config::Configuration;
use crate::networking::Network;
use crate::pacemaker::Pacemaker;
use crate::state::{BlockTreeCamera, KVStore};
use crate::types::{AppStateUpdates, ValidatorSetUpdates};
pub struct Replica;

impl Replica {
    pub fn initialize<'a, K: KVStore<'a>>(
        kv_store: &mut K,
        initial_app_state: AppStateUpdates, 
        initial_validator_set: ValidatorSetUpdates
    ) {
        todo!()
    }

    pub fn start<'a, K: KVStore<'a>>(
        app: impl App<K::Snapshot>,
        keypair: Keypair,
        network_stub: impl Network,
        kv_store: K,
        pacemaker: impl Pacemaker,
        configuration: Configuration,
    ) -> (Replica, BlockTreeCamera<'a, K>) {
        todo!()
    }
}

