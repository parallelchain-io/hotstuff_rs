/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions of types specific to the [HotStuff][crate::hotstuff::protocol::HotStuff] protocol.

use std::collections::{HashMap, HashSet};

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::Verifier;

use crate::types::{
    basic::*,
    validators::*,
};
use super::messages::Vote;

/// Proof that at least a quorum of validators have voted for a given [proposal][crate::hotstuff::messages::Proposal] or [nudge][crate::hotstuff::messages::Nudge].
/// Required for extending a block in the [HotStuff][crate::hotstuff::protocol::HotStuff], and for optimistic advance to a new view as part of the 
/// [pacemaker][crate::pacemaker::protocol::Pacemaker] protocol.
#[derive(Clone, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
pub struct QuorumCertificate {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub block: CryptoHash,
    pub phase: Phase,
    pub signatures: SignatureSet,
}

impl QuorumCertificate {
    /// Checks if all of the signatures in the certificate are correct, and if the set of signatures forms a quorum.
    ///
    /// A special case is if the qc is the genesis qc, in which case it is automatically correct.
    pub(crate) fn is_correct(&self, validator_set: &ValidatorSet) -> bool {
        if self.is_genesis_qc() {
            true
        } else {
            // Check whether the size of the signature set is the same as the size of the validator set.
            if self.signatures.len() != validator_set.len() {
                return false;
            }

            // Check whether every signature is correct and tally up their powers.
            let mut total_power: TotalPower = TotalPower::new(0);
            for (signature, (signer, power)) in self
                .signatures
                .iter()
                .zip(validator_set.validators_and_powers())
            {
                if let Some(signature) = signature {
                    if let Ok(signature) = Signature::from_slice(&signature.bytes()) {
                        if signer
                            .verify(
                                &(self.chain_id, self.view, self.block, self.phase)
                                    .try_to_vec()
                                    .unwrap(),
                                &signature,
                            )
                            .is_ok()
                        {
                            total_power += power;
                        } else {
                            // qc contains incorrect signature.
                            return false;
                        }
                    } else {
                        // qc contains incorrect signature.
                        return false;
                    }
                }
            }

            // Check if the signatures form a quorum.
            total_power >= validator_set.quorum()
        }
    }

    pub const fn genesis_qc() -> QuorumCertificate {
        QuorumCertificate {
            chain_id: ChainID::new(0),
            view: ViewNumber::init(),
            block: CryptoHash::new([0u8; 32]),
            phase: Phase::Generic,
            signatures: SignatureSet::init(),
        }
    }

    pub fn is_genesis_qc(&self) -> bool {
        *self == Self::genesis_qc()
    }

}

#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize, Debug)]
pub enum Phase {
    // ↓↓↓ For pipelined flow ↓↓↓ //
    Generic,

    // ↓↓↓ For phased flow ↓↓↓ //
    Prepare,

    // The inner view number is the view number of the *prepare* qc contained in the nudge which triggered the
    // vote containing this phase.
    Precommit(ViewNumber),

    // The inner view number is the view number of the *precommit* qc contained in the nudge which triggered the
    // vote containing this phase.
    Commit(ViewNumber),

    //TODO
    //Decide(ViewNumber)
}

impl Phase {
    pub fn is_generic(self) -> bool {
        self == Phase::Generic
    }

    pub fn is_prepare(self) -> bool {
        self == Phase::Prepare
    }

    pub fn is_precommit(self) -> bool {
        matches!(self, Phase::Precommit(_))
    }

    pub fn is_commit(self) -> bool {
        matches!(self, Phase::Commit(_))
    }

    //TODO
    //is_decide
}

/// Serves to incrementally form a [QuorumCertificate] by combining votes for the same chain id, view, block, and phase by replicas
/// from a given [validator set](ValidatorSet).
pub(crate) struct VoteCollector {
    chain_id: ChainID,
    view: ViewNumber,
    validator_set: ValidatorSet,
    validator_set_total_power: TotalPower,
    signature_sets: HashMap<(CryptoHash, Phase), (SignatureSet, TotalPower)>,
}

impl VoteCollector {
    pub(crate) fn new(
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
    pub(crate) fn collect(
        &mut self,
        signer: &VerifyingKey,
        vote: Vote,
    ) -> Option<QuorumCertificate> {
        if self.chain_id != vote.chain_id || self.view != vote.view {
            return None
        }

        // Check if the signer is actually in the validator set.
        if let Some(pos) = self.validator_set.position(signer) {
            // If the vote is for a new (block, phase) pair, prepare an empty signature set.
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.signature_sets.entry((vote.block, vote.phase))
            {
                e.insert((SignatureSet::new(self.validator_set.len()), TotalPower::new(0)));
            }

            let (signature_set, signature_set_power) = self
                .signature_sets
                .get_mut(&(vote.block, vote.phase))
                .unwrap();

            // If a vote for the (block, phase) from the signer hasn't been collected before, insert it into the signature set.
            if signature_set.get(pos).is_none() {
                signature_set.set(pos, Some(vote.signature));
                *signature_set_power += *self.validator_set.power(signer).unwrap();

                // If inserting the vote makes the signature set form a quorum, then create a quorum certificate.
                if *signature_set_power >= self.validator_set.quorum() {
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

/// Keeps track of the validators that have sent a [NewView][crate::hotstuff::messages::NewView]
/// message for a given view.
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
            accumulated_power: TotalPower::new(0),
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
            self.accumulated_power += *self.validator_set.power(sender).unwrap();
        }

        self.accumulated_power >= self.validator_set.quorum()
    }
}