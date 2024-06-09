/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! The test suite for HotStuff-rs involves an [app](NumberApp) that keeps track of a single number in
//! its state, which is initially 0. Tests push transactions to this app to increase this number, or
//! change its validator set, and then query its state to check if consensus is proceeding.
//!
//! The replicas used in this test suite use a mock [NetworkStub], a mock [MemDB](key-value store).
//! These use channels to simulate communication, and a hashmap to simulate persistence, and thus
//! never leaves any artifacts.
//!
//! There are currently three tests:
//! 1. [basic_consenus_and_validator_set_update_test]: tests the most basic user-visible functionalities:
//!    committing transactions, querying app state, and expanding the validator set. This should complete
//!    in less than 40 seconds.
//! 2. [multiple_validator_set_updates_test]: tests if frequent validator set updates, including ones that
//!    completely change the validator set (i.e., there is no intersection between the old and the new
//!    validator sets), can succeed. This should complete in less than 1 minute.
//! 3. [pacemaker_initial_view_sync_test]: tests whether validators can succesfully synchronize in the
//!    initial view even if they start executing the protocol at different moments. This should complete
//!    in less than 30 seconds.

extern crate rand;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{SigningKey, VerifyingKey};
use hotstuff_rs::app::{
    App, ProduceBlockRequest, ProduceBlockResponse, ValidateBlockRequest, ValidateBlockResponse,
};
use hotstuff_rs::events::{
    CommitBlockEvent, InsertBlockEvent, ReceiveProposalEvent, UpdateHighestQCEvent, VoteEvent,
};
use hotstuff_rs::messages::*;
use hotstuff_rs::networking::Network;
use hotstuff_rs::replica::{Configuration, Replica, ReplicaSpec};
use hotstuff_rs::state::kv_store::{KVGet, KVStore};
use hotstuff_rs::state::write_batch::WriteBatch;
use hotstuff_rs::types::basic::{
    AppStateUpdates, BufferSize, ChainID, CryptoHash, Data, Datum, EpochLength, Power, ViewNumber,
};
use hotstuff_rs::types::validators::{ValidatorSet, ValidatorSetState, ValidatorSetUpdates};
use log::LevelFilter;
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use std::collections::btree_map::Keys;
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Once;
use std::sync::{
    mpsc::{self, Receiver, Sender, TryRecvError},
    Arc, Mutex, MutexGuard,
};
use std::thread;
use std::time::{Duration, SystemTime};

type VerifyingKeyBytes = [u8; 32];

