# HotStuff-rs Integration Tests

The integration test suite for HotStuff-rs involves an app (`NumberApp`) that keeps track of a single number its state, which is initially 0. Tests push transactions to this app to increase this number, or change its validator set, and then query its state to check if consensus is proceeding.

The replicas used in this test suite use a mock `NetworkStub`, a mock `MemDB`(key-value store). These use channels to simulate communication, and a hashmap to simulate persistence, and thus never leaves any persistent artifacts.

There are currently three tests:
1. `basic_consensus_and_validator_set_update_test`: tests the most basic user-visible functionalities: committing transactions, querying app state, and expanding the validator set. This should complete in less than 40 seconds.
2. `multiple_validator_set_updates_test`: tests if frequent validator set updates, including ones that completely change the validator set (i.e., there is no intersection between the old and the new validator sets), can succeed. This should complete in less than 1 minute.
3. `pacemaker_initial_view_sync_test`: tests whether validators can succesfully synchronize in the initial view even if they start executing the protocol at different moments. This should complete in less than 30 seconds.