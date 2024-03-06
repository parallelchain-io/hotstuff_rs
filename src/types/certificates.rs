/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definition of the [Certificate] trait which specifies the minimal signature for types that serve as evidence that a quorum of validators supports a given action.
//! Also defines:
//! 1. The [QuorumCertificate] type which stores the validators' votes for a [block][crate::types::block::Block] and implements the [Certificate] trait.
//! 2. The [TimeoutCertificate] type which stores the validators' votes in favour of advancing to a new view (intended for use only in the last view of an epoch).

use crate::types::basic::*;
use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::Verifier;

use super::validators::ValidatorSet;

/// Certificates serve as a proof that a [quorum][Certificate::quorum] of validators has done something, e.g., voted for a proposal.
/// The correctness of a certificate can be validated with a [Certificate::is_correct] method given the validator set.
pub trait Certificate {

    fn is_correct(&self, validator_set: &ValidatorSet) -> bool;

    fn quorum(validator_set_power: TotalPower) -> TotalPower {
        const TOTAL_POWER_OVERFLOW: &str = "Validator set power exceeds u128::MAX/2. Read the itemdoc for Validator Set.";

        (validator_set_power
            .checked_mul(2)
            .expect(TOTAL_POWER_OVERFLOW)
            / 3)
            + 1
    }
    
}

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

impl Certificate for QuorumCertificate {
    /// Checks if all of the signatures in the certificate are correct, and if the set of signatures forms a quorum.
    ///
    /// A special case is if the qc is the genesis qc, in which case it is automatically correct.
    fn is_correct(&self, validator_set: &ValidatorSet) -> bool {
        if self.is_genesis_qc() {
            true
        } else {
            // Check whether the size of the signature set is the same as the size of the validator set.
            if self.signatures.len() != validator_set.len() {
                return false;
            }

            // Check whether every signature is correct and tally up their powers.
            let mut total_power: TotalPower = 0;
            for (signature, (signer, power)) in self
                .signatures
                .iter()
                .zip(validator_set.validators_and_powers())
            {
                if let Some(signature) = signature {
                    if let Ok(signature) = Signature::from_slice(signature) {
                        if signer
                            .verify(
                                &(self.chain_id, self.view, self.block, self.phase)
                                    .try_to_vec()
                                    .unwrap(),
                                &signature,
                            )
                            .is_ok()
                        {
                            total_power += power as u128;
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
            let quorum = Self::quorum(validator_set.total_power());
            total_power >= quorum
        }
    }

}

impl QuorumCertificate {

    pub const fn genesis_qc() -> QuorumCertificate {
        QuorumCertificate {
            chain_id: 0,
            view: 0,
            block: [0u8; 32],
            phase: Phase::Generic,
            signatures: SignatureSet::new(),
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

/// Proof that at least a quorum of validators have sent a [TimeoutVote][crate::pacemaker::messages::TimeoutVote] for the same view.
/// Required for advancing to a new epoch as part of the [pacemaker][crate::pacemaker::protocol::Pacemaker] protocol.
#[derive(Clone, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
pub struct TimeoutCertificate {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub signatures: SignatureSet,
}

impl Certificate for TimeoutCertificate {

    /// Checks if all of the signatures in the certificate are correct, and if the set of signatures forms a quorum.
    fn is_correct(&self, validator_set: &ValidatorSet) -> bool {

            // Check whether the size of the signature set is the same as the size of the validator set.
            if self.signatures.len() != validator_set.len() {
                return false;
            }

            // Check whether every signature is correct and tally up their powers.
            let mut total_power: TotalPower = 0;
            for (signature, (signer, power)) in self
                .signatures
                .iter()
                .zip(validator_set.validators_and_powers())
            {
                if let Some(signature) = signature {
                    if let Ok(signature) = Signature::from_slice(signature) {
                        if signer
                            .verify(
                                &(self.chain_id, self.view)
                                    .try_to_vec()
                                    .unwrap(),
                                &signature,
                            )
                            .is_ok()
                        {
                            total_power += power as u128;
                        } else {
                            // tc contains incorrect signature.
                            return false;
                        }
                    } else {
                        // tc contains incorrect signature.
                        return false;
                    }
                }
            }

            // Check if the signatures form a quorum.
            let quorum = Self::quorum(validator_set.total_power());
            total_power >= quorum
    
    }

}