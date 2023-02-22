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
use std::thread;
use std::time::Duration;
use borsh::{BorshSerialize, BorshDeserialize};
use sha2::{Sha256, Digest};
use ed25519_dalek::Keypair as DalekKeypair;
use crate::app::{App, ProduceBlockRequest, ValidateBlockRequest, ProduceBlockResponse, ValidateBlockResponse};
use crate::pacemaker::DefaultPacemaker;
use crate::replica::Replica;
use crate::state::{KVStore, KVGet, WriteBatch};
use crate::types::{PublicKeyBytes, ValidatorSetUpdates, AppID, AppStateUpdates, Power, CryptoHash, ValidatorSet};
use crate::messages::*;
use crate::networking::Network;

#[test]
fn integration_test() {
    let keypairs: [DalekKeypair; 6]; // generate 6.
    let network_stubs = mock_network(keypairs);
    let init_as = {
        let mut state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), usize::to_le_bytes(0).to_vec());
        state
    };
    let init_vs = {
        let mut validator_set = ValidatorSetUpdates::new();
        validator_set.insert(keypairs[0].public.to_bytes(), 1);
        validator_set
    };

    let mut nodes: Vec<Node> = keypairs
        .into_iter()
        .zip(network_stubs)
        .map(|(keypair, network)| Node::new(keypair, network, init_as.clone(), init_vs.clone()))
        .collect();
    
    // Submit an Increment transaction to each of the 3 initial validators.
    nodes[0].submit_transaction(NumberAppTransaction::Increment);
    nodes[1].submit_transaction(NumberAppTransaction::Increment);
    nodes[2].submit_transaction(NumberAppTransaction::Increment);

    // Poll the app state of every replica until the value is 3.
    while nodes[0].number() != 3 || nodes[1].number() != 3 || nodes[2].number() != 3 {
        thread::sleep(Duration::from_millis(500));
    }

    // Submit an set validators transaction to each of the 3 initial validators to register the rest (3) of the peers.
    nodes[0].submit_transaction(NumberAppTransaction::SetValidator(nodes[3].public_key(), 1));
    nodes[1].submit_transaction(NumberAppTransaction::SetValidator(nodes[4].public_key(), 1));
    nodes[2].submit_transaction(NumberAppTransaction::SetValidator(nodes[5].public_key(), 1));

    // Poll the validator set of every replica until we have 6 validators.
    while nodes[0].committed_validator_set().len() != 6 || nodes[1].committed_validator_set().len() != 6 || nodes[2].committed_validator_set().len() != 6
        || nodes[3].committed_validator_set().len() != 6 || nodes[4].committed_validator_set().len() != 6 || nodes[5].committed_validator_set().len() != 6 {
        thread::sleep(Duration::from_millis(500));
    }

    // Push an Increment transaction to each of the 6 validators we have now.
    nodes[0].submit_transaction(NumberAppTransaction::Increment);
    nodes[1].submit_transaction(NumberAppTransaction::Increment);
    nodes[2].submit_transaction(NumberAppTransaction::Increment);
    nodes[3].submit_transaction(NumberAppTransaction::Increment);
    nodes[4].submit_transaction(NumberAppTransaction::Increment);
    nodes[5].submit_transaction(NumberAppTransaction::Increment);

    // Poll the app state of every replica until the value is 9.
    while nodes[0].number() != 9 || nodes[1].number() != 9 || nodes[2].number() != 9 || 
        nodes[3].number() != 9 || nodes[4].number() != 9 || nodes[5].number() != 9 {
        thread::sleep(Duration::from_millis(500));
    }

}


/// A mock network stub which passes messages from and to threads using channels.  
#[derive(Clone)]
struct NetworkStub {
    my_public_key: PublicKeyBytes,
    all_peers: HashMap<PublicKeyBytes, Sender<(PublicKeyBytes, Message)>>,
    inbox: Arc<Mutex<Receiver<(PublicKeyBytes, Message)>>>,
}

impl Network for NetworkStub {
    fn update_validator_set(&mut self, validator_set: ValidatorSetUpdates) {
        ()
    }

    fn send(&mut self, peer: PublicKeyBytes, message: Message) {
        if let Some(peer) = self.all_peers.get(&peer) {
            peer.send((self.my_public_key, message));
        }
    }

    fn broadcast(&mut self, message: Message) {
        for (_, peer) in self.all_peers {
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

fn mock_network<const N: usize>(peers: [DalekKeypair; N]) -> [NetworkStub; N] {
    todo!()
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

impl<'a> KVStore for MemDB {
    type WriteBatch = MemWriteBatch;
    type Snapshot<'a> = MemDBSnapshot<'a>;

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

    fn snapshot(&'a self) -> MemDBSnapshot<'a> {
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

struct NumberApp {
    tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>,
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

        let tx_queue = self.tx_queue.lock().unwrap();

        let (app_state_updates, validator_set_updates) = self.execute(initial_number, &tx_queue);
        let data = vec![tx_queue.try_to_vec().unwrap()];
        let data_hash = {
            let hasher = Sha256::new();
            hasher.update(data[0]);
            hasher.finalize().into()
        };
        
        tx_queue.clear();
        
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
    fn new(tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>) -> NumberApp {
        Self {
            tx_queue,
        }
    }

    fn execute(&mut self, initial_number: u32, transactions: &Vec<NumberAppTransaction>) -> (Option<AppStateUpdates>, Option<ValidatorSetUpdates>) {
        let number = initial_number;
        
        let mut validator_set_updates: Option<ValidatorSetUpdates> = None;
        for transaction in transactions {
            match transaction {
                NumberAppTransaction::Increment => {
                    number += 1;
                },
                NumberAppTransaction::SetValidator(validator, power) => {
                    if let Some(validator_set_updates) = validator_set_updates {
                        validator_set_updates.insert(*validator, *power);
                    } else {
                        let mut vsu = ValidatorSetUpdates::new();
                        vsu.insert(*validator, *power);
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
struct Node {
    public_key: PublicKeyBytes,
    tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>,
    replica: Replica<MemDB>,
}

impl Node {
    fn new(keypair: DalekKeypair, network: NetworkStub, init_as: AppStateUpdates, init_vs: ValidatorSetUpdates) -> Node {
        let kv_store = MemDB::new();

        Replica::initialize(kv_store.clone(), init_as, init_vs);

        let public_key = *keypair.public.as_bytes();
        let tx_queue = Arc::new(Mutex::new(Vec::new()));
        let replica = Replica::start(NumberApp::new(tx_queue.clone()), keypair, network, kv_store, DefaultPacemaker);

        Node {
            public_key,
            replica,
            tx_queue,
        }
    }

    fn submit_transaction(&mut self, txn: NumberAppTransaction) {
        todo!()
    }

    fn number(&self) -> u32 {
        todo!()
    }

    fn committed_validator_set(&self) -> ValidatorSet {
        todo!()
    }

    fn public_key(&self) -> PublicKeyBytes {
        todo!()
    }
}
