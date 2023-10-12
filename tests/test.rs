/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0

    Authors: Alice Lim
*/

//! The test suite for HotStuff-rs involves an [app](NumberApp) that keeps track of a single number in its state, which
//! is initially 0. Tests push transactions to this app to increase this number, or change its validator set, and then
//! query its state to check if consensus is proceeding.
//!
//! The replicas used in this test suite use a mock [NetworkStub], a mock [MemDB](key-value store), and the
//! [default pacemaker](hotstuff_rs::pacemaker::DefaultPacemaker). These use channels to simulate communication, and a
//! hashmap to simulate persistence, and thus never leaves any artifacts.
//! 
//! There are currently two tests:
//! 1. [basic_functions_integration_test]: tests the most basic user-visible functionalities: committing transactions,
//!    querying app state, and expanding the validator set. This should complete in less than 1 minute.
//! 2. [default_pacemaker_view_sync_integration_test]: tests whether a validator set using [DefaultPacemaker] that
//!    start with unsynchronized views. This should complete in less than 3 minutes.

extern crate rand;
use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::SigningKey;
use hotstuff_rs::app::{
    App, ProduceBlockRequest, ProduceBlockResponse, ValidateBlockRequest, ValidateBlockResponse,
};
use hotstuff_rs::events::InsertBlockEvent;
use hotstuff_rs::messages::*;
use hotstuff_rs::networking::Network;
use hotstuff_rs::pacemaker::DefaultPacemaker;
use hotstuff_rs::replica::{Replica, ReplicaBuilder, Configuration};
use hotstuff_rs::state::{KVGet, KVStore, WriteBatch};
use hotstuff_rs::types::{
    AppStateUpdates, ChainID, CryptoHash, Power, PublicKey, ValidatorSet, ValidatorSetUpdates,
    ViewNumber,
};
use log::LevelFilter;
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Once;
use std::sync::{
    mpsc::{self, Receiver, Sender, TryRecvError},
    Arc, Mutex, MutexGuard,
};
use std::thread;
use std::time::Duration;

type PublicKeyBytes = [u8; 32];

#[test]
fn basic_functions_integration_test() {
    setup_logger(LevelFilter::Trace);

    let mut csprg = OsRng {};
    let keypairs: Vec<SigningKey> = (0..3).map(|_| SigningKey::generate(&mut csprg)).collect();
    let network_stubs = mock_network(keypairs.iter().map(|kp| kp.verifying_key()));
    let init_as = {
        let mut state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), u32::to_le_bytes(0).to_vec());
        state
    };
    let init_vs = {
        let mut validator_set = ValidatorSetUpdates::new();
        validator_set.insert(keypairs[0].verifying_key(), 1);
        validator_set
    };

    let mut nodes: Vec<Node> = keypairs
        .into_iter()
        .zip(network_stubs)
        .map(|(keypair, network)| Node::new(keypair, network, init_as.clone(), init_vs.clone()))
        .collect();

    // Submit an Increment transaction to the initial validator.
    log::debug!("Submitting an Increment transaction to the initial validator.");
    nodes[0].submit_transaction(NumberAppTransaction::Increment);

    // Poll the app state of every replica until the value is 1.
    log::debug!("Polling the app state of every replica until the value is 1.");
    while nodes[0].number() != 1 || nodes[1].number() != 1 || nodes[2].number() != 1 {
        thread::sleep(Duration::from_millis(500));
    }

    // Submit 2 set validator transactions to the initial validator to register the rest (2) of the peers.
    log::debug!("Submitting 2 set validator transactions to the initial validator to register the rest (2) of the peers.");
    let node_1 = nodes[1].public_key();
    nodes[0].submit_transaction(NumberAppTransaction::SetValidator(node_1, 1));
    let node_2 = nodes[2].public_key();
    nodes[0].submit_transaction(NumberAppTransaction::SetValidator(node_2, 1));

    // Poll the validator set of every replica until we have 3 validators.
    log::debug!("Polling the validator set of every replica until we have 3 validators.");
    while nodes[0].committed_validator_set().len() != 3
        || nodes[1].committed_validator_set().len() != 3
        || nodes[2].committed_validator_set().len() != 3
    {
        thread::sleep(Duration::from_millis(500));
    }

    // Push an Increment transaction to each of the 3 validators we have now.
    log::debug!("Submitting an increment transaction to each of the 3 validators we have now.");
    nodes[0].submit_transaction(NumberAppTransaction::Increment);
    nodes[1].submit_transaction(NumberAppTransaction::Increment);
    nodes[2].submit_transaction(NumberAppTransaction::Increment);

    // Poll the app state of every replica until the value is 4.
    log::debug!("Polling the app state of every replica until the value is 4");
    while nodes[0].number() != 4 || nodes[1].number() != 4 || nodes[2].number() != 4 {
        thread::sleep(Duration::from_millis(500));
    }
}

