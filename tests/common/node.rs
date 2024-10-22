//! `[Node]`, a wrapper that manages access to a replica built around `NumberApp`.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use borsh::BorshDeserialize;
use ed25519_dalek::SigningKey;
use hotstuff_rs::{
    events::{
        CommitBlockEvent, InsertBlockEvent, PhaseVoteEvent, ReceiveProposalEvent,
        UpdateHighestPCEvent,
    },
    replica::{Configuration, Replica, ReplicaSpec},
    types::{
        data_types::{BufferSize, ChainID, EpochLength, ViewNumber},
        update_sets::{AppStateUpdates, ValidatorSetUpdates},
        validator_set::{ValidatorSet, ValidatorSetState},
    },
};

use crate::common::{
    mem_db::MemDB,
    network::NetworkStub,
    number_app::{NumberApp, NumberAppTransaction},
    verifying_key_bytes::VerifyingKeyBytes,
};

use super::logging::{first_seven_base64_chars, log_with_context};

/// A single "node" in an integration test cluster built around number app.
///
/// Users can do two sets of things with a `Node` by calling its methods:
/// 1. Submit a `NumberAppTransaction` to the node ([`submit_transaction`](Self::submit_transaction)).
/// 2. Query the state of the replica and/or the app (e.g., [`number`](Self::number),
/// [`verifying_key`](Self::verifying_key), [`committed_validator_set`](Self::committed_validator_set)).
pub(crate) struct Node {
    verifying_key: VerifyingKeyBytes,
    tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>,
    replica: Replica<MemDB>,
}

impl Node {
    /// Create a new `Node` that signs messages using the provided `keypair`, receives and sends messages
    /// using the provided `network_stub`, initializing its app state and validator set using
    /// `init_as_updates` and `init_vs_updates` respectively.
    ///
    /// The replica in the returned `Node` starts working immediately, and is automatically shut down when
    /// the node is dropped.
    pub(crate) fn new(
        keypair: SigningKey,
        network_stub: NetworkStub,
        init_as_updates: AppStateUpdates,
        init_vs_updates: ValidatorSetUpdates,
    ) -> Node {
        let kv_store = MemDB::new();

        let mut init_vs = ValidatorSet::new();
        init_vs.apply_updates(&init_vs_updates);
        let init_vs_state = ValidatorSetState::new(init_vs.clone(), init_vs, None, true);

        Replica::initialize(kv_store.clone(), init_as_updates, init_vs_state);

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
            // `max_view_time` must be **at least** 500 milliseconds, since `NumberApp`'s `produce_block` and
            // `validate_block` each take a minimum of 250 milliseconds to complete.
            .max_view_time(Duration::from_millis(2000))
            .log_events(false)
            .build();

        let replica = ReplicaSpec::builder()
            .app(NumberApp::new(tx_queue.clone()))
            .network(network_stub)
            .kv_store(kv_store)
            .configuration(configuration)
            .on_insert_block(insert_block_handler(verifying_key))
            .on_receive_proposal(receive_proposal_handler(verifying_key))
            .on_commit_block(commit_block_handler(verifying_key))
            .on_update_highest_pc(update_highest_pc_handler(verifying_key))
            .on_phase_vote(phase_vote_handler(verifying_key))
            .build()
            .start();

        Node {
            verifying_key,
            replica,
            tx_queue,
        }
    }

    /// Push the transaction `txn` to the node's local number app transaction queue.
    pub(crate) fn submit_transaction(&mut self, txn: NumberAppTransaction) {
        self.tx_queue.lock().unwrap().push(txn);
    }

    /// Query the number in the node's local app state.
    pub(crate) fn number(&self) -> u32 {
        NumberApp::number(self.replica.block_tree_camera().snapshot())
    }

    /// Query the committed validator set in the node's local block tree.
    pub(crate) fn committed_validator_set(&self) -> ValidatorSet {
        self.replica
            .block_tree_camera()
            .snapshot()
            .committed_validator_set()
            .expect("should have been able to get the committed validator set from the block tree")
    }

