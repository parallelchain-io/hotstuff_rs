/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Methods used to enforce who is allowed to propose and vote at any given point. The methods implement
//! the key rules of the hotstuff-rs validator set update protocol.
//!
//! ## Validator Set Update protocol
//! hotstuff-rs adds an extra "decide" phase to the phased HotStuff protocol. This phase serves as the
//! transition phase for the validator set update, and is associated with the following invariant
//! which implies the liveness as well as immediacy of a validator set update:
//!
//! "If there exists a valid decideQC for a validator-set-updating block, then a quorum from the new
//! validator set has committed the block together with the validator-set-update, and hence is ready
//! to act as the newly committed validator set"
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

/// Returns whether the replica with a given public key is allowed to act as the proposer for a
/// given view.
///
/// Usually, the proposer is the leader of the committed validator set. However, during the validator
/// set update period the leader of the previous validator set can also act as the proposer.
pub(crate) fn is_proposer(
    validator: &VerifyingKey,
    view: ViewNumber,
    validator_set_state: &ValidatorSetState,
) -> bool {
    validator == &select_leader(view, validator_set_state.committed_validator_set())
        || (!validator_set_state.update_completed()
            && validator == &select_leader(view, validator_set_state.previous_validator_set()))
}

/// Returns the public key of the replica tasked with receiving and collecting a given vote.
///
/// Usually, the recipient of a vote is the leader of the committed validator set for the subsequent
/// view. However, during the validator set update period the leader of the previous validator set
/// for the next view is tasked with collecting all kinds of votes other than decide-phase votes, which
/// are addressed to the next leader of the committed validator set.
pub(crate) fn vote_recipient(vote: &Vote, validator_set_state: &ValidatorSetState) -> VerifyingKey {
    if validator_set_state.update_completed() {
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

/// Returns whether the replica with a given public key is allowed to vote for a nudge or proposal with
/// a given justify.
///
/// Usually, a replica is allowed to vote if it belongs to the committed validator set. However,
/// during the validator set update period replicas from the previous validator set are also
/// allowed to vote for nudges and proposals for the validator-set-updating block, other than the
/// "decide" phase nudge containing a commitQC. This is a fallback mechanism in case only selected
/// replicas enter the validator set update period.
///
/// # Precondition
/// justify satisfies [safe_qc](crate::state::safety::safe_qc) and
/// [is_correct](crate::hotstuff::types::QuorumCertificate::is_correct), and the block tree updates
/// associated with this justify have already been applied.
pub(crate) fn is_voter(
    validator: &VerifyingKey,
    validator_set_state: &ValidatorSetState,
    justify: &QuorumCertificate,
) -> bool {
    if validator_set_state.update_completed() {
        validator_set_state
            .committed_validator_set()
            .contains(&validator)
    } else {
        match justify.phase {
            Phase::Generic | Phase::Prepare | Phase::Precommit | Phase::Decide => {
                validator_set_state
                    .previous_validator_set()
                    .contains(&validator)
            }
            Phase::Commit => validator_set_state
                .committed_validator_set()
                .contains(&validator),
        }
    }
}

/// Returns whether the replica with a given public key is an active validator. An active
/// validator can:
/// 1. Propose/nudge and vote in the HotStuff protocol under certain circumstances described above,
/// 2. Contribute [timeout votes](crate::pacemaker::messages::TimeoutVote) and
///    [advance view messages](crate::pacemaker::messages::AdvanceView).
///
/// In general, memebers of the committed validator set are always active, but during the validator
/// set update period members of the previous validator set are active too.
pub(crate) fn is_validator(
    verifying_key: &VerifyingKey,
    validator_set_state: &ValidatorSetState,
) -> bool {
    validator_set_state
        .committed_validator_set()
        .contains(verifying_key)
        || (!validator_set_state.update_completed()
            && validator_set_state
                .previous_validator_set()
                .contains(verifying_key))
}

/// Returns the leader(s) of a given view. In general, a view has only one leader, but during the
/// validator set update period the leader of the resigning validator set can act as a leader too.
///
/// ## Return value
///
/// Returns a pair with the following items:
/// 1. `VerifyingKey`: the leader in the committed validator set in the specified view.
/// 2. `Option<VerifyingKey>`: the leader in the resigning validator set in the specified view (`None`
///     if the most recently initiated validator set update has been completed).
pub(crate) fn leaders(
    view: ViewNumber,
    validator_set_state: &ValidatorSetState,
) -> (VerifyingKey, Option<VerifyingKey>) {
    (
        select_leader(view, validator_set_state.committed_validator_set()),
        if validator_set_state.update_completed() {
            None
        } else {
            Some(select_leader(
                view,
                validator_set_state.previous_validator_set(),
            ))
        },
    )
}
