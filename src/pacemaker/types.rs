/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions of types specific to the [HotStuff] protocol.

use std::collections::{HashMap, HashSet};

use crate::types::{
    basic::*,
    validators::*,
    certificates::*,
    voting::*,
};
use crate::pacemaker::messages::TimeoutVote;

/// Helps leaders incrementally form [TimeoutCertificate]s by combining votes for the same chain_id and view by replicas
/// in a given [validator set](ValidatorSet).
pub(crate) struct TimeoutVoteCollector {
    chain_id: ChainID,
    view: ViewNumber,
    validator_set: ValidatorSet,
    validator_set_total_power: TotalPower,
    signature_set: SignatureSet,
    signature_set_power: TotalPower,
}

impl VoteCollector<TimeoutVote, TimeoutCertificate> for TimeoutVoteCollector {
    
    fn new(chain_id: ChainID, view: ViewNumber, validator_set: ValidatorSet) -> Self {
        Self { 
            chain_id, 
            view, 
            validator_set, 
            validator_set_total_power: validator_set.total_power(), 
            signature_set: vec![None; validator_set.len()], 
            signature_set_power: 0
        }
    }

    fn collect(&mut self, signer: &VerifyingKey, vote: TimeoutVote) -> Option<TimeoutCertificate> {
        
        if self.chain_id != vote.chain_id || self.view != vote.view {
            return None
        }

        // Check if the signer is actually in the validator set.
        if let Some(pos) = self.validator_set.position(signer) {

            // If the vote has not been collected before, insert its signature into the signature set.
            if self.signature_set[pos].is_none() {
                self.signature_set[pos] = Some(vote.signature);
                *self.signature_set_power += *self.validator_set.power(signer).unwrap() as u128;

                // If inserting the vote makes the signature set form a quorum, then create a quorum certificate.
                if *self.signature_set_power >= TimeoutCertificate::quorum(self.validator_set_total_power) {
                    let collected_tc = TimeoutCertificate {
                        chain_id: self.chain_id,
                        view: self.view,
                        signatures: self.signature_set.clone(),
                    };

                    return Some(collected_tc);
                }
            }
        }

        None

    }
}