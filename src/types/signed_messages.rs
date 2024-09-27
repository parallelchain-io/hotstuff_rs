/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Generic `SignedMessage` and `Collector` traits.
//!
//! Implementations used by the [`Pacemaker`][crate::pacemaker::types] and
//! [`HotStuff`][crate::hotstuff::types] protocols can be found in the respective modules. [`Collectors`]
//! groups collectors for all active validator sets into a single struct, which can be easily updated
//! on view or validator set state updates.

pub use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
pub use sha2::Sha256 as CryptoHasher;

use crate::state::{
    block_tree::{BlockTree, BlockTreeError},
    kv_store::KVStore,
};

use super::{
    basic::{ChainID, SignatureBytes, TotalPower, ViewNumber},
    validators::{ValidatorSet, ValidatorSetState},
};

/// Data types that contain a message, and a digital signature over said message whose correctness
/// can be verified against a `VerifyingKey`.
pub(crate) trait SignedMessage: Clone {
    /// Get the bytes that are passed as input into the signing function to form the signature.
    fn message_bytes(&self) -> Vec<u8>;

    /// Get the signature.
    fn signature_bytes(&self) -> SignatureBytes;

    /// Verify that `signature_bytes` is a signature created by `verifying_key` over `message_bytes`.
    fn is_correct(&self, verifying_key: &VerifyingKey) -> bool {
        let signature = Signature::from_bytes(&self.signature_bytes().bytes());
        verifying_key
            .verify(&self.message_bytes(), &signature)
            .is_ok()
    }
}

/// Data types that aggregate multiple [`SignedMessage`]s of the same type into evidence that a
/// [`quorum`](Certificate::quorum) of validators in a particular validator set supports a particular
/// decision.
pub(crate) trait Certificate {
    /// The specific `SignedMessage` type that this `Certificate` aggregates into one value.
    type SignedMessage: SignedMessage;

    /// Check whether the certificate is "correct" ( i.e., whether it can serve as evidence that a particular
    /// decision has been approved by the quorum of validators assigned to collectively make the decision),
    /// given the current `block_tree`.
    ///
    /// ## Guidelines for implementation
    ///
    /// Implementations of `is_correct` should generally execute the following three steps (in addition to
    /// any specialized steps needed to check the correctness of the specific implementing type):
    /// 1. Get [`ValidatorSetState`] from `block_tree`.
    /// 2. Decide whether the certificate should be tested against the Committed Validator Set, or the Previous
    ///    Validator Set, or against both.
    /// 3. Call [`is_correctly_signed`](Certificate::is_correctly_signed) on the certificate, passing the
    ///    committed validator set, the previous validator set, or both (in separate calls).
    fn is_correct<K: KVStore>(&self, block_tree: &BlockTree<K>) -> Result<bool, BlockTreeError>;

    /// Check whether the certificate is correctly signed by a quorum of validators in the given
    /// `validator_set`.
    ///
    /// In general, this method should only be called from inside [`is_correct`](Certificate::is_correct).
    /// Other code should call `is_correct` instead of calling this method.
    fn is_correctly_signed(&self, validator_set: &ValidatorSet) -> bool;

    /// Compute the minimum voting power that a certificate produced by a validator set with
    /// `validator_set_power` total power must contain in order for it to correspond to a "quorum" of
    /// validators.
    ///
    /// "Minimum" here is understood in the inclusive sense, a certificate corresponds to a quorum if its power
    /// is **greater than or equal** to the return value of `quorum`.
    fn quorum(validator_set_power: TotalPower) -> TotalPower {
        const TOTAL_POWER_OVERFLOW: &str =
            "Validator set power exceeds u128::MAX/2. Read the itemdoc for Validator Set.";
        TotalPower::new(
            (validator_set_power
                .int()
                .checked_mul(2)
                .expect(TOTAL_POWER_OVERFLOW)
                / 3)
                + 1,
        )
    }
}

/// Types that progressively combine [`SignedMessage`]s in order to form [`Certificate`]s
///
/// TODO: matching chain_id and view.
///
/// TODO: a single validator set.
pub(crate) trait Collector: Clone {
    /// The specific `SignedMessage` type that this `Collector` takes in as input.
    type SignedMessage: SignedMessage;

    /// The specific `Certificate` type that this `Collector` returns as output.
    type Certificate: Certificate<SignedMessage = Self::SignedMessage>;

    /// Create a new instance of the `Collector`, configuring it to collect `SignedMessage`s for the specified
    /// `chain_id`, and `view`, and signed by a member
    ///
    /// TODO: seems like `chain_id`, `view`, `validator_set` are concepts only in `Collector`, not
    /// `SignedMessage`.
    fn new(chain_id: ChainID, view: ViewNumber, validator_set: ValidatorSet) -> Self;

    fn chain_id(&self) -> ChainID;

    fn view(&self) -> ViewNumber;

    fn validator_set(&self) -> &ValidatorSet;