#[test]
fn basic_consenus_and_validator_set_update_test() {
    setup_logger(LevelFilter::Trace);

    let mut csprg = OsRng {};
    let keypairs: Vec<SigningKey> = (0..3).map(|_| SigningKey::generate(&mut csprg)).collect();
    let network_stubs = mock_network(keypairs.iter().map(|kp| kp.verifying_key()));
    let init_as = {
        let mut state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), u32::to_le_bytes(0).to_vec());
        state
    };
    let init_vs_updates = {
        let mut vs_updates = ValidatorSetUpdates::new();
        vs_updates.insert(keypairs[0].verifying_key(), Power::new(1));
        vs_updates
    };

    let mut nodes: Vec<Node> = keypairs
        .into_iter()
        .zip(network_stubs)
        .map(|(keypair, network)| {
            Node::new(keypair, network, init_as.clone(), init_vs_updates.clone())
        })
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
    let node_1 = nodes[1].verifying_key();
    nodes[0].submit_transaction(NumberAppTransaction::SetValidator(node_1, Power::new(1)));
    let node_2 = nodes[2].verifying_key();
    nodes[0].submit_transaction(NumberAppTransaction::SetValidator(node_2, Power::new(1)));

    // Poll the validator set of every replica until we have 3 validators.
    log::debug!("Polling the validator set of every replica until we have 3 validators.");
    while nodes[0].committed_validator_set().len() != 3
        || nodes[1].committed_validator_set().len() != 3
        || nodes[2].committed_validator_set().len() != 3
    {
        thread::sleep(Duration::from_millis(500));
    }

    thread::sleep(Duration::from_millis(1000));

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
fn multiple_validator_set_updates_test() {
    setup_logger(LevelFilter::Trace);

    let mut csprg = OsRng {};
    let keypairs: Vec<SigningKey> = (0..6).map(|_| SigningKey::generate(&mut csprg)).collect();
    let network_stubs = mock_network(keypairs.iter().map(|kp| kp.verifying_key()));
    let init_as = {
        let mut state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), u32::to_le_bytes(0).to_vec());
        state
    };
    let init_vs_updates = {
        let mut vs_updates = ValidatorSetUpdates::new();
        vs_updates.insert(keypairs[0].verifying_key(), Power::new(1));
        vs_updates.insert(keypairs[1].verifying_key(), Power::new(1));
        vs_updates
    };

    let mut nodes: Vec<Node> = keypairs
        .into_iter()
        .zip(network_stubs)
        .map(|(keypair, network)| {
            Node::new(keypair, network, init_as.clone(), init_vs_updates.clone())
        })
        .collect();

    thread::sleep(Duration::from_millis(500));

    // Submit a set validator transaction to one of the 2 initial validators to register 1 more peer.
    log::debug!(
        "Submitting a set validator transaction to the initial validator to register 1 more peer."
    );
    let node_2 = nodes[2].verifying_key();
    nodes[0].submit_transaction(NumberAppTransaction::SetValidator(node_2, Power::new(1)));

    // Poll the validator set of every replica until we have 3 validators.
    log::debug!("Polling the validator set of every replica until we have 3 validators.");
    while nodes[0].committed_validator_set().len() != 3
        || nodes[1].committed_validator_set().len() != 3
        || nodes[2].committed_validator_set().len() != 3
    {
        thread::sleep(Duration::from_millis(500));
    }

    thread::sleep(Duration::from_millis(1000));

    // Submit a set validator transaction to one of the 2 initial validators to register 1 more peer.
    log::debug!("Submitting a set validator transaction to one of the initial validators to register 1 more peer.");
    let node_3 = nodes[3].verifying_key();
    nodes[1].submit_transaction(NumberAppTransaction::SetValidator(node_3, Power::new(3)));

    // Poll the validator set of every replica until we have 4 validators.
    log::debug!("Polling the validator set of every replica until we have 4 validators.");
    while nodes[0].committed_validator_set().len() != 4
        || nodes[1].committed_validator_set().len() != 4
        || nodes[2].committed_validator_set().len() != 4
    {
        thread::sleep(Duration::from_millis(500));
    }

    // Submit set validator transactions to one of the existing validators to:
    // 1. Remove all existing validators
    // 2. Set new validators: nodes[4] and nodes[5]
    log::debug!("Submitting a set validator transaction to one of the initial validators to delete the current validators and add two new validators.");
    let node_4 = nodes[4].verifying_key();
    let node_5 = nodes[5].verifying_key();
    let node_0 = nodes[0].verifying_key();
    let node_1 = nodes[1].verifying_key();
    nodes[1].submit_transaction(NumberAppTransaction::SetValidator(node_4, Power::new(2)));
    nodes[1].submit_transaction(NumberAppTransaction::SetValidator(node_5, Power::new(4)));
    nodes[1].submit_transaction(NumberAppTransaction::DeleteValidator(node_0));
    nodes[1].submit_transaction(NumberAppTransaction::DeleteValidator(node_1));
    nodes[1].submit_transaction(NumberAppTransaction::DeleteValidator(node_2));
    nodes[1].submit_transaction(NumberAppTransaction::DeleteValidator(node_3));

    // Poll the validator set of every replica until we have 2 validators.
    log::debug!("Polling the validator set of every replica until we have 2 validators.");
    while nodes[0].committed_validator_set().len() != 2
        || nodes[1].committed_validator_set().len() != 2
        || nodes[2].committed_validator_set().len() != 2
    {
        thread::sleep(Duration::from_millis(500));
    }
}

