/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Types specific to the HotStuff subprotocol.

use std::collections::HashMap;

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::Verifier;

use super::messages::Vote;
use crate::state::{
    block_tree::{BlockTree, BlockTreeError},
    kv_store::KVStore,
};
use crate::types::{
    basic::*,
    collectors::{Certificate, Collector},
    validators::*,
};

/// Proof that at least a quorum of validators have voted for a specific
/// [`Proposal`][crate::hotstuff::messages::Proposal] or [`Nudge`][crate::hotstuff::messages::Nudge].
///
/// Required for extending a block in the HotStuff subprotocol, and for optimistic advance to a new
/// view in the [pacemaker][crate::pacemaker] protocol.
#[derive(Clone, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
pub struct QuorumCertificate {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub block: CryptoHash,
    pub phase: Phase,
    pub signatures: SignatureSet,
}

impl Certificate for QuorumCertificate {
    /// Compute the appropriate validator set that the QC should be checked against, and check if the
    /// signatures in the certificate are correct and form a quorum given this validator set.
    ///
    /// A special case is if the qc is the Genesis QC, in which case it is automatically correct.
    fn is_correct<K: KVStore>(&self, block_tree: &BlockTree<K>) -> Result<bool, BlockTreeError> {
        if self.is_genesis_qc() {
            return Ok(true);
        };

        let block_height = block_tree.block_height(&self.block)?;

        let validator_set_state = block_tree.validator_set_state()?;

        let result = match (block_height, validator_set_state.update_height()) {
            // If the Block that the QC certifies is not in the block tree, **or** else if the validator set has
            // never been updated in the history of the blockchain, validate the QC according to the current
            // committed validator set.
            (None, _) | (Some(_), &None) => {
                self.is_correctly_signed(validator_set_state.committed_validator_set())
            }

            // If the Block that the QC certifies is in the block tree at `height` **and** the validator set was
            // last updated at height `update_height`:
            (Some(height), &Some(update_height)) => {
                // If the Block comes in the block tree before the latest validator set updating block, validate the QC
                // according to the previous validator set.
                if height < update_height {
                    self.is_correctly_signed(validator_set_state.previous_validator_set())

                // Else if the Block comes after the latest validator set updating block, validate the QC according to
                // the committed validator set.
                } else if height > update_height {
                    self.is_correctly_signed(validator_set_state.committed_validator_set())

                // Else if the Block **is** the latest validator set updating block:
                } else {
                    match self.phase {
                        // If the QC is a Decide QC:
                        Phase::Decide => {
                            match block_tree.validator_set_updates_status(&self.block)? {
                                // If the block's validator set updates have been committed, then validate the QC according to the
                                // committed validator set.
                                ValidatorSetUpdatesStatus::Committed => self.is_correctly_signed(
                                    validator_set_state.committed_validator_set(),
                                ),

                                // If the block's validator set updates have not been committed, then apply the validator set updates
                                // to the committed validator set and validate the QC according to the resulting validator set.
                                //
                                // Note (from Karolina): this may be the case if the block justified by this QC is received via sync
                                // hence the updates have not been applied yet. In this case we need to compute the validator set
                                // expected to have voted "Decide" for the block.
                                ValidatorSetUpdatesStatus::Pending(vs_updates) => {
                                    let mut new_validator_set =
                                        block_tree.committed_validator_set()?;
                                    new_validator_set.apply_updates(&vs_updates);
                                    self.is_correctly_signed(&new_validator_set)
                                }

                                // If the block does not have validator set updates, then it should be justified by a Generic QC, not
                                // a phased mode QC like a Decide QC. Therefore, the QC is invalid.
                                //
                                // Issue: https://github.com/parallelchain-io/hotstuff_rs/issues/46
                                ValidatorSetUpdatesStatus::None => false,
                            }
                        }

                        // Else if the QC is a phased mode QC that is not a Decide QC:
                        Phase::Prepare | Phase::Precommit | Phase::Commit => {
                            match block_tree.validator_set_updates_status(&self.block)? {
                                // If the latest validator set update has been committed
                                ValidatorSetUpdatesStatus::Committed => self.is_correctly_signed(
                                    validator_set_state.previous_validator_set(),
                                ),
                                ValidatorSetUpdatesStatus::Pending(_) => self.is_correctly_signed(
                                    validator_set_state.committed_validator_set(),
                                ),

                                // If the block does not have validator set updates, then it should be justified by a Generic QC, not
                                // a phased mode QC like a Prepare QC, Precommit QC, or Commit QC. Therefore, the QC is invalid.
                                //
                                // Issue: https://github.com/parallelchain-io/hotstuff_rs/issues/46
                                ValidatorSetUpdatesStatus::None => false,
                            }
                        }

                        // If the block has validator set updates, then it should be justified by a phased mode QC, not a
                        // Generic QC. Therefore, the QC is invalid.
                        //
                        // Issue: https://github.com/parallelchain-io/hotstuff_rs/issues/46
                        // Note (from Karolina): cannot panic here, since safe_qc has not been checked yet.
                        _ => false,
                    }
                }
            }
        };

        Ok(result)
    }

