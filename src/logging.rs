/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Functions that log out events.
//!
//! The logs defined in this module are printed if the user enabled them via replica's
//! [config](crate::replica::Configuration).
//!
//! HotStuff-rs logs using the [log](https://docs.rs/log/latest/log/) crate. To get these messages
//! printed onto a terminal or to a file, set up a
//! [logging implementation](https://docs.rs/log/latest/log/#available-logging-implementations).
//!
//! ## Log message format
//!
//! Log messages are CSVs (Comma Separated Values) with at least two values. The first two values are
//! always:
//! 1. The name of the [event](crate::events) in PascalCase (defined in this module as constants).
//! 2. The time the event was emitted (as number of seconds since the Unix Epoch).
//!
//! The rest of the values differ depending on the kind of event. For example, the following snippet
//! is how a [ReceiveProposal](crate::events::ReceiveProposalEvent) is printed:
//!
//! ```text
//! ReceiveProposal, 1701329264, Id5u7f6, fNGCJyk, 0
//! ```
//!
//! In the snippet:
//! - The third value is the first seven characters of the Base64 encoding of the public address of the
//!   origin of the proposal.
//! - The fourth value is the first seven characters of the Base64 encoding of the hash of the proposed
//!   block.
//! - The fifth value is the height of the proposed block.

use crate::{events::*, pacemaker::messages::ProgressCertificate};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use log;
use std::time::SystemTime;

// Names of each event in PascalCase for printing:
pub const INSERT_BLOCK: &str = "InsertBlock";
pub const COMMIT_BLOCK: &str = "CommitBlock";
pub const PRUNE_BLOCK: &str = "PruneBlock";
pub const UPDATE_HIGHEST_PC: &str = "UpdateHighestPC";
pub const UPDATE_LOCKED_PC: &str = "UpdateLockedPC";
pub const UPDATE_HIGHEST_TC: &str = "UpdateHighestTC";
pub const UPDATE_VALIDATOR_SET: &str = "UpdateValidatorSet";

pub const PROPOSE: &str = "Propose";
pub const NUDGE: &str = "Nudge";
pub const PHASE_VOTE: &str = "PhaseVote";
pub const NEW_VIEW: &str = "NewView";
pub const TIMEOUT_VOTE: &str = "TimeoutVote";
pub const ADVANCE_VIEW: &str = "AdvanceView";

pub const RECEIVE_PROPOSAL: &str = "ReceiveProposal";
pub const RECEIVE_NUDGE: &str = "ReceiveNudge";
pub const RECEIVE_PHASE_VOTE: &str = "ReceivePhaseVote";
pub const RECEIVE_NEW_VIEW: &str = "ReceiveNewView";
pub const RECEIVE_TIMEOUT_VOTE: &str = "ReceiveTimeoutVote";
pub const RECEIVE_ADVANCE_VIEW: &str = "ReceiveAdvanceView";

pub const START_VIEW: &str = "StartView";
pub const VIEW_TIMEOUT: &str = "ViewTimeout";
pub const COLLECT_PC: &str = "CollectPC";
pub const COLLECT_TC: &str = "CollectTC";

pub const START_SYNC: &str = "StartSync";
pub const END_SYNC: &str = "EndSync";
pub const RECEIVE_SYNC_REQUEST: &str = "ReceiveSyncRequest";
pub const SEND_SYNC_RESPONSE: &str = "SendSyncResponse";

/// Implemented by event types. Used to get a closure that logs the event.
pub(crate) trait Logger {
    /// Returns a pointer to the default logging handler for a given event type.
    fn get_logger() -> Box<dyn Fn(&Self) + Send>;
}

impl Logger for InsertBlockEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |insert_block_event: &InsertBlockEvent| {
            log::info!(
                "{}, {}, {}, {}",
                INSERT_BLOCK,
                secs_since_unix_epoch(insert_block_event.timestamp),
                first_seven_base64_chars(&insert_block_event.block.hash.bytes()),
                insert_block_event.block.height
            )
        };
        Box::new(logger)
    }
}

