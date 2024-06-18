use std::{thread, time::Duration};

use log::LevelFilter;
use rand_core::OsRng;

use hotstuff_rs::types::{
    basic::{AppStateUpdates, Power},
    collectors::SigningKey,
    validators::ValidatorSetUpdates,
};

mod common;

use common::{
    logging::setup_logger,
    network::mock_network,
    node::Node,
    number_app::{NumberAppTransaction, NUMBER_KEY},
};

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