#[test]
fn default_pacemaker_view_sync_integration_test() {
    setup_logger(LevelFilter::Trace);

    let mut csprg = OsRng {};
    let keypair_1 = SigningKey::generate(&mut csprg);
    let keypair_2 = SigningKey::generate(&mut csprg);
    let init_as = {
        let mut state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), u32::to_le_bytes(0).to_vec());
        state
    };
    let init_vs = {
        let mut validator_set = ValidatorSetUpdates::new();
        validator_set.insert(keypair_1.verifying_key(), 1);
        validator_set.insert(keypair_2.verifying_key(), 1);
        validator_set
    };
    let (network_stub_1, network_stub_2) = {
        let mut network_stubs =
            mock_network([keypair_1.verifying_key(), keypair_2.verifying_key()].into_iter());
        (network_stubs.remove(0), network_stubs.remove(0))
    };

    // Start the first node.
    let mut first_node = Node::new(keypair_1, network_stub_1, init_as.clone(), init_vs.clone());

    // Wait until the first node's current view advances to 5.
    while first_node.highest_view_entered() < 5 {
        thread::sleep(Duration::from_millis(500));
    }

    // Start the second node.
    log::debug!(
        "First node is at view {}, starting second node.",
        first_node.highest_view_entered()
    );
    let second_node = Node::new(keypair_2, network_stub_2, init_as.clone(), init_vs.clone());

    // Wait until both nodes' current views match.
    log::debug!("Waiting until both nodes' current views match.");
    while first_node.highest_view_entered() != second_node.highest_view_entered() {
        thread::sleep(Duration::from_millis(500));
    }

    // Submit an Increment transaction.
    log::debug!("Submitting an increment transaction.");
    first_node.submit_transaction(NumberAppTransaction::Increment);

    // Wait until the app state of both nodes has value == 1.
    log::debug!("Waiting until both nodes' values are 1.");
    while first_node.number() != 1 || second_node.number() != 1 {
        thread::sleep(Duration::from_millis(500));
    }
}

static LOGGER_INIT: Once = Once::new();

// Set up a logger that logs all log messages with level Trace and above.
fn setup_logger(level: LevelFilter) {
    LOGGER_INIT.call_once(|| {
        fern::Dispatch::new()
            .format(|out, message, record| {
                out.finish(format_args!(
                    "[{:?}][{}] {}",
                    thread::current().id(),
                    record.level(),
                    message
                ))
            })
            .level(level)
            .chain(io::stdout())
            .apply()
            .unwrap();
    })
}

/// A mock network stub which passes messages from and to threads using channels.  
#[derive(Clone)]
struct NetworkStub {
    my_public_key: PublicKey,
    all_peers: HashMap<PublicKey, Sender<(PublicKey, Message)>>,
    inbox: Arc<Mutex<Receiver<(PublicKey, Message)>>>,
}

impl Network for NetworkStub {
    fn init_validator_set(&mut self, _: ValidatorSet) {}

    fn update_validator_set(&mut self, _: ValidatorSetUpdates) {}

    fn send(&mut self, peer: PublicKey, message: Message) {
        if let Some(peer) = self.all_peers.get(&peer) {
            let _ = peer.send((self.my_public_key, message));
        }
    }

    fn broadcast(&mut self, message: Message) {
        for (_, peer) in &self.all_peers {
            let _ = peer.send((self.my_public_key, message.clone()));
        }
    }

    fn recv(&mut self) -> Option<(PublicKey, Message)> {
        match self.inbox.lock().unwrap().try_recv() {
            Ok(o_m) => Some(o_m),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!(),
        }
    }
}

