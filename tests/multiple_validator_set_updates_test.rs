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

/// Tests "extreme" validator set updates.
///
/// Makes two different kinds of validator set updates in sequence:
/// 1. A validator set update that adds a validator with much more power than the rest.
/// 2. A validator set update that removes all existing validators and adds completely new ones.
#[test]
fn multiple_validator_set_updates_test() {
    // 1. Initialize test components.

    // 1.1. Generate signing keys for 6 replicas.
    let mut csprg = OsRng {};
    let keypairs: Vec<SigningKey> = (0..6).map(|_| SigningKey::generate(&mut csprg)).collect();

    // 1.2. Create a mock network connecting the 6 replicas.
    let network_stubs = mock_network(keypairs.iter().map(|kp| kp.verifying_key()));

    // 1.3. Initialize the app state of the number app to the number 0.
    let init_as = NumberApp::initial_app_state();

    // 1.4. Initialize the validator set of the cluster to initially contain only two replicas (index 0 and
    // index 1).
    let init_vs_updates = {
        let mut vs_updates = ValidatorSetUpdates::new();
        vs_updates.insert(keypairs[0].verifying_key(), Power::new(1));
        vs_updates.insert(keypairs[1].verifying_key(), Power::new(1));
        vs_updates
    };

    // 1.5. Simultaneously start all 6 replicas.
    let mut nodes: Vec<Node> = keypairs
        .into_iter()
        .zip(network_stubs)
        .map(|(keypair, network)| {
            Node::new(keypair, network, init_as.clone(), init_vs_updates.clone())
        })
        .collect();

    // 2. Test adding one more validator to the validator set.

    // 2.1. Submit a Set Validator transaction to an existing validator.
    log_with_context(None, "Submitting a set validator transaction to the initial validator to register 1 more validator (small power).");
    let node_2 = nodes[2].verifying_key();
    nodes[0].submit_transaction(NumberAppTransaction::SetValidator(node_2, Power::new(1)));

    // 2.2. Poll the validator set of every replica until each sees 3 validators in their committed
    // validator set.
    log_with_context(
        None,
        "Polling the validator set of every replica until all see 3 validators.",
    );
    while !nodes
        .iter()
        .all(|node| node.committed_validator_set().len() == 3)
    {
        thread::sleep(Duration::from_millis(500));
    }

    // 3. Test adding one more validator (this time with *big* power) to the validator set.

    // 3.1. Submit another Set Validator transaction to an existing validator.
    log_with_context(None, "Submitting a set validator transaction to one of the initial validators to register 1 more validator (big power).");
    let node_3 = nodes[3].verifying_key();
    nodes[1].submit_transaction(NumberAppTransaction::SetValidator(node_3, Power::new(3)));

    // 3.2. Poll the validator set of every replica until each sees 4 validators in their committed
    // validator set.
    log_with_context(
        None,
        "Polling the validator set of every replica until all see 4 validators.",
    );
    while !nodes
        .iter()
        .all(|node| node.committed_validator_set().len() == 4)
    {
        thread::sleep(Duration::from_millis(500));
    }

    // 4. Test replacing the validator set with a totally different, disjoint validator set.

    // 4.1. Submit Set Validator transactions to existing validators to:
    //     - Remove all existing validators, and
    //     - Add two new validators: nodes[4] and nodes[5]
    log_with_context(None, "Submitting a set validator transaction to one of the initial validators to delete the current validators and add two new validators.");
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

    // 4.2. Poll the validator set of every replica until each sees 2 validators in their committed
    // validator set.
    log_with_context(
        None,
        "Polling the validator set of every replica until all see 2 validators.",
    );
    while !nodes
        .iter()
        .all(|node| node.committed_validator_set().len() == 2)
    {
        thread::sleep(Duration::from_millis(500));
    }
}