impl Logger for CommitBlockEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |commit_block_event: &CommitBlockEvent| {
            log::info!(
                "{}, {}, {}",
                COMMIT_BLOCK,
                secs_since_unix_epoch(commit_block_event.timestamp),
                first_seven_base64_chars(&commit_block_event.block.bytes())
            )
        };
        Box::new(logger)
    }
}

impl Logger for PruneBlockEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |prune_block_event: &PruneBlockEvent| {
            log::info!(
                "{}, {}, {}",
                PRUNE_BLOCK,
                secs_since_unix_epoch(prune_block_event.timestamp),
                first_seven_base64_chars(&prune_block_event.block.bytes())
            )
        };
        Box::new(logger)
    }
}

impl Logger for UpdateHighestPCEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |update_highest_pc_event: &UpdateHighestPCEvent| {
            log::info!(
                "{}, {}, {}, {}, {:?}",
                UPDATE_HIGHEST_PC,
                secs_since_unix_epoch(update_highest_pc_event.timestamp),
                first_seven_base64_chars(&update_highest_pc_event.highest_pc.block.bytes()),
                update_highest_pc_event.highest_pc.view,
                update_highest_pc_event.highest_pc.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for UpdateLockedPCEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |update_locked_pc_event: &UpdateLockedPCEvent| {
            log::info!(
                "{}, {}, {}, {}, {:?}",
                UPDATE_LOCKED_PC,
                secs_since_unix_epoch(update_locked_pc_event.timestamp),
                first_seven_base64_chars(&update_locked_pc_event.locked_pc.block.bytes()),
                update_locked_pc_event.locked_pc.view,
                update_locked_pc_event.locked_pc.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for UpdateHighestTCEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |update_highest_tc_event: &UpdateHighestTCEvent| {
            log::info!(
                "{}, {}, {}",
                UPDATE_HIGHEST_TC,
                secs_since_unix_epoch(update_highest_tc_event.timestamp),
                update_highest_tc_event.highest_tc.view,
            )
        };
        Box::new(logger)
    }
}

impl Logger for UpdateValidatorSetEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |update_validator_set_event: &UpdateValidatorSetEvent| {
            log::info!(
                "{}, {}, {}",
                UPDATE_VALIDATOR_SET,
                secs_since_unix_epoch(update_validator_set_event.timestamp),
                first_seven_base64_chars(&update_validator_set_event.cause_block.bytes())
            )
        };
        Box::new(logger)
    }
}

impl Logger for ProposeEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |propose_event: &ProposeEvent| {
            log::info!(
                "{}, {}, {}, {}, {}",
                PROPOSE,
                secs_since_unix_epoch(propose_event.timestamp),
                first_seven_base64_chars(&propose_event.proposal.block.hash.bytes()),
                propose_event.proposal.block.height,
                propose_event.proposal.view
            )
        };
        Box::new(logger)
    }
}

