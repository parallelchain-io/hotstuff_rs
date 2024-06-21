use std::{thread, time::Duration};

use rand_core::OsRng;

use hotstuff_rs::types::{
    basic::{Power, ViewNumber},
    collectors::SigningKey,
    validators::ValidatorSetUpdates,
};

mod common;

use common::{
    logging::log_with_context,
    network::mock_network,
    node::Node,
    number_app::{NumberApp, NumberAppTransaction},
};

#[test]
fn pacemaker_initial_view_sync_test() {
    let mut csprg = OsRng {};
    let keypair_1 = SigningKey::generate(&mut csprg);
    let keypair_2 = SigningKey::generate(&mut csprg);

    let init_as = NumberApp::initial_app_state();

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
    log_with_context(None, "Starting the first node.");
    let mut first_node = Node::new(
        keypair_1,
        network_stub_1,
        init_as.clone(),
        init_vs_updates.clone(),
    );

    thread::sleep(Duration::from_millis(4000));

    // Start the second node.
    log_with_context(None, "Starting the second node.");
    let second_node = Node::new(
        keypair_2,
        network_stub_2,
        init_as.clone(),
        init_vs_updates.clone(),
    );

    // Wait until both nodes' current views match.
    log_with_context(None, "Waiting until both nodes enter view 1.");
    while first_node.highest_view_entered() < ViewNumber::new(1)
        || second_node.highest_view_entered() < ViewNumber::new(1)
    {
        thread::sleep(Duration::from_millis(500));
    }

    // Submit an Increment transaction.
    log_with_context(None, "Submitting an increment transaction.");
    first_node.submit_transaction(NumberAppTransaction::Increment);

    // Wait until the app state of both nodes has value == 1.
    log_with_context(None, "Waiting until both nodes' values are 1.");
    while first_node.number() != 1 || second_node.number() != 1 {
        thread::sleep(Duration::from_millis(500));
    }
}
