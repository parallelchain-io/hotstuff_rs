use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use borsh::BorshDeserialize;
use ed25519_dalek::SigningKey;
use hotstuff_rs::{
    events::{
        CommitBlockEvent, InsertBlockEvent, ReceiveProposalEvent, UpdateHighestQCEvent, VoteEvent,
    },
    replica::{Configuration, Replica, ReplicaSpec},
    types::{
        basic::{AppStateUpdates, BufferSize, ChainID, EpochLength, ViewNumber},
        validators::{ValidatorSet, ValidatorSetState, ValidatorSetUpdates},
    },
};

use crate::common::{
    mem_db::MemDB,
    network::NetworkStub,
    number_app::{NumberApp, NumberAppTransaction, NUMBER_KEY},
    verifying_key_bytes::VerifyingKeyBytes,
};

use super::{
    logging::{first_seven_base64_chars, log_with_context},
    verifying_key_bytes,
};

/// Things the Nodes will have in common:
/// - Initial Validator Set.
/// - Configuration.
///
/// Things that they will differ in:
/// - App instance.
/// - Network instance.
/// - KVStore.
/// - Keypair.
pub(crate) struct Node {
    verifying_key: VerifyingKeyBytes,
    tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>,
    replica: Replica<MemDB>,
}

impl Node {
    pub(crate) fn new(
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

        let replica = ReplicaSpec::builder()
            .app(NumberApp::new(tx_queue.clone()))
            .network(network)
            .kv_store(kv_store)
            .configuration(configuration)
            .on_insert_block(insert_block_handler(verifying_key))
            .on_receive_proposal(receive_proposal_handler(verifying_key))
            .on_commit_block(commit_block_handler(verifying_key))
            .on_update_highest_qc(update_highest_qc_handler(verifying_key))
            .on_vote(vote_handler(verifying_key))
            .build()
            .start();

        Node {
            verifying_key,
            replica,
            tx_queue,
        }
    }

    pub(crate) fn submit_transaction(&mut self, txn: NumberAppTransaction) {
        self.tx_queue.lock().unwrap().push(txn);
    }

    pub(crate) fn number(&self) -> u32 {
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

    pub(crate) fn committed_validator_set(&self) -> ValidatorSet {
        self.replica
            .block_tree_camera()
            .snapshot()
            .committed_validator_set()
            .expect("Cannot obtain the committed validator set from the block tree!")
    }

    pub(crate) fn highest_view_entered(&self) -> ViewNumber {
        self.replica
            .block_tree_camera()
            .snapshot()
            .highest_view_entered()
            .expect("Cannot obtain the highest view entered from the block tree!")
    }

    pub(crate) fn verifying_key(&self) -> VerifyingKeyBytes {
        self.verifying_key
    }
}

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

fn update_highest_qc_handler(
    verifying_key: VerifyingKeyBytes,
) -> impl Fn(&UpdateHighestQCEvent) + Send + 'static {
    move |update_highest_qc_event| {
        log_with_context(
            Some(verifying_key),
            &format!(
                "Updated Highest QC, block hash: {}, view: {}, phase: {:?}, no. of signatures: {}",
                first_seven_base64_chars(&update_highest_qc_event.highest_qc.block.bytes()),
                update_highest_qc_event.highest_qc.view,
                update_highest_qc_event.highest_qc.phase,
                update_highest_qc_event
                    .highest_qc
                    .signatures
                    .iter()
                    .filter(|sig| sig.is_some())
                    .count()
            ),
        );
    }
}

fn vote_handler(verifying_key: VerifyingKeyBytes) -> impl Fn(&VoteEvent) + Send + 'static {
    move |vote_event: &VoteEvent| {
        log_with_context(
            Some(verifying_key),
            &format!(
                "Voted, block hash: {}, view: {}, phase: {:?}",
                first_seven_base64_chars(&vote_event.vote.block.bytes()),
                vote_event.vote.view,
                vote_event.vote.phase,
            ),
        );
    }
}
