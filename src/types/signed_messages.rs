/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Signed messages, votes, and certificates.
//!
//! The basic definition made in this module is the **[`SignedMessage`]** trait, which, as its rustdocs
//! explain, is implemented by data types that contain: 1. A message, and 2. A digital signature over
//! said message whose correctness can be verified against a `VerifyingKey`.
//!
//! Although `SignedMessage` on its own is implemented by a single library-defined type (namely
//! `AdvertiseBlock`), most of its utility comes from being a supertrait of the more important
//! **[`Vote`]** trait, also defined in this module.
//!
//! "Votes", along with "Certificates" are two notions that are common to both the
//! [HotStuff](crate::hotstuff) and [Pacemaker](crate::pacemaker) subprotocols, and are essential to
//! their functioning. These are represented in this module by the `Vote` and [`Certificate`] traits.
//!
//! `Vote`s and `Certificate`s are data types that represent, respectively: a *single validator's*
//! digitally signed, non-repudiable agreement to a "decision", and the same thing but for a *set of
//! validators* instead of only a single one.
//!
//! Certificates are formed by using structs called "Collectors" to aggregate the signatures of
//! Votes with matching `ViewNumber`s and `ChainID`s together until the collected
//! [`SignatureSet`](crate::types::data_types::SignatureSet) contains votes from a
//! ["quorum"](Certificate::quorum) of validators in a validator set. Collector structs generally
//! implement the same basic logic whichever specific `Vote` type they collect into whichever specific
//! `Certificate` type, which is why this module can define a **[`Collector`]** trait that generalizes
//! across collector implementations.
//!
//! The `Collector` trait in turn allows this module to define a generic "Active Collector Pair"
//! struct. An **[`ActiveCollectorPair`]** is not itself a `Collector`, but combines multiple collectors
//! collecting votes for different "active" validator sets into a single struct and wraps interactions
//! with them inside a single interface, which simplifies usage.

use crate::block_tree::{
    accessors::internal::{BlockTreeError, BlockTreeSingleton},
    pluggables::KVStore,
};

use super::{
    crypto_primitives::{Signature, Verifier, VerifyingKey},
    data_types::{ChainID, SignatureBytes, TotalPower, ViewNumber},
    validator_set::{ValidatorSet, ValidatorSetState},
};

/// Data types that contain: 1. A message, and 2. A digital signature over said message whose
/// correctness can be verified against a `VerifyingKey`.
pub(crate) trait SignedMessage: Clone {
    /// Get the bytes that are passed as input into the signing function to form the signature
    /// of the `SignedMessage`.
    fn message_bytes(&self) -> Vec<u8>;

    /// Get the signature of the `SignedMessage`.
    fn signature_bytes(&self) -> SignatureBytes;

    /// Verify that `signature_bytes` is a signature created by `verifying_key` over `message_bytes`.
    fn is_correct(&self, verifying_key: &VerifyingKey) -> bool {
        let signature = Signature::from_bytes(&self.signature_bytes().bytes());
        verifying_key
            .verify(&self.message_bytes(), &signature)
            .is_ok()
    }
}

/// Data types that indicate that a validator supports a particular **decision** about a particular
/// `chain_id` and `view`.
pub(crate) trait Vote: SignedMessage {
    /// Get the `chain_id` of the chain that the `Vote` is about.
    fn chain_id(&self) -> ChainID;

    /// Get the `view` that the `Vote` is about.
    fn view(&self) -> ViewNumber;
}