impl Logger for NudgeEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |nudge_event: &NudgeEvent| {
            log::info!(
                "{}, {}, {}, {}, {:?}",
                NUDGE,
                secs_since_unix_epoch(nudge_event.timestamp),
                first_seven_base64_chars(&nudge_event.nudge.justify.block.bytes()),
                nudge_event.nudge.view,
                nudge_event.nudge.justify.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for PhaseVoteEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |phase_vote_event: &PhaseVoteEvent| {
            log::info!(
                "{}, {}, {}, {}, {:?}",
                PHASE_VOTE,
                secs_since_unix_epoch(phase_vote_event.timestamp),
                first_seven_base64_chars(&phase_vote_event.vote.block.bytes()),
                phase_vote_event.vote.view,
                phase_vote_event.vote.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for NewViewEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |new_view_event: &NewViewEvent| {
            log::info!(
                "{}, {}, {}, {}, {:?}",
                NEW_VIEW,
                secs_since_unix_epoch(new_view_event.timestamp),
                first_seven_base64_chars(&new_view_event.new_view.highest_pc.block.bytes()),
                new_view_event.new_view.view,
                new_view_event.new_view.highest_pc.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for TimeoutVoteEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |timeout_vote_event: &TimeoutVoteEvent| {
            log::info!(
                "{}, {}, {}",
                TIMEOUT_VOTE,
                secs_since_unix_epoch(timeout_vote_event.timestamp),
                timeout_vote_event.timeout_vote.view,
            )
        };
        Box::new(logger)
    }
}

impl Logger for AdvanceViewEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |advance_view_event: &AdvanceViewEvent| {
            log::info!(
                "{}, {}, {}",
                ADVANCE_VIEW,
                secs_since_unix_epoch(advance_view_event.timestamp),
                progress_certificate_info(&advance_view_event.advance_view.progress_certificate),
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveProposalEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_proposal_event: &ReceiveProposalEvent| {
            log::info!(
                "{}, {}, {}, {}, {}",
                RECEIVE_PROPOSAL,
                secs_since_unix_epoch(receive_proposal_event.timestamp),
                first_seven_base64_chars(&receive_proposal_event.origin.to_bytes()),
                first_seven_base64_chars(&receive_proposal_event.proposal.block.hash.bytes()),
                receive_proposal_event.proposal.block.height
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveNudgeEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_nudge_event: &ReceiveNudgeEvent| {
            log::info!(
                "{}, {}, {}, {}, {:?}",
                RECEIVE_NUDGE,
                secs_since_unix_epoch(receive_nudge_event.timestamp),
                first_seven_base64_chars(&receive_nudge_event.origin.to_bytes()),
                first_seven_base64_chars(&receive_nudge_event.nudge.justify.block.bytes()),
                receive_nudge_event.nudge.justify.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceivePhaseVoteEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_vote_event: &ReceivePhaseVoteEvent| {
            log::info!(
                "{}, {}, {}, {}, {:?}",
                RECEIVE_PHASE_VOTE,
                secs_since_unix_epoch(receive_vote_event.timestamp),
                first_seven_base64_chars(&receive_vote_event.origin.to_bytes()),
                first_seven_base64_chars(&receive_vote_event.phase_vote.block.bytes()),
                receive_vote_event.phase_vote.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveNewViewEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_new_view_event: &ReceiveNewViewEvent| {
            log::info!(
                "{}, {}, {}, {}, {}, {:?}",
                RECEIVE_NEW_VIEW,
                secs_since_unix_epoch(receive_new_view_event.timestamp),
                first_seven_base64_chars(&receive_new_view_event.origin.to_bytes()),
                first_seven_base64_chars(&receive_new_view_event.new_view.highest_pc.block.bytes()),
                receive_new_view_event.new_view.view,
                receive_new_view_event.new_view.highest_pc.phase
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveTimeoutVoteEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_timeout_vote_event: &ReceiveTimeoutVoteEvent| {
            log::info!(
                "{}, {}, {}, {}",
                RECEIVE_TIMEOUT_VOTE,
                secs_since_unix_epoch(receive_timeout_vote_event.timestamp),
                first_seven_base64_chars(&receive_timeout_vote_event.origin.to_bytes()),
                receive_timeout_vote_event.timeout_vote.view,
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveAdvanceViewEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_advance_view_event: &ReceiveAdvanceViewEvent| {
            log::info!(
                "{}, {}, {}, {}",
                RECEIVE_ADVANCE_VIEW,
                secs_since_unix_epoch(receive_advance_view_event.timestamp),
                first_seven_base64_chars(&receive_advance_view_event.origin.to_bytes()),
                progress_certificate_info(
                    &receive_advance_view_event.advance_view.progress_certificate
                ),
            )
        };
        Box::new(logger)
    }
}

impl Logger for StartViewEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |start_view_event: &StartViewEvent| {
            log::info!(
                "{}, {}, {}",
                START_VIEW,
                secs_since_unix_epoch(start_view_event.timestamp),
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
                "{}, {}, {}",
                VIEW_TIMEOUT,
                secs_since_unix_epoch(view_timeout_event.timestamp),
                view_timeout_event.view,
            )
        };
        Box::new(logger)
    }
}

impl Logger for CollectPCEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |collect_pc_event: &CollectPCEvent| {
            log::info!(
                "{}, {}, {}, {}, {:?}",
                COLLECT_PC,
                secs_since_unix_epoch(collect_pc_event.timestamp),
                first_seven_base64_chars(&collect_pc_event.phase_certificate.block.bytes()),
                collect_pc_event.phase_certificate.view,
                collect_pc_event.phase_certificate.phase,
            )
        };
        Box::new(logger)
    }
}

impl Logger for CollectTCEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |collect_tc_event: &CollectTCEvent| {
            log::info!(
                "{}, {}, {}",
                COLLECT_TC,
                secs_since_unix_epoch(collect_tc_event.timestamp),
                collect_tc_event.timeout_certificate.view,
            )
        };
        Box::new(logger)
    }
}

impl Logger for StartSyncEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |start_sync_event: &StartSyncEvent| {
            log::info!(
                "{}, {}, {}",
                START_SYNC,
                secs_since_unix_epoch(start_sync_event.timestamp),
                first_seven_base64_chars(&start_sync_event.peer.to_bytes())
            )
        };
        Box::new(logger)
    }
}

impl Logger for EndSyncEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |end_sync_event: &EndSyncEvent| {
            log::info!(
                "{}, {}, {}, {}",
                END_SYNC,
                secs_since_unix_epoch(end_sync_event.timestamp),
                first_seven_base64_chars(&end_sync_event.peer.to_bytes()),
                end_sync_event.blocks_synced
            )
        };
        Box::new(logger)
    }
}

impl Logger for ReceiveSyncRequestEvent {
    fn get_logger() -> Box<dyn Fn(&Self) + Send> {
        let logger = |receive_sync_request_event: &ReceiveSyncRequestEvent| {
            log::info!(
                "{}, {}, {}, {}, {}",
                RECEIVE_SYNC_REQUEST,
                secs_since_unix_epoch(receive_sync_request_event.timestamp),
                first_seven_base64_chars(&receive_sync_request_event.peer.to_bytes()),
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
                "{}, {}, {}, {}, {}",
                SEND_SYNC_RESPONSE,
                secs_since_unix_epoch(send_sync_response_event.timestamp),
                first_seven_base64_chars(&send_sync_response_event.peer.to_bytes()),
                first_seven_base64_chars(&send_sync_response_event.highest_pc.block.bytes()),
                send_sync_response_event.blocks.len(),
            )
        };
        Box::new(logger)
    }
}

// Get a more readable representation of a bytesequence by base64-encoding it and taking the first 7 characters.
fn first_seven_base64_chars(bytes: &[u8]) -> String {
    let encoded = STANDARD_NO_PAD.encode(bytes);
    if encoded.len() > 7 {
        encoded[0..7].to_string()
    } else {
        encoded
    }
}

fn secs_since_unix_epoch(timestamp: SystemTime) -> u64 {
    timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Event occured before the Unix Epoch.")
        .as_secs()
}

fn progress_certificate_info(certificate: &ProgressCertificate) -> String {
    match certificate {
        ProgressCertificate::PhaseCertificate(pc) => String::from(format!(
            "Phase Certificate, view: {}, phase: {:?}, block: {}, no. of signatures: {}",
            pc.view,
            pc.phase,
            first_seven_base64_chars(&pc.block.bytes()),
            pc.signatures.iter().filter(|sig| sig.is_some()).count()
        )),
        ProgressCertificate::TimeoutCertificate(tc) => {
            String::from(format!("Timeout Certificate, view: {}", tc.view))
        }
    }
}