    /// # No-ops
    ///
    /// This function is a no-op if `message.chain_id`
    ///
    /// # Preconditions
    ///
    /// `message.is_correct(signer)`
    fn collect(
        &mut self,
        signer: &VerifyingKey,
        message: Self::SignedMessage,
    ) -> Option<Self::Certificate>;
}

/// A struct that combines [`Collector`]s for the two validator sets that could be considered "active"
/// at any given [`ValidatorSetState`] (the committed validator set and the previous validator set) and
/// wraps interactions with them behind a single interface.
///
/// ## Usage
///
/// Use [`new`](Self::new) to create a `ActiveCollectors` for a specific `ChainID`, `View`, and the
/// current `ValidatorSetState`. Then, [`collect`](Self::collect) on it to collect any `SignedMessage`s
/// that arrive. Call [`update_validator_sets`](Self::update_validator_sets) whenever the current
/// `ValidatorSetState` changes.
///
/// When the current `View` changes, discard the `ActiveCollectors` and create a new one using the
/// current view.
pub(crate) struct ActiveCollectorPair<CL: Collector> {
    committed_validator_set_collector: CL,
    prev_validator_set_collector: Option<CL>,
}

impl<CL: Collector> ActiveCollectorPair<CL> {
    /// Create `ActiveCollectors` for `chain_id`, `view`, and `validator_set_state`.
    pub(crate) fn new(
        chain_id: ChainID,
        view: ViewNumber,
        validator_set_state: &ValidatorSetState,
    ) -> Self {
        Self {
            committed_validator_set_collector: CL::new(
                chain_id,
                view,
                validator_set_state.committed_validator_set().clone(),
            ),
            prev_validator_set_collector: if validator_set_state.update_decided() {
                None
            } else {
                Some(CL::new(
                    chain_id,
                    view,
                    validator_set_state.previous_validator_set().clone(),
                ))
            },
        }
    }

    /// Collect `message` with the appropriate collector in this `ActiveCollectors`.
    pub(crate) fn collect(
        &mut self,
        signer: &VerifyingKey,
        message: <CL::Certificate as Certificate>::SignedMessage,
    ) -> Option<CL::Certificate> {
        if let Some(certificate) = self
            .committed_validator_set_collector
            .collect(signer, message.clone())
        {
            return Some(certificate);
        } else if let Some(ref mut collector) = self.prev_validator_set_collector {
            if let Some(certificate) = collector.collect(signer, message) {
                return Some(certificate);
            }
        }
        None
    }

    /// Inform this `ActiveCollectors` of the latest current `validator_set_state`.  
    ///
    /// If `validator_set_state` is different from the latest `ValidatorSetState` known by the
    /// `ActiveCollectors`, the collectors will be updated and this function will return `true`. Otherwise
    /// this function returns `false`.
    pub(crate) fn update_validator_sets(
        &mut self,
        validator_set_state: &ValidatorSetState,
    ) -> bool {
        let chain_id = self.committed_validator_set_collector.chain_id();
        let view = self.committed_validator_set_collector.view();

        // If the PVS collector is currently `None`, but now that latest validator set update is not decided,
        // this implies that between the last `update_validator_sets` call and this call, the validator set
        // update period **started**.
        let validator_set_update_period_started =
            self.prev_validator_set_collector.is_none() && !validator_set_state.update_decided();

        // If the current committed validator set is not the same as the latest committed validator set, then
        // the committed validator set was updated.
        let committed_validator_set_was_updated =
            self.committed_validator_set_collector.validator_set()
                != validator_set_state.committed_validator_set();

        // If the PVS collector is currently `Some`, but now the latest validator set update is decided, this
        // implies that between the last `update_validator_sets` call and this call, the validator set update
        // period **ended**.
        let validator_set_update_period_ended =
            self.prev_validator_set_collector.is_some() && validator_set_state.update_decided();

        // If a validator set update has been initiated but no vote collector has been assigned for the
        // previous validator set, the latest previous validator set must be equal to the
        // current committed validator set...
        if validator_set_update_period_started {
            if validator_set_state.previous_validator_set()
                == self.committed_validator_set_collector.validator_set()
            {
                // ...so we replace the current previous validator set with the current committed validator set.
                self.prev_validator_set_collector =
                    Some(self.committed_validator_set_collector.clone())
            } else {
                unreachable!(
                    "if the validator set update period started, then the latest previous validator set should be equal
                    to committed validator set currently known by the `ActiveCollectorPair`. The fact that this 
                    invariant is broken suggests that an internal call to `update_validator_sets` was 'skipped' and
                    therefore the `ActiveCollectorPair` missed a validator set update. This is a library bug"
                )
            }
        }

        // If the latest committed validator set was updated, create a new collector and set it as the CVS
        // collector.
        if committed_validator_set_was_updated {
            self.committed_validator_set_collector = CL::new(
                chain_id,
                view,
                validator_set_state.committed_validator_set().clone(),
            )
        }

        // If the validator set update period has ended, set the PVS collector to `None`. It is not needed
        // anymore.
        if validator_set_update_period_ended {
            self.prev_validator_set_collector = None
        }

        validator_set_update_period_started
            || committed_validator_set_was_updated
            || validator_set_update_period_ended
    }
}
