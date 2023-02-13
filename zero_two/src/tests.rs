//! The test suite for HotStuff-rs involves an [NumberApp](app) that keeps track of a single number in its state, which
//! is initially 0. Users can push transactions to this app to increase this number, or change its validator
//! set.
//! 
//! The replicas used in this test suite use a mock [NetworkStub], a mock [MemDB](key-value store), and
//! the [crate::pacemaker::DefaultPacemaker]. These use channels to simulate communication, and a hashmap to 
//! simulate persistence, and so do not leave any artifacts behind.

use std::collections::HashMap;
use std::sync::{
    Arc,
    Mutex,
    MutexGuard,
    mpsc::{Sender, Receiver, TryRecvError}
};
use crate::app::{App, ProposeBlockRequest, ValidateBlockRequest, ProposeBlockResponse, ValidateBlockResponse};
use crate::config::Configuration;
use crate::pacemaker::DefaultPacemaker;
use crate::replica::Replica;
use crate::state::{BlockTreeCamera, KVStore, KVGet, WriteBatch};
use crate::types::{PublicKeyBytes, Keypair, ValidatorSetUpdates, AppID, AppStateUpdates, Power};
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
    transactions: Vec<NumberAppTransaction>,
}
const NUMBER_KEY: [u8; 1] = [0];

enum NumberAppTransaction {
    Increment,
    SetValidator(PublicKeyBytes, Power),
}

impl App<MemDBSnapshot<'_>> for NumberApp {
    fn id(&self) -> AppID {
        1
    }

    fn propose_block(&mut self, request: ProposeBlockRequest<MemDBSnapshot>) -> ProposeBlockResponse {
        let number = request.app_state().get(&NUMBER_KEY);
        todo!()
    }

    fn validate_block(&mut self, request: ValidateBlockRequest<MemDBSnapshot>) -> ValidateBlockResponse {
        todo!() 
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

impl<'a> KVStore<'a> for MemDB {
    type WriteBatch = MemWriteBatch;
    type Snapshot = MemDBSnapshot<'a>;

    fn write(&mut self, wb: Self::WriteBatch) {
        let map = self.0.lock().unwrap();
        for (key, value) in wb.0 {
            map.insert(key, value); 
        } 
    }
    
    fn clear(&mut self) {
        self.0.lock().unwrap().clear();
    }

    fn snapshot(&'a self) -> Self::Snapshot {
        MemDBSnapshot(self.0.lock().unwrap())
    }
} 

impl<'a> KVGet for MemDB {
    fn get(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.lock().unwrap().get(key).cloned()
    }
}

struct MemWriteBatch(Vec<(Vec<u8>, Vec<u8>)>);

impl WriteBatch for MemWriteBatch {
    fn new() -> Self {
        MemWriteBatch(Vec::new())
    }

    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.0.push((key, value));
    }
}

struct MemDBSnapshot<'a>(MutexGuard<'a, HashMap<Vec<u8>, Vec<u8>>>);

impl KVGet for MemDBSnapshot<'_> {
    fn get(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.get(key).cloned()
    }
}

struct Node<'a> {
    replica: Replica,
    bt_camera: BlockTreeCamera<'a, MemDB>,
}

#[test]
fn integration_test() {
    let keypairs: [Keypair; 6] = todo!(); // generate 6.

    let init_as = {
        let state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), usize::to_le_bytes(0).to_vec());
        state
    };
    // let init_vs: ValidatorSetUpdates = keypairs[0..3].iter().map(|k| (*k.public.as_bytes(), 1)).collect();
    // let configuration: Configuration = todo!();

    // let nodes: Vec<Node> = mock_network(keypairs).into_iter().map(|(keypair, network_stub)| {
    //     let kv_store = MemDB::new();

    //     Replica::initialize(&mut kv_store, init_as.clone(), init_vs.clone());
    //     todo!();

    //     let (replica, bt_camera) = Replica::start(
    //         NumberApp,
    //         keypair,
    //         network_stub,
    //         MemDB::new(),
    //         DefaultPacemaker,
    //         configuration,
    //     );

    //     Node {
    //         replica,
    //         bt_camera
    //     }
    // }).collect();

    // Push an add transaction to each of the 3 initial validators.

    // Poll the app state of every replica until the value is 3.

    // Push an add validators transaction to one of the validators.

    // Poll the validator set of every replica until we have 6 validators.

    // push an add transaction to each of the 6 validators we have now.

    // Poll the app state of every replica until the value is 9.
}