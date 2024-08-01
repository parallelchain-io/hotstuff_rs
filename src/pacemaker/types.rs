/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions of types specific to the Pacemaker protocol.

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::Verifier;

use crate::pacemaker::messages::TimeoutVote;
use crate::state::block_tree::{BlockTree, BlockTreeError};
use crate::state::kv_store::KVStore;
use crate::types::collectors::{Certificate, Collector};
use crate::types::{basic::*, validators::*};

/// Proof that at least a quorum of validators have sent a
/// [`TimeoutVote`][crate::pacemaker::messages::TimeoutVote] for the same view.
/// Required for advancing to a new epoch as part of the
/// [pacemaker][crate::pacemaker::protocol::Pacemaker] protocol.
#[derive(Clone, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
pub struct TimeoutCertificate {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub signatures: SignatureSet,
}

impl Certificate for TimeoutCertificate {
    /// Checks if the signatures in the TC are correct and form a quorum for an appropriate validator set.
    ///
    /// During the speculation phase, i.e., when the new validator set has been committed, but the old
    /// validator set is still active, a TC is correct if it is correctly signed by a quorum from either
    /// of the two validator sets.
    fn is_correct<K: KVStore>(&self, block_tree: &BlockTree<K>) -> Result<bool, BlockTreeError> {
        let validator_set_state = block_tree.validator_set_state()?;
        if validator_set_state.update_decided() {
            Ok(self.is_correctly_signed(validator_set_state.committed_validator_set()))
        } else {
            Ok(
                self.is_correctly_signed(validator_set_state.committed_validator_set())
                    || self.is_correctly_signed(validator_set_state.previous_validator_set()),
            )
        }
    }

    /// Checks if all of the signatures in the certificate are correct, and if the set of signatures forms
    /// a quorum.
    fn is_correctly_signed(&self, validator_set: &ValidatorSet) -> bool {
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
                            &(self.chain_id, self.view).try_to_vec().unwrap(),
                            &signature,
                        )
                        .is_ok()
                    {
                        total_power += power;
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
        total_power >= validator_set.quorum()
    }
}

/// Helps leaders incrementally form [`TimeoutCertificate`]s by combining votes for the same chain_id and
/// view by replicas in a given [validator set](ValidatorSet).
#[derive(Clone, PartialEq)]
pub(crate) struct TimeoutVoteCollector {
    chain_id: ChainID,
    view: ViewNumber,
    validator_set: ValidatorSet,
    signature_set_power: TotalPower,
    signature_set: SignatureSet,
}

impl Collector for TimeoutVoteCollector {
    type S = TimeoutVote;
    type C = TimeoutCertificate;

    fn new(chain_id: ChainID, view: ViewNumber, validator_set: ValidatorSet) -> Self {
        let n = validator_set.len();
        Self {
            chain_id,
            view,
            validator_set,
            signature_set_power: TotalPower::new(0),
            signature_set: SignatureSet::new(n),
        }
    }

    fn validator_set(&self) -> &ValidatorSet {
        &self.validator_set
    }

    fn chain_id(&self) -> ChainID {
        self.chain_id
    }

    fn view(&self) -> ViewNumber {
        self.view
    }

    /// Adds the timeout vote to a signature set if it has the correct view and chain id. Returning a Quorum
    /// Certificate if adding the vote allows for one to be created.
    ///
    /// If the timeout vote is not signed correctly, or doesn't match the collector's view, or the signer is
    /// not part of its validator set, then this is a no-op.
    ///
    /// # Preconditions
    /// vote.is_correct(signer)
    fn collect(&mut self, signer: &VerifyingKey, vote: TimeoutVote) -> Option<TimeoutCertificate> {
        if self.chain_id != vote.chain_id || self.view != vote.view {
            return None;
        }

        // Check if the signer is actually in the validator set.
        if let Some(pos) = self.validator_set.position(signer) {
            // If the vote has not been collected before, insert its signature into the signature set.
            if self.signature_set.get(pos).is_none() {
                self.signature_set.set(pos, Some(vote.signature));
                self.signature_set_power += *self.validator_set.power(signer).unwrap();

                // If inserting the vote makes the signature set form a quorum, then create a quorum certificate.
                if self.signature_set_power >= self.validator_set.quorum() {
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
