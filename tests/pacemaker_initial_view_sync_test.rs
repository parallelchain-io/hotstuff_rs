use std::{thread, time::Duration};

use rand_core::OsRng;

use hotstuff_rs::types::{
    crypto_primitives::SigningKey,
    data_types::{Power, ViewNumber},
    update_sets::ValidatorSetUpdates,
};

mod common;

use common::{
    logging::log_with_context,
    network::mock_network,
    node::Node,
    number_app::{NumberApp, NumberAppTransaction},
};

/// Tests that the pacemaker is able to make progress past view 0 (which is an epoch-change view).
///
/// Sets up a network consisting of two replicas, initially starts one replica only and confirms that
/// this replica cannot proceed beyond view number 0. Then, starts the other replica and confirms
/// that the replicas can now enter views higher 0.
#[test]
fn pacemaker_initial_view_sync_test() {
    // 1. Initialize test components.

    // 1.1. Create signing keys for 2 replicas.
    let mut csprg = OsRng {};
    let keypair_1 = SigningKey::generate(&mut csprg);
    let keypair_2 = SigningKey::generate(&mut csprg);

    // 1.2. Initialize the app state of the number app to the number 0.
    let init_as = NumberApp::initial_app_state();

    // 1.3. Initialize the validator set
    let init_vs_updates = {
        let mut vs_updates = ValidatorSetUpdates::new();
        vs_updates.insert(keypair_1.verifying_key(), Power::new(1));
        vs_updates.insert(keypair_2.verifying_key(), Power::new(1));
        vs_updates
    };

    // 1.4. Create a test network connecting the two replicas. We store each network stub in separate
    // variables so that we can start the replicas separately later on.
    let (network_stub_1, network_stub_2) = {
        let mut network_stubs =
            mock_network([keypair_1.verifying_key(), keypair_2.verifying_key()].into_iter());
        (network_stubs.remove(0), network_stubs.remove(0))
    };

    // 2. Start the first replica *only*. Confirm that its view number doesn't increase.

    // 2.1. Start the first replica.
    log_with_context(None, "Starting the first replica.");
    let mut first_node = Node::new(
        keypair_1,
        network_stub_1,
        init_as.clone(),
        init_vs_updates.clone(),
    );

    // 2.2. Wait for 5 seconds.
    thread::sleep(Duration::from_millis(5000));

    // 2.3. Confirm that its view number remains at 0.
    assert_eq!(first_node.highest_view_entered(), ViewNumber::new(0));

    // 3. Start the second replica. Confirm that both replicas' view number now increase.

    // 3.1. Start the second replica.
    log_with_context(None, "Starting the second replica.");
    let second_node = Node::new(
        keypair_2,
        network_stub_2,
        init_as.clone(),
        init_vs_updates.clone(),
    );

    // 3.2. Confirm that both replicas' view numbers are now 1 or greater.
    log_with_context(None, "Waiting until both nodes enter view 1.");
    while first_node.highest_view_entered() < ViewNumber::new(1)
        || second_node.highest_view_entered() < ViewNumber::new(1)
    {
        thread::sleep(Duration::from_millis(500));
    }

    // 4. Test updating the app state.

    // 4.1. Submit an Increment transaction.
    log_with_context(None, "Submitting an increment transaction.");
    first_node.submit_transaction(NumberAppTransaction::Increment);

    // 4.2. Wait until the app state of both nodes has value == 1.
    log_with_context(None, "Waiting until both nodes' values are 1.");
    while first_node.number() != 1 || second_node.number() != 1 {
        thread::sleep(Duration::from_millis(500));
    }
}
