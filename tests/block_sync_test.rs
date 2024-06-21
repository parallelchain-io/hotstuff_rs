use std::{thread, time::Duration};

use rand_core::OsRng;

use hotstuff_rs::types::{basic::Power, collectors::SigningKey, validators::ValidatorSetUpdates};

mod common;

use common::{
    logging::log_with_context,
    network::mock_network,
    node::Node,
    number_app::{NumberApp, NumberAppTransaction},
};

#[test]
fn block_sync_test() {
    let mut csprg = OsRng {};
    let mut keypairs: Vec<SigningKey> = (0..4).map(|_| SigningKey::generate(&mut csprg)).collect();
    let mut network_stubs = mock_network(keypairs.iter().map(|kp| kp.verifying_key()));
    let last_keypair = keypairs.split_off(3);
    let last_newtork = network_stubs.split_off(3);

    let init_as = NumberApp::initial_app_state();

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
    log_with_context(
        None,
        "Submitting an Increment transaction to the initial validator.",
    );
    init_nodes[0].submit_transaction(NumberAppTransaction::Increment);

    // Submit an Increment transaction to the initial validator.
    log_with_context(
        None,
        "Submitting an Increment transaction to the initial validator.",
    );
    init_nodes[1].submit_transaction(NumberAppTransaction::Increment);

    // Poll the app state of every replica until the value is 2.
    log_with_context(
        None,
        "Polling the app state of every replica until the value is 1.",
    );
    while init_nodes[0].number() != 2 || init_nodes[1].number() != 2 || init_nodes[2].number() != 2
    {
        thread::sleep(Duration::from_millis(500));
    }

    // Start the "lagging replica".
    log_with_context(None, "Start the lagging replica.");
    let last_node = Node::new(
        last_keypair[0].clone(),
        last_newtork[0].clone(),
        init_as.clone(),
        init_vs_updates.clone(),
    );

    // Poll the app state of the lagging replica until it catches up with the others.
    log_with_context(None, "Polling the app state of the last-joined replica .");
    while last_node.number() != 2 {
        thread::sleep(Duration::from_millis(500));
    }

    thread::sleep(Duration::from_millis(1000));
}
