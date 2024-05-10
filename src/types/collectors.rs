/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines the generic [`SignedMessage`] and [`Collector`] traits. 
//! 
//! Implementations used by the [Pacemaker][crate::pacemaker::types] and 
//! [HotStuff][crate::hotstuff::types] protocols can be found in the respective modules. [`Collectors`]
//! groups collectors for all active validator sets into a single struct, which can be easily updated
//! on view or validator set state updates.

pub use ed25519_dalek::{SigningKey, VerifyingKey, Signature};
pub use sha2::Sha256 as CryptoHasher;

use crate::messages::SignedMessage;
use crate::state::{
    block_tree::{BlockTree, BlockTreeError}, 
    kv_store::KVStore
};

use super::{
    basic::{ChainID, TotalPower, ViewNumber}, 
    validators::{ValidatorSet, ValidatorSetState}
};

/// Evidence that a quorum of validators from a given validator set supports a given decision. The 
/// evidence comes in the form of a set of signatures of the validators.
pub trait Certificate {

    /// Returns whether the certificate is correct, i.e., can serve as evidence that a given block is 
    /// approved by a quorum of validators assigned to validate it. This may require checking if it is 
    /// correctly signed by a quorum of validators from an appropriate validator set.
    fn is_correct<K: KVStore>(&self, block_tree: &BlockTree<K>) -> Result<bool, BlockTreeError>;

    /// Returns whether the certificate is correctly signed by a quorum of validators from the given 
    /// validator set.
    fn is_correctly_signed(&self, validator_set: &ValidatorSet) -> bool;
    
    /// Returns the total power of validators from the given validator set that corresponds to a quorum.
    fn quorum(validator_set_power: TotalPower) -> TotalPower {
        const TOTAL_POWER_OVERFLOW: &str = "Validator set power exceeds u128::MAX/2. Read the itemdoc for Validator Set.";
        TotalPower::new(
            (validator_set_power
            .int()
            .checked_mul(2)
            .expect(TOTAL_POWER_OVERFLOW)
            / 3)
            + 1
        )
    }

}

/// Collects [correct][SignedMessage::is_correct] [signed messages][SignedMessage] into a [Certificate].
/// Otherwise, stores the collected signatures collected from members of a given [validator set](ValidatorSet).
pub(crate) trait Collector: Clone {
    type S: SignedMessage;
    type C: Certificate;

    fn new(chain_id: ChainID, view: ViewNumber, validator_set: ValidatorSet) -> Self;

    fn chain_id(&self) -> ChainID;

    fn view(&self) -> ViewNumber;

    fn validator_set(&self) -> &ValidatorSet;

    fn collect(&mut self, signer: &VerifyingKey, message: Self::S) -> Option<Self::C>;
}

/// Combines the collectors for the two potentially active validator sets:
/// 1. The committed validator set, 
/// 2. The previous validator set (only during the validator set update period).
pub(crate) struct Collectors<CL: Collector> {
    committed_validator_set_collector: CL,
    prev_validator_set_collector: Option<CL>,
}

impl<CL:Collector> Collectors<CL> {

    /// Create fresh collectors for given view, chain id, and validator set state.
    pub(crate) fn new(chain_id: ChainID, view: ViewNumber, validator_set_state: &ValidatorSetState) -> Self {
        Self{
            committed_validator_set_collector: CL::new(chain_id, view, validator_set_state.committed_validator_set().clone()),
            prev_validator_set_collector: 
                if validator_set_state.update_completed() {
                    None
                } else {
                    Some(CL::new(chain_id, view, validator_set_state.previous_validator_set().clone()))
                }
        }
    }

    /// Update the collectors in response to an update of the [validator set state](ValidatorSetState).
    pub(crate) fn update_validator_sets(&mut self, validator_set_state: &ValidatorSetState) {
        
        let chain_id = self.committed_validator_set_collector.chain_id();
        let view = self.committed_validator_set_collector.view();

        // If a validator set update has been initiated but no vote collector has been assigned for
        // the previous validator set. In this case, the previous validator set must be equal to the
        // committed validator set stored by the vote collectors.
        if self.prev_validator_set_collector.is_none() & !validator_set_state.update_completed() {
            if validator_set_state.previous_validator_set() == self.committed_validator_set_collector.validator_set() {
                self.prev_validator_set_collector = Some(self.committed_validator_set_collector.clone())
            } else {
                panic!() // Safety: as explained above.
            }
        }

        // If the committed validator set has been updated.
        if self.committed_validator_set_collector.validator_set() != validator_set_state.committed_validator_set() {
            self.committed_validator_set_collector = CL::new(
                chain_id, 
                view, 
                validator_set_state.committed_validator_set().clone(),
            )
        }

        // If the validator set update has been marked as complete. In this case, a collector
        // for the previous validator set is not required anymore.
        if self.prev_validator_set_collector.is_some() && validator_set_state.update_completed() {
            self.prev_validator_set_collector = None
        }

    }

    /// Checks if the collectors should be updated given possible updates to the validator
    /// set state.
    pub(crate) fn should_update_validator_sets(&self, validator_set_state: &ValidatorSetState) -> bool {
        self.committed_validator_set_collector.validator_set() != validator_set_state.committed_validator_set() ||
        (self.prev_validator_set_collector.is_some() && validator_set_state.update_completed()) ||
        (self.prev_validator_set_collector.is_none() && !validator_set_state.update_completed())
    }

    /// Collects the message with the appropriate collector.
    pub(crate) fn collect(&mut self, signer: &VerifyingKey, message: CL::S) -> Option<CL::C> {
        if let Some(certificate) = self.committed_validator_set_collector.collect(signer, message.clone()) {
            return Some(certificate)
        } else if let Some(ref mut collector) = self.prev_validator_set_collector {
            if let Some(certificate) = collector.collect(signer, message) {
                return Some(certificate)
            }
        }
        None
    }
}