/// Data types that aggregate multiple [`Vote`]s of the same type into evidence that a
/// [`quorum`](Certificate::quorum) of validators in a particular validator set supports a particular
/// decision.
pub(crate) trait Certificate {
    /// The specific `Vote` type that this `Certificate` aggregates into one value.
    type Vote: Vote;

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
    fn is_correct<K: KVStore>(
        &self,
        block_tree: &BlockTreeSingleton<K>,
    ) -> Result<bool, BlockTreeError>;

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
    /// "Minimum" here is understood in the inclusive sense, a certificate corresponds to a quorum if its
    /// power is **greater than or equal** to the return value of `quorum`.
    ///
    /// # Uniqueness of quorums
    ///
    /// The quorum size of a validator set with the total power `validator_set_power` is
    /// `ceil(validator_set_power * 2/3)`.
    ///
    /// This exact threshold (**2/3rds**) of the total power of a validator set is the minimum
    /// needed to guarantee the invariant that in any view, at most one `Certificate` (of each concrete
    /// kind) can be formed, given that at most 1/3rds of the total power could be Byzantine.
    ///
    /// To see why, consider the "worst-case" scenario where, in the current view:
    /// - Just under 1/3rds ("1/3rds - 1") of the total power of the current validator set is Byzantine.
    /// - Just over 2/3rds ("2/3rds + 1") of the total power is honest.
    ///
    /// To attack the uniqueness invariant in this scenario (i.e., to try and form two conflicting
    /// certificates), all Byzantine validators would (double-)vote for conflicting certificates `A` and
    /// `B` with the same view number. Both certificates would now have the power "1/3rds - 1". Then, since
    /// honest validators only vote for one certificate of each concrete kind per view, one of the following
    /// will be the case:
    /// - Only `A` accumulates enough votes to constitute a quorum.
    /// - Only `B` accumulates enough votes to constitute a quorum.
    /// - Neither `A` and `B` accumulate enough votes to constitute a quorum.
    ///
    /// In particular, the case where both `A` and `B` both constitute a quorum is impossible, because there
    /// simply isn't enough remaining votes in "2/3rds + 1" to form two quorums. Critically, note that if
    /// the quorum threshold were any lower (e.g., just 2/3rds), this case would be possible, and the
    /// invariant would no longer hold.
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

/// Types that progressively combine `Vote`s with the same `chain_id`, `view`, and `validator_set` to form
/// `Certificate`s.
pub(crate) trait Collector: Clone {
    /// The specific `SignedMessage` type that this `Collector` takes in as input.
    type Vote: Vote;

    /// The specific `Certificate` type that this `Collector` returns as output.
    type Certificate: Certificate<Vote = Self::Vote>;

    /// Create a new instance of the `Collector`, configuring it to collect `SignedMessage`s for the specified
    /// `chain_id`, and `view`, and signed by a member of `validator_set`.
    fn new(chain_id: ChainID, view: ViewNumber, validator_set: ValidatorSet) -> Self;

    /// Collect a `vote` signed by `signer`, returning a `Certificate` if a [`quorum`](Certificate::quorum)
    /// of matching `Vote`s from the `Collector`'s configured [`validator_set`](Collector::validator_set)
    /// has been collected.
    ///
    /// # No-ops
    ///
    /// Calling this method is a no-op if:
    /// - `vote.chain_id != self.chain_id()`
    /// - `vote.view != self.view()`
    ///
    /// # Preconditions
    ///
    /// [`vote.is_correct(signer)`](SignedMessage::is_correct).
    fn collect(&mut self, signer: &VerifyingKey, vote: Self::Vote) -> Option<Self::Certificate>;

    /// Get the `ChainID` of the chain that this `Collector` is currently configured to collect `Vote`s about.
    fn chain_id(&self) -> ChainID;

    /// Get the `View` that this `Collector` is currently configured to collect `Vote`s about.
    fn view(&self) -> ViewNumber;

    /// Get the `ValidatorSet` that this `Collector` is currently configured to collect `Vote`s from.
    fn validator_set(&self) -> &ValidatorSet;
}

/// Struct that combines [`Collector`]s for the validator set(s) that could be considered
/// ["active"](Self#active-validator-sets) at any given [`ValidatorSetState`], wrapping interaction with
/// them behind a single interface.
///
/// # Active Validator Sets
///
/// At any specific `ValidatorSetState` and in the execution of the [HotStuff](crate::hotstuff) and
/// [Pacemaker](crate::pacemaker) subprotocols, the "Active Validator Sets" are the validator set(s)
/// that should participate in voting (by sending [`PhaseVote`](crate::hotstuff::messages::PhaseVote)s
/// and [`TimeoutVote`](crate::pacemaker::messages::TimeoutVote)s, respectively).
///
/// At most **two** validator sets could be active at any given `validator_set_state`:
/// 1. The **Committed Validator Set (CVS)** is always an active validator set,
/// 2. While the **Previous Validator Set (PVS)** is active if-and-only-if the latest validator set
///    update is *not* decided (i.e.,
///    [`validator_set_state.update_decided()`](ValidatorSetState::update_decided)).
///
/// Because `Vote`s do not indicate which validator set they are a part of, `ActiveCollectorPair`'s
/// [`collect`](Self::collect) method tries to collect any vote it receives with both collectors the
/// pair may currently contain in turn.
///
/// # Usage
///
/// Use [`new`](Self::new) to create a `ActiveCollectorPair` for a specific `ChainID`, `View`, and the
/// current `ValidatorSetState`. Then, call `collect` on it any time a `Vote` arrives.
///
/// Call [`update_validator_sets`](Self::update_validator_sets) whenever the current `ValidatorSetState`
/// changes. When the current `ViewNumber` changes, discard the `ActiveCollectorPair` and create a new one
/// using the current view.
pub(crate) struct ActiveCollectorPair<CL: Collector> {
    /// `Collector` collecting votes from the current Committed Validator Set (CVS).
    cvs_collector: CL,

