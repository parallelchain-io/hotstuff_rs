use std::{thread, time::Duration};

use rand_core::OsRng;

use hotstuff_rs::types::{
    crypto_primitives::SigningKey, data_types::Power, update_sets::ValidatorSetUpdates,
};

mod common;

use common::{
    logging::log_with_context,
    network::mock_network,
    node::Node,
    number_app::{NumberApp, NumberAppTransaction},
};

/// Tests block sync.
///
/// Starts a network with three replicas, makes progress with them, then starts another replica (a
/// "lagging" replica) and confirms that it can sync with the initial three replicas.
#[test]
fn block_sync_test() {
    // 1. Initialize test components.

    // 1.1. Generate signing keys for 4 replicas.
    let mut csprg = OsRng {};
    let mut keypairs: Vec<SigningKey> = (0..4).map(|_| SigningKey::generate(&mut csprg)).collect();

    // 1.2. Create a mock network connecting the 4 replicas.
    let mut network_stubs = mock_network(keypairs.iter().map(|kp| kp.verifying_key()));

    // 1.3. Split off the last keypair and network stub (these will be used to create the lagging replica later
    // on in the test).
    let lagging_replica_keypair = keypairs.split_off(3);
    let lagging_replica_network = network_stubs.split_off(3);

    // 1.4. Initialize the app state of the number app to the number 0.
    let init_as = NumberApp::initial_app_state();

    // 1.4. Initialize the validator set of the cluster to contain 4 replicas.
    let init_vs_updates = {
        let mut vs_updates = ValidatorSetUpdates::new();
        vs_updates.insert(keypairs[0].verifying_key(), Power::new(1));
        vs_updates.insert(keypairs[1].verifying_key(), Power::new(1));
        vs_updates.insert(keypairs[2].verifying_key(), Power::new(1));
        vs_updates
    };

    // 1.5. Simultaneously start the first 3 replicas.
    let mut live_nodes: Vec<Node> = keypairs
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

    // 2. Test making progress with the live replicas.

    // 2.1. Submit 2 Increment transactions to the live replicas.
    log_with_context(
        None,
        "Submitting one Increment transactions to each of replica 0 and replica 1.",
    );
    live_nodes[0].submit_transaction(NumberAppTransaction::Increment);
    live_nodes[1].submit_transaction(NumberAppTransaction::Increment);

    // 2.2. Poll the app state of the live replicas until each sees the number as 2.
    log_with_context(
        None,
        "Polling the app state of the live replicas until each sees the number as 2.",
    );
    while !live_nodes.iter().all(|node| node.number() == 2) {
        thread::sleep(Duration::from_millis(500));
    }

    // 3. Test whether the lagging replica can sync up to the live replicas.

    // 3.1. Start the lagging replica.
    log_with_context(None, "Start the lagging replica.");
    let lagging_node = Node::new(
        lagging_replica_keypair[0].clone(),
        lagging_replica_network[0].clone(),
        init_as.clone(),
        init_vs_updates.clone(),
    );

    // 3.2. Poll the app state of the lagging replica until it sees the number as 2.
    log_with_context(None, "Polling the app state of the lagging replica.");
    while lagging_node.number() != 2 {
        thread::sleep(Duration::from_millis(500));
    }
}
