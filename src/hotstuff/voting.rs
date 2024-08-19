/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Functions that define which replicas are allowed to propose and vote at any given point in the
//! execution of the HotStuff subprotocol.
//!
//! ## Validator Set Update protocol
//!
//! HotStuff-rs' modified HotStuff SMR adds an extra "decide" phase to the phased HotStuff protocol.
//! This phase serves as the transition phase for the validator set update, and is associated with
//! the following invariant which implies the liveness as well as immediacy of a validator set update:
//!
//! > "If there exists a valid decideQC for a validator-set-updating block, then a quorum from the new
//! > validator set has committed the block together with the validator-set-update, and hence is ready
//! > to act as the newly committed validator set"
//!
//! Concretely, suppose B is a block that updates the validator set from vs to vs'. Once a commitQC
//! for B is seen, vs' becomes the committed validator set, and vs is stored as the previous validator
//! set, with the update being marked as incomplete. While the update is incomplete, vs may still be
//! active: it is allowed to re-broadcast the proposal for B or nudges for B. This is to account for the
//! possibility that due to asynchrony or byzantine behaviour not enough replicas may have received the
//! commitQC, and hence progressing to a succesful "decide" phase is not possible without such
//! re-broadcast. Once a valid decideQC signed by a quorum of validators from vs' is seen, the update is
//! marked as complete and vs becomes inactive.

use ed25519_dalek::VerifyingKey;

use crate::pacemaker::protocol::select_leader;
use crate::types::{basic::ViewNumber, validators::ValidatorSetState};

use super::messages::{NewView, Vote};
use super::types::{Phase, QuorumCertificate};

/// Check whether the `replica` with the specified `VerifyingKey` is an active validator, given the
/// current `validator_set_state`.
///
/// An active validator can:
/// - Propose/nudge and vote in the HotStuff protocol under certain circumstances described above,
/// - Contribute [timeout votes](crate::pacemaker::messages::TimeoutVote) and
///    [advance view messages](crate::pacemaker::messages::AdvanceView).
///
/// In general, members of the committed validator set are always active, but during the validator
/// set update period members of the previous validator set are active too.
pub(crate) fn is_validator(
    replica: &VerifyingKey,
    validator_set_state: &ValidatorSetState,
) -> bool {
    validator_set_state
        .committed_validator_set()
        .contains(replica)
        || (!validator_set_state.update_decided()
            && validator_set_state
                .previous_validator_set()
                .contains(replica))
}

/// Check whether `replica`'s votes can become part of quorum certificates that directly extend `justify`,
/// given the current `validator_set_update`.
///
/// If `replica`'s votes cannot become part of QCs that directly extend `justify`, then it is fruitless
/// to vote for the `Proposal` or `Nudge` containing `justify`, since the next leader will ignore the
/// replica's votes anyway.
///
/// ## Rules
///
/// Whether or not `replica`'s vote can become part of QCs that directly extend `justify` depends on
/// whether or not the latest validator set update has been decided:
/// ([`validator_set_state.update_decided()`](ValidatorSetState::update_decided)):
/// - If it **has** been decided, then `replica` is allowed to vote only if it is in the **committed**
///    validator set.
/// - If it **has not** been decided, then `replica` is allowed to vote only if:
///     - `justify.phase` is `Commit`, and `replica` is in the **committed** validator set.
///     - `justify.phase` is `Generic`, `Prepare`, `Precommit`, or `Decide`, and `replica` is in the previous
///       validator set.
///
/// ## Preconditions
///
/// `justify` satisfies [`safe_qc`](crate::state::safety::safe_qc) and
/// [`is_correct`](crate::types::collectors::Certificate::is_correct), and the block tree updates
/// associated with this `justify` have already been applied.
pub(crate) fn is_voter(
    replica: &VerifyingKey,
    validator_set_state: &ValidatorSetState,
    justify: &QuorumCertificate,
) -> bool {
    if validator_set_state.update_decided() {
        validator_set_state
            .committed_validator_set()
            .contains(&replica)
    } else {
        match justify.phase {
            Phase::Generic | Phase::Prepare | Phase::Precommit | Phase::Decide => {
                validator_set_state
                    .previous_validator_set()
                    .contains(&replica)
            }
            Phase::Commit => validator_set_state
                .committed_validator_set()
                .contains(&replica),
        }
    }
}

/// Check whether `validator` is allowed to act as a proposer in the given `view`, given the current
/// `validator_set_state`.
///
/// `validator` is a proposer in two situations:
/// 1. `validator` is the leader of the current view in the **committed** validator set, or
/// 2. The latest validator set update is not decided yet, and `validator` is the leader of the current
///    view in the **previous** validator set.
pub(crate) fn is_proposer(
    validator: &VerifyingKey,
    view: ViewNumber,
    validator_set_state: &ValidatorSetState,
) -> bool {
    validator == &select_leader(view, validator_set_state.committed_validator_set())
        || (!validator_set_state.update_decided()
            && validator == &select_leader(view, validator_set_state.previous_validator_set()))
}

/// Identify the replica that should receive `vote`, given the current `validator_set_state`.
///
/// ## Rules
///
/// Which replica should be the recipient of `vote` depends on whether the latest validator set update
/// has been decided:
/// - If it **has** been decided, then the recipient should be the leader of `vote.view + 1` in the
///   **committed** validator set.
/// - If it **has not** been decided:
///     - ...and if `vote.phase` is `Generic`, `Prepare`, `Precommit`, or `Commit`, then the recipient should
///       be the leader of `vote.view + 1` in the **previous** validator set.
///     - ...and if `vote.phase` is `Decide`, then the recipient should be the leader of `vote.view + 1` in
///       the **committed** validator set.
pub(crate) fn vote_recipient(vote: &Vote, validator_set_state: &ValidatorSetState) -> VerifyingKey {
    if validator_set_state.update_decided() {
        select_leader(vote.view + 1, validator_set_state.committed_validator_set())
    } else {
        match vote.phase {
            Phase::Generic | Phase::Prepare | Phase::Precommit | Phase::Commit => {
                select_leader(vote.view + 1, validator_set_state.previous_validator_set())
            }
            Phase::Decide => {
                select_leader(vote.view + 1, validator_set_state.committed_validator_set())
            }
        }
    }
}

/// Identify the replica(s) that should receive `new_view`, given the current `validator_set_state`.
///
/// If the latest validator set update has been decided, then the view only has one leader (taken from
/// the committed validator set), but if it hasn't, then the view will have two leaders (the one
/// additional leader coming from the previous validator set).
///
/// ## Return value
///
/// Returns a pair containing the following items:
/// 1. `VerifyingKey`: the leader in the committed validator set in `new_view.view + 1`.
/// 2. `Option<VerifyingKey>`: the leader in the resigning validator set in `new_view.view + 1` (`None`
///     if the most recently initiated validator set update has been decided).
pub(crate) fn new_view_recipients(
    new_view: &NewView,
    validator_set_state: &ValidatorSetState,
) -> (VerifyingKey, Option<VerifyingKey>) {
    (
        select_leader(
            new_view.view + 1,
            validator_set_state.committed_validator_set(),
        ),
        if validator_set_state.update_decided() {
            None
        } else {
            Some(select_leader(
                new_view.view + 1,
                validator_set_state.previous_validator_set(),
            ))
        },
    )
}