    /// `Collector` collecting votes from the current Previous Validator Set (PVS). Is set to `None` if
    /// `validator_set_state.update_decided()`.
    pvs_collector: Option<CL>,
}

impl<CL: Collector> ActiveCollectorPair<CL> {
    /// Create an `ActiveCollectorPair` for `chain_id`, `view`, and `validator_set_state`.
    pub(crate) fn new(
        chain_id: ChainID,
        view: ViewNumber,
        validator_set_state: &ValidatorSetState,
    ) -> Self {
        Self {
            cvs_collector: CL::new(
                chain_id,
                view,
                validator_set_state.committed_validator_set().clone(),
            ),
            pvs_collector: if validator_set_state.update_decided() {
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

    /// Collect `message` with both collectors in this `ActiveCollectorPair`.
    pub(crate) fn collect(
        &mut self,
        signer: &VerifyingKey,
        message: CL::Vote,
    ) -> Option<CL::Certificate> {
        if let Some(certificate) = self.cvs_collector.collect(signer, message.clone()) {
            return Some(certificate);
        } else if let Some(ref mut collector) = self.pvs_collector {
            if let Some(certificate) = collector.collect(signer, message) {
                return Some(certificate);
            }
        }
        None
    }

    /// Update the `ActiveCollectorPair`'s active validator sets given the latest current
    /// `validator_set_state`.
    ///
    /// If `validator_set_state` is different from the latest `ValidatorSetState` known by the
    /// `ActiveCollectorPair`, the collectors will be updated and this method will return `true`. Otherwise
    /// calling this method is a no-op and returns `false`.
    pub(crate) fn update_validator_sets(&mut self, latest_vss: &ValidatorSetState) -> bool {
        let mut is_updated = false;

        // If the latest VSS's CVS is different from the current **CVS** collector's validator set, replace the
        // current CVS collector with a new collector.
        if latest_vss.committed_validator_set() != self.cvs_collector.validator_set() {
            self.cvs_collector = CL::new(
                self.cvs_collector.chain_id(),
                self.cvs_collector.view(),
                latest_vss.committed_validator_set().clone(),
            );

            is_updated = true;
        }

        // If the latest validator set update **has** been decided, and the current **PVS** collector is
        //`Some`, then set the PVS collector to `None`.
        if latest_vss.update_decided() && self.pvs_collector.is_some() {
            self.pvs_collector = None;

            is_updated = true;
        }

        // Else, if the latest validator set update has **not** been decided, and the latest VSS' **PVS** is
        // different from the current PVS collector's validator set, decide what to replace the collector 
        // pair's PVS collector with:
        else if self.pvs_collector.is_none() ||
            latest_vss.previous_validator_set() != self.pvs_collector.as_ref().expect("if `pvs_collector` is `None`, execution should have short-circuited before reaching here").validator_set() {

            // If the latest VSS' PVS is the same as the current CVS collector's validator set, then replace
            // the current PVS collector with the current CVS collector.
            self.pvs_collector = Some(if latest_vss.previous_validator_set() == self.cvs_collector.validator_set() {
                self.cvs_collector.clone()
            }
            // Else, if it is not the same, then replace the current PVS collector with a new collector.
            else {
                CL::new(
                    self.cvs_collector.chain_id(),
                    self.cvs_collector.view(),
                    latest_vss.previous_validator_set().clone(),
                )
            });

            is_updated = true;
        }

        is_updated
    }
}