#[test]
fn pacemaker_initial_view_sync_test() {
    setup_logger(LevelFilter::Trace);

    let mut csprg = OsRng {};
    let keypair_1 = SigningKey::generate(&mut csprg);
    let keypair_2 = SigningKey::generate(&mut csprg);
    let init_as = {
        let mut state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), u32::to_le_bytes(0).to_vec());
        state
    };
    let init_vs_updates = {
        let mut vs_updates = ValidatorSetUpdates::new();
        vs_updates.insert(keypair_1.verifying_key(), Power::new(1));
        vs_updates.insert(keypair_2.verifying_key(), Power::new(1));
        vs_updates
    };
    let (network_stub_1, network_stub_2) = {
        let mut network_stubs =
            mock_network([keypair_1.verifying_key(), keypair_2.verifying_key()].into_iter());
        (network_stubs.remove(0), network_stubs.remove(0))
    };

    // Start the first node.
    log::debug!("Starting the first node.");
    let mut first_node = Node::new(
        keypair_1,
        network_stub_1,
        init_as.clone(),
        init_vs_updates.clone(),
    );

    thread::sleep(Duration::from_millis(4000));

    // Start the second node.
    log::debug!("Starting the second node.");
    let second_node = Node::new(
        keypair_2,
        network_stub_2,
        init_as.clone(),
        init_vs_updates.clone(),
    );

    // Wait until both nodes' current views match.
    log::debug!("Waiting until both nodes enter view 1.");
    while first_node.highest_view_entered() < ViewNumber::new(1)
        || second_node.highest_view_entered() < ViewNumber::new(1)
    {
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

#[test]
fn block_sync_test() {
    setup_logger(LevelFilter::Trace);

    let mut csprg = OsRng {};
    let mut keypairs: Vec<SigningKey> = (0..4).map(|_| SigningKey::generate(&mut csprg)).collect();
    let mut network_stubs = mock_network(keypairs.iter().map(|kp| kp.verifying_key()));
    let last_keypair = keypairs.split_off(3);
    let last_newtork = network_stubs.split_off(3);

    let init_as = {
        let mut state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), u32::to_le_bytes(0).to_vec());
        state
    };
    let init_vs_updates = {
        let mut vs_updates = ValidatorSetUpdates::new();
        vs_updates.insert(keypairs[0].verifying_key(), Power::new(1));
        vs_updates.insert(keypairs[1].verifying_key(), Power::new(1));
        vs_updates.insert(keypairs[2].verifying_key(), Power::new(1));
        vs_updates
    };

    let mut init_nodes: Vec<Node> = keypairs
        .into_iter()
        .zip(network_stubs)
        .map(|(keypair, network)| {
            Node::new(
                keypair.clone(),
                network,
                init_as.clone(),
                init_vs_updates.clone(),
            )
        })
        .collect();

    // Submit an Increment transaction to the initial validator.
    log::debug!("Submitting an Increment transaction to the initial validator.");
    init_nodes[0].submit_transaction(NumberAppTransaction::Increment);

    // Submit an Increment transaction to the initial validator.
    log::debug!("Submitting an Increment transaction to the initial validator.");
    init_nodes[1].submit_transaction(NumberAppTransaction::Increment);

    // Poll the app state of every replica until the value is 2.
    log::debug!("Polling the app state of every replica until the value is 1.");
    while init_nodes[0].number() != 2 || init_nodes[1].number() != 2 || init_nodes[2].number() != 2
    {
        thread::sleep(Duration::from_millis(500));
    }

    // Start the "lagging replica".
    log::debug!("Start the lagging replica.");
    let last_node = Node::new(
        last_keypair[0].clone(),
        last_newtork[0].clone(),
        init_as.clone(),
        init_vs_updates.clone(),
    );

    // Poll the app state of the lagging replica until it catches up with the others.
    log::debug!("Polling the app state of the last-joined replica .");
    while last_node.number() != 2 {
        thread::sleep(Duration::from_millis(500));
    }

    thread::sleep(Duration::from_millis(1000));
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
    my_verifying_key: VerifyingKey,
    all_peers: HashMap<VerifyingKey, Sender<(VerifyingKey, Message)>>,
    inbox: Arc<Mutex<Receiver<(VerifyingKey, Message)>>>,
}

impl Network for NetworkStub {
    fn init_validator_set(&mut self, _: ValidatorSet) {}

    fn update_validator_set(&mut self, _: ValidatorSetUpdates) {}

    fn send(&mut self, peer: VerifyingKey, message: Message) {
        if let Some(peer) = self.all_peers.get(&peer) {
            let _ = peer.send((self.my_verifying_key, message));
        }
    }

