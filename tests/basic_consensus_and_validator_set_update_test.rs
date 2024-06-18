use std::{thread, time::Duration};

use hotstuff_rs::types::{
    basic::{AppStateUpdates, Power},
    collectors::SigningKey,
    validators::ValidatorSetUpdates,
};
use log::LevelFilter;
use rand_core::OsRng;

mod common;

use crate::common::{
    logging::setup_logger,
    network::mock_network,
    node::Node,
    number_app::{NumberAppTransaction, NUMBER_KEY},
};

#[test]
fn basic_consensus_and_validator_set_update_test() {
    setup_logger(LevelFilter::Trace);

    // 1. Initialize test components.

    // 1.1. Create signing keys for 3 replicas.
    let mut csprg = OsRng {};
    let keypairs: Vec<SigningKey> = (0..3).map(|_| SigningKey::generate(&mut csprg)).collect();

    // 1.2. Create a mock network connecting the 3 replicas.
    let network_stubs = mock_network(keypairs.iter().map(|kp| kp.verifying_key()));

    // 1.3. Initialize the app state of the Number App to 0.
    let init_as = {
        let mut state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), u32::to_le_bytes(0).to_vec());
        state
    };

    // 1.4. Initialize the validator set of the cluster to initially contain only the replica with index 0.
    let init_vs_updates = {
        let mut vs_updates = ValidatorSetUpdates::new();
        vs_updates.insert(keypairs[0].verifying_key(), Power::new(1));
        vs_updates
    };

    // 1.5 Simultaneously start all replicas.
    let mut nodes: Vec<Node> = keypairs
        .into_iter()
        .zip(network_stubs)
        .map(|(keypair, network)| {
            Node::new(keypair, network, init_as.clone(), init_vs_updates.clone())
        })
        .collect();

    // 2. Test updating the app state with a singleton validator.

    // 2.1. Submit an Increment transaction to the initial validator.
    log::debug!("Submitting an Increment transaction to the initial validator.");
    nodes[0].submit_transaction(NumberAppTransaction::Increment);

    // 2.2. Poll the app state of every replica until the value is 1.
    log::debug!("Polling the app state of every replica until the value is 1.");
    while nodes[0].number() != 1 || nodes[1].number() != 1 || nodes[2].number() != 1 {
        thread::sleep(Duration::from_millis(500));
    }

    // 3. Test dynamically expanding the validator set.

    // 3.1. Submit 2 set validator transactions to the initial validator to register the rest (2) of the peers.
    log::debug!("Submitting 2 set validator transactions to the initial validator to register the rest (2) of the peers.");
    let node_1 = nodes[1].verifying_key();
    nodes[0].submit_transaction(NumberAppTransaction::SetValidator(node_1, Power::new(1)));
    let node_2 = nodes[2].verifying_key();
    nodes[0].submit_transaction(NumberAppTransaction::SetValidator(node_2, Power::new(1)));

    // 3.2. Poll the validator set of every replica until we have 3 validators.
    log::debug!("Polling the validator set of every replica until we have 3 validators.");
    while nodes[0].committed_validator_set().len() != 3
        || nodes[1].committed_validator_set().len() != 3
        || nodes[2].committed_validator_set().len() != 3
    {
        thread::sleep(Duration::from_millis(500));
    }

    // 4. Test updating the app state now that we have 3 validators.

    // 4.1. Push an Increment transaction to each of the 3 validators we have now.
    log::debug!("Submitting an increment transaction to each of the 3 validators we have now.");
    nodes[0].submit_transaction(NumberAppTransaction::Increment);
    nodes[1].submit_transaction(NumberAppTransaction::Increment);
    nodes[2].submit_transaction(NumberAppTransaction::Increment);

    // 4.2. Poll the app state of every replica until the value is 4.
    log::debug!("Polling the app state of every replica until the value is 4");
    while nodes[0].number() != 4 || nodes[1].number() != 4 || nodes[2].number() != 4 {
        thread::sleep(Duration::from_millis(500));
    }
}