fn mock_network(peers: impl Iterator<Item = PublicKey>) -> Vec<NetworkStub> {
    let mut all_peers = HashMap::new();
    let peer_and_inboxes: Vec<(PublicKey, Receiver<(PublicKey, Message)>)> = peers
        .map(|peer| {
            let (sender, receiver) = mpsc::channel();
            all_peers.insert(peer, sender);

            (peer, receiver)
        })
        .collect();

    peer_and_inboxes
        .into_iter()
        .map(|(my_public_key, inbox)| NetworkStub {
            my_public_key,
            all_peers: all_peers.clone(),
            inbox: Arc::new(Mutex::new(inbox)),
        })
        .collect()
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

impl KVStore for MemDB {
    type WriteBatch = MemWriteBatch;
    type Snapshot<'a> = MemDBSnapshot<'a>;

    fn write(&mut self, wb: Self::WriteBatch) {
        let mut map = self.0.lock().unwrap();
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

    fn snapshot<'b>(&'b self) -> MemDBSnapshot<'b> {
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

impl App<MemDB> for NumberApp {
    fn chain_id(&self) -> ChainID {
        0
    }

    fn produce_block(&mut self, request: ProduceBlockRequest<MemDB>) -> ProduceBlockResponse {
        thread::sleep(Duration::from_millis(250));
        let initial_number = u32::from_le_bytes(
            request
                .block_tree()
                .app_state(&NUMBER_KEY)
                .unwrap()
                .try_into()
                .unwrap(),
        );

        let mut tx_queue = self.tx_queue.lock().unwrap();

        let (app_state_updates, validator_set_updates) = self.execute(initial_number, &tx_queue);
        let data = vec![tx_queue.try_to_vec().unwrap()];
        let data_hash = {
            let mut hasher = Sha256::new();
            hasher.update(&data[0]);
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

    fn validate_block(&mut self, request: ValidateBlockRequest<MemDB>) -> ValidateBlockResponse {
        thread::sleep(Duration::from_millis(250));
        let data = &request.proposed_block().data;
        let data_hash: CryptoHash = {
            let mut hasher = Sha256::new();
            hasher.update(&data[0]);
            hasher.finalize().into()
        };

        if request.proposed_block().data_hash != data_hash {
            ValidateBlockResponse::Invalid
        } else {
            let initial_number = u32::from_le_bytes(
                request
                    .block_tree()
                    .app_state(&NUMBER_KEY)
                    .unwrap()
                    .try_into()
                    .unwrap(),
            );

            if let Ok(transactions) =
                Vec::<NumberAppTransaction>::deserialize(&mut &*request.proposed_block().data[0])
            {
                let (app_state_updates, validator_set_updates) =
                    self.execute(initial_number, &transactions);
                ValidateBlockResponse::Valid {
                    app_state_updates,
                    validator_set_updates,
                }
            } else {
                ValidateBlockResponse::Invalid
            }
        }
    }
}

impl NumberApp {
    fn new(tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>) -> NumberApp {
        Self { tx_queue }
    }

    fn execute(
        &self,
        initial_number: u32,
        transactions: &Vec<NumberAppTransaction>,
    ) -> (Option<AppStateUpdates>, Option<ValidatorSetUpdates>) {
        let mut number = initial_number;

        let mut validator_set_updates: Option<ValidatorSetUpdates> = None;
        for transaction in transactions {
            match transaction {
                NumberAppTransaction::Increment => {
                    number += 1;
                }
                NumberAppTransaction::SetValidator(validator, power) => {
                    if let Some(updates) = &mut validator_set_updates {
                        updates.insert(PublicKey::from_bytes(validator).unwrap(), *power);
                    } else {
                        let mut vsu = ValidatorSetUpdates::new();
                        vsu.insert(PublicKey::from_bytes(validator).unwrap(), *power);
                        validator_set_updates = Some(vsu);
                    }
                }
            }
        }
        let app_state_updates = if number != initial_number {
            let mut updates = AppStateUpdates::new();
            updates.insert(NUMBER_KEY.to_vec(), number.try_to_vec().unwrap());
            Some(updates)
        } else {
            None
        };

        (app_state_updates, validator_set_updates)
    }
}
struct Node {
    public_key: PublicKeyBytes,
    tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>,
    replica: Replica<MemDB>,
}

impl Node {
    fn new(
        keypair: SigningKey,
        network: NetworkStub,
        init_as: AppStateUpdates,
        init_vs: ValidatorSetUpdates,
    ) -> Node {
        let kv_store = MemDB::new();

        Replica::initialize(kv_store.clone(), init_as, init_vs);

        let public_key = keypair.verifying_key().to_bytes();
        let tx_queue = Arc::new(Mutex::new(Vec::new()));
        let pacemaker =
            DefaultPacemaker::new(Duration::from_millis(500), 10, Duration::from_secs(3));

        let configuration = Configuration {
            me: keypair,
            sync_request_limit: 500,
            sync_trigger_timeout: Duration::new(10, 0),
            sync_response_timeout: Duration::new(10, 0),
            progress_msg_buffer_capacity: 1000,
            log_events: true,
        };

        let insert_block_handler = |insert_block_event: &InsertBlockEvent| {
            println!("Inserted block with hash: {:?}, timestamp: {:?}", insert_block_event.block.hash, insert_block_event.timestamp)
        };

        let replica = 
            ReplicaBuilder :: builder()
            .app(NumberApp::new(tx_queue.clone()))
            .pacemaker(pacemaker)
            .network(network)
            .kv_store(kv_store)
            .configuration(configuration)
            .on_insert_block(Box::new(insert_block_handler))
            .build()
            .start();

        Node {
            public_key,
            replica,
            tx_queue,
        }
    }

    fn submit_transaction(&mut self, txn: NumberAppTransaction) {
        self.tx_queue.lock().unwrap().push(txn);
    }

    fn number(&self) -> u32 {
        u32::deserialize(
            &mut &*self
                .replica
                .block_tree_camera()
                .snapshot()
                .committed_app_state(&NUMBER_KEY)
                .unwrap(),
        )
        .unwrap()
    }

    fn committed_validator_set(&self) -> ValidatorSet {
        self.replica
            .block_tree_camera()
            .snapshot()
            .committed_validator_set()
    }

    fn highest_view_entered(&self) -> ViewNumber {
        self.replica
            .block_tree_camera()
            .snapshot()
            .highest_view_entered()
    }

    fn public_key(&self) -> PublicKeyBytes {
        self.public_key
    }
}