    /// Query the highest view entered in the node's local block tree.
    pub(crate) fn highest_view_entered(&self) -> ViewNumber {
        self.replica
            .block_tree_camera()
            .snapshot()
            .highest_view_entered()
            .expect("should have been able to get the highest view entered from the block tree")
    }

    /// Query the verifying key of the node.
    pub(crate) fn verifying_key(&self) -> VerifyingKeyBytes {
        self.verifying_key
    }
}

/// Return a closure that logs out an `InsertBlockEvent` in a human-readable way.
fn insert_block_handler(
    verifying_key: VerifyingKeyBytes,
) -> impl Fn(&InsertBlockEvent) + Send + 'static {
    move |insert_block_event| {
        log_with_context(
            Some(verifying_key),
            &format!(
                "Inserted Block, block hash: {}",
                first_seven_base64_chars(&insert_block_event.block.hash.bytes())
            ),
        );
    }
}

/// Return a closure that logs out an `ReceiveProposalEvent` in a human-readable way.
fn receive_proposal_handler(
    verifying_key: VerifyingKeyBytes,
) -> impl Fn(&ReceiveProposalEvent) + Send + 'static {
    move |receive_proposal_event| {
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
                    NumberAppTransaction::Increment => String::from("Increment"),
                    NumberAppTransaction::SetValidator(_, _) => String::from("Set Validator"),
                    NumberAppTransaction::DeleteValidator(_) => String::from("Delete Validator"),
                })
                .collect();
            all.join(", ")
        };

        log_with_context(
            Some(verifying_key),
            &format!("Received Proposal, origin: {}, view: {}, block hash: {}, block height: {}, transactions: {}",
                    first_seven_base64_chars(&receive_proposal_event.origin.to_bytes()),
                    receive_proposal_event.proposal.view,
                    first_seven_base64_chars(&receive_proposal_event.proposal.block.hash.bytes()),
                    receive_proposal_event.proposal.block.height.clone(),
                    txn_printable
            )
        );
    }
}

/// Return a closure that logs out an `CommitBlockEvent` in a human-readable way.
fn commit_block_handler(
    verifying_key: VerifyingKeyBytes,
) -> impl Fn(&CommitBlockEvent) + Send + 'static {
    move |commit_block_event: &CommitBlockEvent| {
        log_with_context(
            Some(verifying_key),
            &format!(
                "Committed Block, block hash: {}",
                first_seven_base64_chars(&commit_block_event.block.bytes())
            ),
        );
    }
}

/// Return a closure that logs out an `UpdateHighestPCEvent` in a human-readable way.
fn update_highest_pc_handler(
    verifying_key: VerifyingKeyBytes,
) -> impl Fn(&UpdateHighestPCEvent) + Send + 'static {
    move |update_highest_pc_event| {
        log_with_context(
            Some(verifying_key),
            &format!(
                "Updated Highest PC, block hash: {}, view: {}, phase: {:?}, no. of signatures: {}",
                first_seven_base64_chars(&update_highest_pc_event.highest_pc.block.bytes()),
                update_highest_pc_event.highest_pc.view,
                update_highest_pc_event.highest_pc.phase,
                update_highest_pc_event
                    .highest_pc
                    .signatures
                    .iter()
                    .filter(|sig| sig.is_some())
                    .count()
            ),
        );
    }
}

/// Return a closure that logs out an `VoteEvent` in a human-readable way.
fn phase_vote_handler(
    verifying_key: VerifyingKeyBytes,
) -> impl Fn(&PhaseVoteEvent) + Send + 'static {
    move |phase_vote_event: &PhaseVoteEvent| {
        log_with_context(
            Some(verifying_key),
            &format!(
                "Phase Voted, block hash: {}, view: {}, phase: {:?}",
                first_seven_base64_chars(&phase_vote_event.vote.block.bytes()),
                phase_vote_event.vote.view,
                phase_vote_event.vote.phase,
            ),
        );
    }
}