    /// Check if all of the signatures in the certificate are correct, and if the set of signatures forms
    /// a quorum according to the provided `validator_set`.
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

impl QuorumCertificate {
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

    pub fn is_block_justify(&self) -> bool {
        self.phase.is_generic() || self.phase.is_decide()
    }

    pub fn is_nudge_justify(&self) -> bool {
        self.phase.is_prepare() || self.phase.is_precommit() || self.phase.is_commit()
    }
}

/// Voting phase in the hotstuff-rs consensus protocol.
#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize, Debug)]
pub enum Phase {
    // ↓↓↓ For pipelined flow ↓↓↓ //
    Generic,

    // ↓↓↓ For phased flow ↓↓↓ //
    Prepare,

    Precommit,

    Commit,

    Decide,
}

impl Phase {
    pub fn is_generic(self) -> bool {
        self == Phase::Generic
    }

    pub fn is_prepare(self) -> bool {
        self == Phase::Prepare
    }

    pub fn is_precommit(self) -> bool {
        matches!(self, Phase::Precommit)
    }

    pub fn is_commit(self) -> bool {
        matches!(self, Phase::Commit)
    }

    pub fn is_decide(self) -> bool {
        matches!(self, Phase::Decide)
    }
}

/// Serves to incrementally form a [`QuorumCertificate`] by keeping track of votes for the same chain id,
/// view, block, and phase by replicas from a given [validator set](ValidatorSet).
#[derive(Clone)]
pub(crate) struct VoteCollector {
    chain_id: ChainID,
    view: ViewNumber,
    validator_set: ValidatorSet,
    signature_sets: HashMap<(CryptoHash, Phase), (SignatureSet, TotalPower)>,
}

impl Collector for VoteCollector {
    type C = QuorumCertificate;
    type S = Vote;

    fn new(chain_id: ChainID, view: ViewNumber, validator_set: ValidatorSet) -> Self {
        Self {
            chain_id,
            view,
            validator_set,
            signature_sets: HashMap::new(),
        }
    }

    fn chain_id(&self) -> ChainID {
        self.chain_id
    }

    fn validator_set(&self) -> &ValidatorSet {
        &self.validator_set
    }

    fn view(&self) -> ViewNumber {
        self.view
    }

    /// Add a `vote` to a signature set for the specified view, block, and phase. Returns a Quorum
    /// Certificate if adding this vote allows for one to be created.
    ///
    /// If the vote is not signed correctly, or doesn't match the collector's view, or the signer is not
    /// part of its validator set, then this function is a no-op.
    ///
    /// ## Preconditions
    ///
    /// `vote.is_correct(signer)`
    fn collect(&mut self, signer: &VerifyingKey, vote: Vote) -> Option<QuorumCertificate> {
        if self.chain_id != vote.chain_id || self.view != vote.view {
            return None;
        }

        // Check if the signer is actually in the validator set.
        if let Some(pos) = self.validator_set.position(signer) {
            // If the vote is for a new (block, phase) pair, prepare an empty signature set.
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.signature_sets.entry((vote.block, vote.phase))
            {
                e.insert((
                    SignatureSet::new(self.validator_set.len()),
                    TotalPower::new(0),
                ));
            }

            let (signature_set, signature_set_power) = self
                .signature_sets
                .get_mut(&(vote.block, vote.phase))
                .unwrap();

            // If a vote for the (block, phase) from the signer hasn't been collected before, insert it into the
            // signature set.
            if signature_set.get(pos).is_none() {
                signature_set.set(pos, Some(vote.signature));
                *signature_set_power += *self.validator_set.power(signer).unwrap();

                // If inserting the vote makes the signature set form a quorum, then create a Quorum Certificate.
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
