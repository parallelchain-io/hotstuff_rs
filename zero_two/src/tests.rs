/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! The test suite for HotStuff-rs involves an [NumberApp](app) that keeps track of a single number in its state, which
//! is initially 0. Tests push transactions to this app to increase this number, or change its validator set, and then
//! query its state to check if consensus is proceeding.
//! 
//! The replicas used in this test suite use a mock [NetworkStub], a mock [MemDB](key-value store), and the
//! [crate::pacemaker::DefaultPacemaker]. These use channels to simulate communication, and a hashmap to simulate
//! to simulate persistence, and thus never leaves any artifacts.

use std::collections::{HashMap, HashSet};
use std::sync::{
    Arc,
    Mutex,
    MutexGuard,
    mpsc::{Sender, Receiver, TryRecvError}
};
use borsh::{BorshSerialize, BorshDeserialize};
use sha2::{Sha256, Digest};
use ed25519_dalek::Keypair as DalekKeypair;
use crate::app::{App, ProduceBlockRequest, ValidateBlockRequest, ProduceBlockResponse, ValidateBlockResponse};
use crate::pacemaker::DefaultPacemaker;
use crate::replica::Replica;
use crate::state::{BlockTreeCamera, KVStore, KVGet, WriteBatch};
use crate::types::{PublicKeyBytes, ValidatorSetUpdates, AppID, AppStateUpdates, Power, CryptoHash};
use crate::messages::*;
use crate::networking::Network;

/// A mock network stub which passes messages from and to threads using channels.  
#[derive(Clone)]
struct NetworkStub {
    my_public_key: PublicKeyBytes,
    all_peers: HashMap<PublicKeyBytes, Sender<(PublicKeyBytes, Message)>>,
    inbox: Arc<Mutex<Receiver<(PublicKeyBytes, Message)>>>,
}

impl Network for NetworkStub {
    fn update_validator_set(&mut self, validator_set: crate::types::ValidatorSet) {
        ()    
    }

    fn send(&mut self, peer: PublicKeyBytes, message: Message) {
        if let Some(peer) = self.all_peers.get(&peer) {
            peer.send((self.my_public_key, message));
        }
    }

    fn recv(&mut self) -> Option<(PublicKeyBytes, Message)> {
        match self.inbox.lock().unwrap().try_recv() {
            Ok(o_m) => Some(o_m),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!(),
        }
    }
}

fn mock_network<const N: usize>(peers: [Keypair; N]) -> Vec<(Keypair, NetworkStub)> {
    todo!()
}

struct NumberApp {
    tx_queue: Vec<NumberAppTransaction>,
}
const NUMBER_KEY: [u8; 1] = [0];

#[derive(Clone, BorshSerialize, BorshDeserialize)]
enum NumberAppTransaction {
    Increment,
    SetValidator(PublicKeyBytes, Power),
}

impl App<MemDBSnapshot<'_>> for NumberApp {
    fn id(&self) -> AppID {
        0
    }

    fn produce_block(&mut self, request: ProduceBlockRequest<MemDBSnapshot>) -> ProduceBlockResponse {
        let initial_number = u32::from_le_bytes(request.app_state(&NUMBER_KEY).unwrap().to_owned().try_into().unwrap());

        let (app_state_updates, validator_set_updates) = self.execute(initial_number, &self.tx_queue);
        let data = vec![self.tx_queue.try_to_vec().unwrap()];
        let data_hash = {
            let hasher = Sha256::new();
            hasher.update(data[0]);
            hasher.finalize().into()
        };
        
        self.tx_queue.clear();
        
        ProduceBlockResponse {
            data_hash,
            data,
            app_state_updates,
            validator_set_updates,
        }
    }

    fn validate_block(&mut self, request: ValidateBlockRequest<MemDBSnapshot>) -> ValidateBlockResponse {
        let data = request.proposed_block().data;
        let data_hash: CryptoHash = {
            let hasher = Sha256::new();
            hasher.update(data[0]);
            hasher.finalize().into()
        };

        if !(request.proposed_block().data_hash == data_hash) {
            ValidateBlockResponse::Invalid
        } else {
            let initial_number = u32::from_le_bytes(request.app_state(&NUMBER_KEY).unwrap().to_owned().try_into().unwrap());

            if let Ok(transactions) = Vec::<NumberAppTransaction>::deserialize(&mut &*request.proposed_block().data[0]) {
                let (app_state_updates, validator_set_updates) = self.execute(initial_number, &transactions);
                ValidateBlockResponse::Valid { app_state_updates, validator_set_updates }
            } else {
                ValidateBlockResponse::Invalid
            }
        }
    }
}

