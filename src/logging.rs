/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Functions for logging out library events of varying levels of importance.
//!
//! HotStuff-rs logs using the [log](https://docs.rs/log/latest/log/) crate. To get these messages
//! printed onto a terminal or to a file, set up a [logging
//! implementation](https://docs.rs/log/latest/log/#available-logging-implementations).
//!
//! Log messages with past tense event names (e.g., "Proposed") indicate an activity that has completed

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use log;

pub(crate) use crate::events::*;

pub(crate) trait Logger {
    fn get_logger() -> Box<dyn Fn(&Self) + Send>;
}

pub const INSERT_BLOCK: &str = "InsertedBlock";
pub const COMMIT_BLOCK: &str = "CommittedBlock";
pub const PRUNE_BLOCK: &str = "PrunedBlock";
pub const UPDATE_HIGHEST_QC: &str = "UpdatedHighestQC";
pub const UPDATE_LOCKED_VIEW: &str = "UpdateLockedView";
pub const UPDATE_VALIDATOR_SET: &str = "UpdatedValidatorSet";

pub const PROPOSE: &str = "Proposed";
pub const NUDGE: &str = "Nudged";
pub const VOTE: &str = "Voted";
pub const NEWVIEW: &str = "NewView";

pub const RECEIVED_PROPOSAL: &str = "ReceivedProposal";
pub const RECEIVED_NUDGE: &str = "ReceivedNudge";
pub const RECEIVED_VOTE: &str = "ReceivedVote";
pub const RECEIVED_NEW_VIEW: &str = "ReceivedNewView";

pub const START_VIEW: &str = "EnteredView";
pub const VIEW_TIME_OUT: &str = "ViewTimedOut";
pub const COLLECT_QC: &str = "CollectedQC";

pub const START_SYNC: &str = "StartedSyncing";
pub const END_SYNC: &str = "FinishedSyncing";
pub const RECEIVE_SYNC_REQUEST: &str = "ReceivedSyncRequest";
pub const SEND_SYNC_RESPONSE: &str = "SentSyncResponse";

impl Logger for InsertBlockEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |insert_block_event: &InsertBlockEvent| {
            log::debug!("{}, {:?}, {}, {}", INSERT_BLOCK, insert_block_event.timestamp, succinct(&insert_block_event.block.hash), insert_block_event.block.height)
        };
        Box::new(logger)
    }
}

impl Logger for CommitBlockEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |commit_block_event: &CommitBlockEvent| {
            log::info!("{}, {:?}, {}", COMMIT_BLOCK, commit_block_event.timestamp, succinct(&commit_block_event.block))
        };
        Box::new(logger)
    }
}

impl Logger for PruneBlockEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |prune_block_event: &PruneBlockEvent| {
            log::info!("{}, {:?}, {}", PRUNE_BLOCK, prune_block_event.timestamp, succinct(&prune_block_event.block))
        };
        Box::new(logger)
    }
}

impl Logger for UpdateHighestQCEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |update_highest_qc_event: &UpdateHighestQCEvent| {
            log::info!(
                "{}, {:?}, {}, {}, {:?}",
                UPDATE_HIGHEST_QC,
                update_highest_qc_event.timestamp,
                succinct(&update_highest_qc_event.highest_qc.block),
                update_highest_qc_event.highest_qc.view,
                update_highest_qc_event.highest_qc.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for UpdateLockedViewEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |update_locked_view_event: &UpdateLockedViewEvent| {
            log::info!(
                "{}, {:?}, {}",
                UPDATE_LOCKED_VIEW,
                update_locked_view_event.timestamp,
                update_locked_view_event.locked_view
            )
        };
        Box::new(logger)
    }
}

impl Logger for UpdateValidatorSetEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |update_validator_set_event: &UpdateValidatorSetEvent| {
            log::info!("{}, {:?}, {}", UPDATE_VALIDATOR_SET, update_validator_set_event.timestamp, succinct(&update_validator_set_event.cause_block))
        };
        Box::new(logger)
    }
}

impl Logger for ProposeEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |propose_event: &ProposeEvent| {
            log::info!("{}, {:?}, {}, {}, {}", PROPOSE, propose_event.timestamp, succinct(&propose_event.proposal.block.hash), propose_event.proposal.block.height, propose_event.proposal.view)
        };
        Box::new(logger)
    }
}

