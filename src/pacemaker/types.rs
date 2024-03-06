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
    collectors::*,
};
use crate::pacemaker::messages::TimeoutVote;

/// Helps leaders incrementally form [TimeoutCertificate]s by combining votes for the same chain_id and view by replicas
/// in a given [validator set](ValidatorSet).
pub(crate) struct TimeoutVoteCollector {
    chain_id: ChainID,
    view: ViewNumber,
    validator_set_total_power: TotalPower,
    validator_set: ValidatorSet,
    signature_set_power: TotalPower,
    signature_set: SignatureSet,
}

impl Collector<TimeoutVote, TimeoutCertificate> for TimeoutVoteCollector {
    
    fn new(chain_id: ChainID, view: ViewNumber, validator_set: ValidatorSet) -> Self {
        let n = validator_set.len();
        Self { 
            chain_id, 
            view, 
            validator_set_total_power: validator_set.total_power(), 
            validator_set, 
            signature_set_power: 0,
            signature_set: vec![None; n], 
        }
    }

    /// Adds the timeout vote to a signature set if it has the correct view and chain id. 
    /// Returning a quorum certificate if adding the vote allows for one to be created.
    ///
    /// If the timeout vote is not signed correctly, or doesn't match the collector's view, 
    /// or the signer is not part of its validator set, then this is a no-op.
    ///
    /// # Preconditions
    /// vote.is_correct(signer)
    fn collect(&mut self, signer: &VerifyingKey, vote: TimeoutVote) -> Option<TimeoutCertificate> {
        
        if self.chain_id != vote.chain_id || self.view != vote.view {
            return None
        }

        // Check if the signer is actually in the validator set.
        if let Some(pos) = self.validator_set.position(signer) {

            // If the vote has not been collected before, insert its signature into the signature set.
            if self.signature_set[pos].is_none() {
                self.signature_set[pos] = Some(vote.signature);
                self.signature_set_power += *self.validator_set.power(signer).unwrap() as u128;

                // If inserting the vote makes the signature set form a quorum, then create a quorum certificate.
                if self.signature_set_power >= TimeoutCertificate::quorum(self.validator_set_total_power) {
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