    fn broadcast(&mut self, message: Message) {
        for (_, peer) in &self.all_peers {
            let _ = peer.send((self.my_verifying_key, message.clone()));
        }
    }

    fn recv(&mut self) -> Option<(VerifyingKey, Message)> {
        match self.inbox.lock().unwrap().try_recv() {
            Ok(o_m) => Some(o_m),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!(),
        }
    }
}

fn mock_network(peers: impl Iterator<Item = VerifyingKey>) -> Vec<NetworkStub> {
    let mut all_peers = HashMap::new();
    let peer_and_inboxes: Vec<(VerifyingKey, Receiver<(VerifyingKey, Message)>)> = peers
        .map(|peer| {
            let (sender, receiver) = mpsc::channel();
            all_peers.insert(peer, sender);

            (peer, receiver)
        })
        .collect();

    peer_and_inboxes
        .into_iter()
        .map(|(my_verifying_key, inbox)| NetworkStub {
            my_verifying_key,
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
    SetValidator(VerifyingKeyBytes, Power),
    DeleteValidator(VerifyingKeyBytes),
}

impl App<MemDB> for NumberApp {
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
        let data = Data::new(vec![Datum::new(tx_queue.try_to_vec().unwrap())]);
        let data_hash = {
            let mut hasher = Sha256::new();
            hasher.update(&data.vec()[0].bytes());
            let bytes = hasher.finalize().into();
            CryptoHash::new(bytes)
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
            hasher.update(&data.vec()[0].bytes());
            let bytes = hasher.finalize().into();
            CryptoHash::new(bytes)
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

            if let Ok(transactions) = Vec::<NumberAppTransaction>::deserialize(
                &mut &*request.proposed_block().data.vec()[0].bytes().as_slice(),
            ) {
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

    fn validate_block_for_sync(
        &mut self,
        request: ValidateBlockRequest<MemDB>,
    ) -> ValidateBlockResponse {
        self.validate_block(request)
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
                        updates.insert(VerifyingKey::from_bytes(validator).unwrap(), *power);
                    } else {
                        let mut vsu = ValidatorSetUpdates::new();
                        vsu.insert(VerifyingKey::from_bytes(validator).unwrap(), *power);
                        validator_set_updates = Some(vsu);
                    }
                }
                NumberAppTransaction::DeleteValidator(validator) => {
                    if let Some(updates) = &mut validator_set_updates {
                        updates.delete(VerifyingKey::from_bytes(validator).unwrap());
                    } else {
                        let mut vsu = ValidatorSetUpdates::new();
                        vsu.delete(VerifyingKey::from_bytes(validator).unwrap());
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
    verifying_key: VerifyingKeyBytes,
    tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>,
    replica: Replica<MemDB>,
}

impl Node {
    fn new(
        keypair: SigningKey,
        network: NetworkStub,
        init_as: AppStateUpdates,
        init_vs_updates: ValidatorSetUpdates,
    ) -> Node {
        let kv_store = MemDB::new();

        let mut init_vs = ValidatorSet::new();
        init_vs.apply_updates(&init_vs_updates);
        let init_vs_state = ValidatorSetState::new(init_vs.clone(), init_vs, None, true);

        Replica::initialize(kv_store.clone(), init_as, init_vs_state);

        let verifying_key = keypair.verifying_key().to_bytes();
        let tx_queue = Arc::new(Mutex::new(Vec::new()));

        let configuration = Configuration::builder()
            .me(keypair)
            .chain_id(ChainID::new(0))
            .block_sync_request_limit(10)
            .block_sync_server_advertise_time(Duration::new(10, 0))
            .block_sync_response_timeout(Duration::new(3, 0))
            .block_sync_blacklist_expiry_time(Duration::new(10, 0))
            .block_sync_trigger_min_view_difference(2)
            .block_sync_trigger_timeout(Duration::new(60, 0))
            .progress_msg_buffer_capacity(BufferSize::new(1024))
            .epoch_length(EpochLength::new(50))
            .max_view_time(Duration::from_millis(2000))
            .log_events(false)
            .build();

        let insert_block_handler = |insert_block_event: &InsertBlockEvent| {
            log::debug!(
                "Inserted block with hash: {}, timestamp: {}",
                first_seven_base64_chars(&insert_block_event.block.hash.bytes()),
                secs_since_unix_epoch(insert_block_event.timestamp)
            )
        };

        let receive_proposal_handler = |receive_proposal_event: &ReceiveProposalEvent| {
            let txn = Vec::<NumberAppTransaction>::deserialize(
                &mut &*receive_proposal_event.proposal.block.data.vec()[0]
                    .bytes()
                    .as_slice(),
            )
            .unwrap();
            let txn_printable = if txn.is_empty() {
                String::from("no transactions")
            } else {
                let all: Vec<String> = txn
                    .iter()
                    .map(|tx| match tx {
                        NumberAppTransaction::Increment => String::from("increment, "),
                        NumberAppTransaction::SetValidator(_, _) => String::from("set validator, "),
                        NumberAppTransaction::DeleteValidator(_) => {
                            String::from("delete validator, ")
                        }
                    })
                    .collect();
                all.join(" ")
            };
            log::debug!(
                "{}, {}, origin: {}, {}, view: {}, block height: {}, transactions: {}",
                "Received Proposal",
                secs_since_unix_epoch(receive_proposal_event.timestamp),
                first_seven_base64_chars(&receive_proposal_event.origin.to_bytes()),
                first_seven_base64_chars(&receive_proposal_event.proposal.block.hash.bytes()),
                receive_proposal_event.proposal.view,
                receive_proposal_event.proposal.block.height.clone(),
                txn_printable
            )
        };

        let commit_block_handler = |commit_block_event: &CommitBlockEvent| {
            log::debug!(
                "{}, {}, {}",
                "Committed block",
                secs_since_unix_epoch(commit_block_event.timestamp),
                first_seven_base64_chars(&commit_block_event.block.bytes())
            )
        };

        let update_highest_qc_handler = |update_highest_qc_event: &UpdateHighestQCEvent| {
            log::debug!(
                "{}, {}, block: {}, view: {}, phase: {:?}, no. of signatures = {}",
                "Updated HighestQC",
                secs_since_unix_epoch(update_highest_qc_event.timestamp),
                first_seven_base64_chars(&update_highest_qc_event.highest_qc.block.bytes()),
                update_highest_qc_event.highest_qc.view,
                update_highest_qc_event.highest_qc.phase,
                update_highest_qc_event
                    .highest_qc
                    .signatures
                    .iter()
                    .filter(|sig| sig.is_some())
                    .count()
            )
        };

        let vote_handler = |vote_event: &VoteEvent| {
            log::debug!(
                "{}, {}, {}, {}, {:?}",
                "Voted",
                secs_since_unix_epoch(vote_event.timestamp),
                first_seven_base64_chars(&vote_event.vote.block.bytes()),
                vote_event.vote.view,
                vote_event.vote.phase
            )
        };

        let replica = ReplicaSpec::builder()
            .app(NumberApp::new(tx_queue.clone()))
            .network(network)
            .kv_store(kv_store)
            .configuration(configuration)
            .on_insert_block(insert_block_handler)
            .on_receive_proposal(receive_proposal_handler)
            .on_commit_block(commit_block_handler)
            .on_update_highest_qc(update_highest_qc_handler)
            .on_vote(vote_handler)
            .build()
            .start();

        Node {
            verifying_key,
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
            .expect("Cannot obtain the committed validator set from the block tree!")
    }

    fn highest_view_entered(&self) -> ViewNumber {
        self.replica
            .block_tree_camera()
            .snapshot()
            .highest_view_entered()
            .expect("Cannot obtain the highest view entered from the block tree!")
    }

    fn verifying_key(&self) -> VerifyingKeyBytes {
        self.verifying_key
    }
}

// Get a more readable representation of a bytesequence by base64-encoding it and taking the first 7 characters.
fn first_seven_base64_chars(bytes: &[u8]) -> String {
    let encoded = STANDARD_NO_PAD.encode(bytes);
    if encoded.len() > 7 {
        encoded[0..7].to_string()
    } else {
        encoded
    }
}

fn secs_since_unix_epoch(timestamp: SystemTime) -> u64 {
    timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Event occured before the Unix Epoch.")
        .as_secs()
}