impl Logger for NudgeEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |nudge_event: &NudgeEvent| {
            log::info!(
                "{}, {:?}, {}, {}, {:?}",
                NUDGE,
                nudge_event.timestamp,
                succinct(&nudge_event.nudge.justify.block),
                nudge_event.nudge.view,
                nudge_event.nudge.justify.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for VoteEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |vote_event: &VoteEvent| {
            log::info!(
                "{}, {:?}, {}, {}, {:?}",
                VOTE,
                vote_event.timestamp,
                succinct(&vote_event.vote.block),
                vote_event.vote.view,
                vote_event.vote.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for NewViewEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |new_view_event: &NewViewEvent| {
            log::info!(
                "{}, {:?}, {}, {}, {:?}",
                VIEW_TIME_OUT,
                new_view_event.timestamp,
                succinct(&new_view_event.new_view.highest_qc.block),
                new_view_event.new_view.view,
                new_view_event.new_view.highest_qc.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveProposalEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_proposal_event: &ReceiveProposalEvent| {
            log::debug!(
                "{}, {:?}, {}, {}, {}",
                RECEIVED_PROPOSAL,
                receive_proposal_event.timestamp,
                succinct(&receive_proposal_event.origin.to_bytes()),
                succinct(&receive_proposal_event.proposal.block.hash),
                receive_proposal_event.proposal.block.height
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveNudgeEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_nudge_event: &ReceiveNudgeEvent| {
            log::debug!(
                "{}, {:?}, {}, {}, {:?}",
                RECEIVED_NUDGE,
                receive_nudge_event.timestamp,
                succinct(&receive_nudge_event.origin.to_bytes()),
                succinct(&receive_nudge_event.nudge.justify.block),
                receive_nudge_event.nudge.justify.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveVoteEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_vote_event: &ReceiveVoteEvent| {
            log::debug!(
                "{}, {:?}, {}, {}, {:?}",
                RECEIVED_VOTE,
                receive_vote_event.timestamp,
                succinct(&receive_vote_event.origin.to_bytes()),
                succinct(&receive_vote_event.vote.block),
                receive_vote_event.vote.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveNewViewEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_new_view_event: &ReceiveNewViewEvent| {
            log::debug!(
                "{}, {:?}, {}, {}, {}, {:?}",
                RECEIVED_NEW_VIEW,
                receive_new_view_event.timestamp,
                succinct(&receive_new_view_event.origin.to_bytes()),
                succinct(&receive_new_view_event.new_view.highest_qc.block),
                receive_new_view_event.new_view.view,
                receive_new_view_event.new_view.highest_qc.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for StartViewEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |start_view_event: &StartViewEvent| {
            log::debug!(
                "{}, {:?}, {}, {}",
                START_VIEW,
                start_view_event.timestamp,
                succinct(&start_view_event.leader.to_bytes()),
                start_view_event.view
            )
        };
        Box::new(logger)
    }
}

impl Logger for ViewTimeoutEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |view_timeout_event: &ViewTimeoutEvent| {
            log::info!(
                "{}, {:?}, {}, {:?}",
                VIEW_TIME_OUT,
                view_timeout_event.timestamp,
                view_timeout_event.view,
                view_timeout_event.timeout,
            )
        };
        Box::new(logger)
    }
}

impl Logger for CollectQCEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |collect_qc_event: &CollectQCEvent| {
            log::info!(
                "{}, {:?}, {}, {}, {:?}",
                COLLECT_QC,
                collect_qc_event.timestamp,
                succinct(&collect_qc_event.quorum_certificate.block),
                collect_qc_event.quorum_certificate.view,
                collect_qc_event.quorum_certificate.phase,
            )
        };
        Box::new(logger)
    }
}

impl Logger for StartSyncEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |start_sync_event: &StartSyncEvent| {
            log::info!("{}, {:?}, {}", START_SYNC, start_sync_event.timestamp, succinct(&start_sync_event.peer.to_bytes()))
        };
        Box::new(logger)
    }
}

impl Logger for EndSyncEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |end_sync_event: &EndSyncEvent| {
            log::info!("{}, {:?}, {}, {}", END_SYNC, end_sync_event.timestamp, succinct(&end_sync_event.peer.to_bytes()), end_sync_event.blocks_synced)
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveSyncRequestEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_sync_request_event: &ReceiveSyncRequestEvent| {
            log::info!(
                "{}, {:?}, {}, {}, {}",
                RECEIVE_SYNC_REQUEST,
                receive_sync_request_event.timestamp,
                succinct(&receive_sync_request_event.peer.to_bytes()),
                receive_sync_request_event.start_height,
                receive_sync_request_event.limit
            )  
        };
        Box::new(logger)
    }
}

impl Logger for SendSyncResponseEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |send_sync_response_event: &SendSyncResponseEvent| {
            log::info!(
                "{}, {:?}, {}, {}, {}",
                SEND_SYNC_RESPONSE,
                send_sync_response_event.timestamp,
                succinct(&send_sync_response_event.peer.to_bytes()),
                succinct(&send_sync_response_event.highest_qc.block),
                send_sync_response_event.blocks.len(),
            )  
        };
        Box::new(logger)
    }
}

// Get a more readable representation of a bytesequence by base64-encoding it and taking the first 7 characters.
pub(crate) fn succinct(bytes: &[u8]) -> String {
    let encoded = STANDARD_NO_PAD.encode(bytes);
    if encoded.len() > 7 {
        let mut truncated = encoded[0..7].to_string();
        truncated.push_str("..");
        truncated
    } else {
        encoded
    }
}
