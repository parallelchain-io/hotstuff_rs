/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! Functions for logging out library events of varying levels of importance.


use log;
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use crate::types::*;

pub mod info {
    use super::*;

    pub const Proposed: &str = "Proposed";
    pub const NUDGING: &str = "Nudged";
    pub const VOTED: &str = "Voted";
    pub const COMMITTING: &str = "Committing";

    pub(crate) fn committing(block: &CryptoHash, height: BlockHeight) {
        log::info!("{}, {}, {}", COMMITTING, succinct(block), height);
    }

    pub(crate) fn proposed(block: &CryptoHash, height: BlockHeight) {
        log::info!("{}, {}, {}", Proposed, succinct(block), height);
    }

    pub(crate) fn nudged(block: &CryptoHash, height: BlockHeight, justify_phase: Phase) {
        log::info!("{}, {}, {}, {:?}", NUDGING, succinct(block), height, justify_phase);
    }

    pub(crate) fn voted(block: &CryptoHash, height: BlockHeight, phase: Phase) {
        log::info!("{}, {}, {}, {:?}", VOTED, succinct(block), height, phase);
    }

}

pub mod debug {
    use super::*;

    pub const ENTERED_VIEW: &str = "EnteredView";
    pub const RECEIVED_PROPOSAL: &str = "ReceivedProposal";
    pub const RECEIVED_NUDGE: &str = "ReceivedNudge";
    pub const RECEIVED_VOTE: &str = "ReceivedVote";
    pub const RECEIVED_NEW_VIEW: &str = "ReceivedNewView";
    pub const COLLECTED_QC: &str = "CollectedQc";
    pub const UPDATING_VALIDATOR_SET: &str = "UpdatingValidatorSet";
    pub const REPLACING_HIGHEST_QC: &str = "ReplacingHighestQc";

    pub(crate) fn entered_view(view: ViewNumber) {
        log::debug!("{}, {}", ENTERED_VIEW, view);
    } 

    pub(crate) fn received_proposal(origin: &PublicKeyBytes, block: &CryptoHash, height: BlockHeight) {
        log::debug!("{}, {}, {}, {}", RECEIVED_PROPOSAL, succinct(origin), succinct(block), height);
    }

    pub(crate) fn received_nudge(origin: &PublicKeyBytes, justify_block: &CryptoHash, justify_phase: Phase) {
        log::debug!("{}, {}, {}, {:?}", RECEIVED_NUDGE, succinct(origin), succinct(justify_block), justify_phase);
    }

    pub(crate) fn received_vote(origin: &PublicKeyBytes, block: &CryptoHash, phase: Phase) {
        log::debug!("{}, {}, {}, {:?}", RECEIVED_VOTE, succinct(origin), succinct(block), phase);
    }

    pub(crate) fn received_new_view(origin: &PublicKeyBytes, view: ViewNumber, block: &CryptoHash, phase: Phase) {
        log::debug!("{}, {}, {}, {}, {:?}", RECEIVED_NEW_VIEW, succinct(origin), view, succinct(block), phase);
    }

    pub(crate) fn collected_qc(block: &CryptoHash, phase: Phase) {
        log::debug!("{}, {}, {:?}", COLLECTED_QC, succinct(block), phase);
    }

    pub(crate) fn updating_validator_set(cause_block: &CryptoHash) {
        log::debug!("{}, {}", UPDATING_VALIDATOR_SET, succinct(cause_block));
    }

    pub(crate) fn replacing_highest_qc(block: &CryptoHash, phase: Phase) {
        log::debug!("{}, {}, {:?}", REPLACING_HIGHEST_QC, succinct(block), phase);
    }


}

// Get a more readable representation of a bytesequence by base64-encoding it and taking the first 7 characters. 
fn succinct(bytes: &[u8]) -> String {
    let encoded = STANDARD_NO_PAD.encode(bytes);
    let mut truncated = encoded[0..7].to_string();
    truncated.push_str("..");

    truncated
}
