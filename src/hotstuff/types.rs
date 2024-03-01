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
use super::messages::BlockVote;

/// Helps leaders incrementally form [QuorumCertificate]s by combining votes for the same chain_id, view, block, and phase by replicas
/// in a given [validator set](ValidatorSet).
pub(crate) struct BlockVoteCollector {
    chain_id: ChainID,
    view: ViewNumber,
    validator_set: ValidatorSet,
    validator_set_total_power: TotalPower,
    signature_sets: HashMap<(CryptoHash, Phase), (SignatureSet, TotalPower)>,
}

impl VoteCollector<BlockVote, QuorumCertificate> for BlockVoteCollector {
    fn new(
        chain_id: ChainID,
        view: ViewNumber,
        validator_set: ValidatorSet,
    ) -> Self {
        Self {
            chain_id,
            view,
            validator_set_total_power: validator_set.total_power(),
            validator_set,
            signature_sets: HashMap::new(),
        }
    }

    /// Adds the vote to a signature set for the specified view, block, and phase. Returning a quorum certificate
    /// if adding the vote allows for one to be created.
    ///
    /// If the vote is not signed correctly, or doesn't match the collector's view, or the signer is not part
    /// of its validator set, then this is a no-op.
    ///
    /// # Preconditions
    /// vote.is_correct(signer)
    ///
    /// # Panics
    /// vote.chain_id and vote.view must be the same as the chain_id and the view used to create this VoteCollector.
    fn collect(
        &mut self,
        signer: &VerifyingKey,
        vote: BlockVote,
    ) -> Option<QuorumCertificate> {
        if self.chain_id != vote.chain_id || self.view != vote.view {
            panic!()
        }

        // Check if the signer is actually in the validator set.
        if let Some(pos) = self.validator_set.position(signer) {
            // If the vote is for a new (block, phase) pair, prepare an empty signature set.
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.signature_sets.entry((vote.block, vote.phase))
            {
                e.insert((vec![None; self.validator_set.len()], 0));
            }

            let (signature_set, signature_set_power) = self
                .signature_sets
                .get_mut(&(vote.block, vote.phase))
                .unwrap();

            // If a vote for the (block, phase) from the signer hasn't been collected before, insert it into the signature set.
            if signature_set[pos].is_none() {
                signature_set[pos] = Some(vote.signature);
                *signature_set_power += *self.validator_set.power(signer).unwrap() as u128;

                // If inserting the vote makes the signature set form a quorum, then create a quorum certificate.
                if *signature_set_power >= QuorumCertificate::quorum(self.validator_set_total_power) {
                    let (signatures, _) = self
                        .signature_sets
                        .remove(&(vote.block, vote.phase))
                        .unwrap();
                    let collected_qc = QuorumCertificate {
                        chain_id: self.chain_id,
                        view: self.view,
                        block: vote.block,
                        phase: vote.phase,
                        signatures,
                    };

                    return Some(collected_qc);
                }
            }
        }

        None
    }
}

pub(crate) struct NewViewCollector {
    validator_set: ValidatorSet,
    total_power: TotalPower,
    collected_from: HashSet<VerifyingKey>,
    accumulated_power: TotalPower,
}

impl NewViewCollector {
    pub(crate) fn new(validator_set: ValidatorSet) -> NewViewCollector {
        Self {
            total_power: validator_set.total_power(),
            validator_set,
            collected_from: HashSet::new(),
            accumulated_power: 0,
        }
    }

    /// Notes that we have collected a new view message from the specified replica in the given view. Then, returns whether
    /// by collecting this message we have collected new view messages from a quorum of validators in this view. If the sender
    /// is not part of the validator set, then this function does nothing and returns false.
    pub(crate) fn collect(&mut self, sender: &VerifyingKey) -> bool {
        if !self.validator_set.contains(sender) {
            return false;
        }

        if !self.collected_from.contains(sender) {
            self.collected_from.insert(*sender);
            self.accumulated_power += *self.validator_set.power(sender).unwrap() as u128;
        }

        self.accumulated_power >= QuorumCertificate::quorum(self.total_power)
    }
}