impl NumberApp {
    fn execute(&mut self, initial_number: u32, transactions: &Vec<NumberAppTransaction>) -> (Option<AppStateUpdates>, Option<ValidatorSetUpdates>) {
        let number = initial_number;
        
        let mut validator_set_updates: Option<ValidatorSetUpdates> = None;
        for transaction in self.tx_queue {
            match transaction {
                NumberAppTransaction::Increment => {
                    number += 1;
                },
                NumberAppTransaction::SetValidator(validator, power) => {
                    if let Some(validator_set_updates) = validator_set_updates {
                        validator_set_updates.insert(validator, power);
                    } else {
                        let mut vsu = ValidatorSetUpdates::new();
                        vsu.insert(validator, power);
                        validator_set_updates = Some(vsu);
                    }
                }
            }
        }
        let mut app_state_updates: Option<AppStateUpdates> = None;
        app_state_updates.unwrap().insert(NUMBER_KEY.to_vec(), number.try_to_vec().unwrap());
        
        (app_state_updates, validator_set_updates)
    }
}

/// Things the Nodes will have in common:
/// - Initial Validator Set.
/// - Configuration.
/// 
/// Things that they will differ in:
/// - App instance.
/// - Network instance.
/// - KVStore.
/// - Keypair.

#[derive(Clone)]
struct MemDB(Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>);

impl MemDB {
    fn new() -> MemDB {
        MemDB(Arc::new(Mutex::new(HashMap::new())))
    }
}

impl<'a> KVStore<MemDBSnapshot<'a>> for MemDB {
    type WriteBatch = MemWriteBatch;

    fn write(&mut self, wb: Self::WriteBatch) {
        let map = self.0.lock().unwrap();
        for (key, value) in wb.insertions {
            map.insert(key, value); 
        } 
        for key in wb.deletions {
            map.remove(&key);
        }
    }
    
    fn clear(&mut self) {
        self.0.lock().unwrap().clear();
    }

    fn snapshot(&self) -> MemDBSnapshot<'a> {
        MemDBSnapshot(self.0.lock().unwrap())
    }
} 

impl KVGet for MemDB {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.lock().unwrap().get(key).cloned()
    }
}

struct MemWriteBatch {
    insertions: HashMap<Vec<u8>, Vec<u8>>,
    deletions: HashSet<Vec<u8>>,
}

impl WriteBatch for MemWriteBatch {
    fn new() -> Self {
        MemWriteBatch {
            insertions: HashMap::new(),
            deletions: HashSet::new(),
        }
    }

    fn set(&mut self, key: &[u8], value: &[u8]) {
        let _ = self.deletions.remove(key);
        self.insertions.insert(key.to_vec(), value.to_vec());
    }

    fn delete(&mut self, key: &[u8]) {
        let _ = self.insertions.remove(key);
        self.deletions.insert(key.to_vec());
    }
}

struct MemDBSnapshot<'a>(MutexGuard<'a, HashMap<Vec<u8>, Vec<u8>>>);

impl KVGet for MemDBSnapshot<'_> {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.get(key).cloned()
    }
}

struct Node<'a> {
    replica: Replica<MemDBSnapshot<'a>, MemDB>,
}

impl<'a> Node<'a> {
    fn submit_transaction(txn: NumberAppTransaction) {
        todo!()
    }
}

#[test]
fn integration_test() {
    let keypairs: [DalekKeypair; 6]; // generate 6.

    let init_as = {
        let state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), usize::to_le_bytes(0).to_vec());
        state
    };
    let init_vs = {
        let validator_set = ValidatorSetUpdates::new();
        validator_set.insert(keypairs[0].public.to_bytes(), 1);
        validator_set
    };

    let nodes: Vec<Node<> = mock_network(keypairs).into_iter().map(|(keypair, network_stub)| {
        let kv_store = MemDB::new();

        Replica::initialize(&mut kv_store, init_as.clone(), init_vs.clone());
        todo!();

        let (replica, bt_camera) = Replica::start(
            NumberApp,
            keypair,
            network_stub,
            MemDB::new(),
            DefaultPacemaker,
            configuration,
        );

        Node {
            replica,
            bt_camera
        }
    }).collect();

    // Push an add transaction to each of the 3 initial validators.

    // Poll the app state of every replica until the value is 3.

    // Push an add validators transaction to one of the validators.

    // Poll the validator set of every replica until we have 6 validators.

    // push an add transaction to each of the 6 validators we have now.

    // Poll the app state of every replica until the value is 9.
}