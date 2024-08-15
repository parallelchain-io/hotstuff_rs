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

use crate::hotstuff::messages::Vote;
use crate::hotstuff::types::{Phase, QuorumCertificate};
use crate::pacemaker::protocol::select_leader;
use crate::types::{basic::ViewNumber, validators::ValidatorSetState};

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

/// Get the `VerifyingKey` of the replica tasked with receiving and collecting a given `vote`.
///
/// Usually, the recipient of a vote is the leader of the committed validator set for the subsequent
/// view. However, during the validator set update period the leader of the previous validator set
/// for the next view is tasked with collecting all kinds of votes other than decide-phase votes, which
/// are addressed to the next leader of the committed validator set.
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

/// Check whether the `replica` with the given `VerifyingKey` is allowed to vote for a nudge or proposal
/// with the given `justify`, given the current `validator_set_state`.
///
/// ## Rules
///
/// Whether or not `replica` is allowed to vote depends on whether or not the latest validator set
/// update has been decided:
/// 1. If it **has** been decided, then `replica` is allowed to vote only if it is in the committed
///   validator set.
/// 2. If it **has not** been decided, then `replica` is allowed to vote if:
///     1. `justify.phase` is `Commit`, and `replica` is in the committed validator set.
///     2. `justify.phase` is `Generic`, `Prepare`, `Precommit`, or `Decide`, and `replica` is in the previous
///       validator set.
///
/// `replica` must be in the committed validator set in case 2.1 since `justify.phase` is `Commit` only
/// if a `Decide` QC is to be formed in this view, and `Decide` QCs must contain votes from the next
/// validator set, while case 2.2 keeps validators in the previous validator set "active" until a
/// validator set update has been decided.
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

/// Check whether the `replica` with the specified `VerifyingKey` is an active validator, given the
/// current `validator_set_state`.
///
/// An active validator can:
/// 1. Propose/nudge and vote in the HotStuff protocol under certain circumstances described above,
/// 2. Contribute [timeout votes](crate::pacemaker::messages::TimeoutVote) and
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

/// Compute the leader(s) of a given `view` given the current `validator_set_state`.
///
/// If the latest validator set update has been decided, then the view only has one leader (taken from
/// the committed validator set), but if it hasn't, then the view will have two leaders (the one
/// additional leader coming from the previous validator set).
///
/// ## Return value
///
/// Returns a pair containing the following items:
/// 1. `VerifyingKey`: the leader in the committed validator set in the specified view.
/// 2. `Option<VerifyingKey>`: the leader in the resigning validator set in the specified view (`None`
///     if the most recently initiated validator set update has been decided).
pub(crate) fn leaders(
    view: ViewNumber,
    validator_set_state: &ValidatorSetState,
) -> (VerifyingKey, Option<VerifyingKey>) {
    (
        select_leader(view, validator_set_state.committed_validator_set()),
        if validator_set_state.update_decided() {
            None
        } else {
            Some(select_leader(
                view,
                validator_set_state.previous_validator_set(),
            ))
        },
    )
}
