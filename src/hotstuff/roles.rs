/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Functions that determine what roles a replica should play at any given View and Validator Set State.

use ed25519_dalek::VerifyingKey;

use crate::{
    pacemaker::implementation::select_leader,
    types::{data_types::ViewNumber, validator_set::ValidatorSetState},
};

use super::{
    messages::{NewView, PhaseVote},
    types::{Phase, PhaseCertificate},
};

/// Determine whether the `replica` is an "active" validator, given the current `validator_set_state`.
///
/// An active validator can:
/// - Propose/Nudge and phase vote in the HotStuff protocol,
/// - Contribute [timeout votes](crate::pacemaker::messages::TimeoutVote) and
///    [advance view messages](crate::pacemaker::messages::AdvanceView).
///
/// ## `is_validator` logic
///
/// Whether or not `replica` is an active validator given the current `validator_set_state` depends on
/// two factors:
/// 1. Whether `replica` is part of the Committed Validator Set (CVS), the Previous Validator Set (PVS),
///    both validator sets, or neither.
/// 2. Whether `validator_set_state.update_decided()` or not.
///
/// The below table specifies exactly the return value of `is_validator` in every possible combination
/// of the two factors:
///
/// ||Validator Set Update Decided|Validator Set Update Not Decided|
/// |---|---|---|
/// |Part of CVS (and maybe also PVS)|`true`|`true`|
/// |Part of PVS only|`false`|`true`|
/// |Part of neither neither CVS or PVS|`false`|`false`|
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

/// Determine whether `validator` should act as a proposer in the given `view`, given the current
/// `validator_set_state`.
///
/// ## `is_proposer` Logic
///
/// Whether or not `validator` is a proposer in `view` depends on two factors:
/// 1. Whether or not  `validator` is the [leader](select_leader) in either the CVS or the PVS in
///    `view` (it could possibly be in both), and
/// 2. Whether or not `validator_set_update.update_decided`.
///
/// The below table specifies exactly the return value of `is_proposer` in every possible combination
/// of the two factors:
///
/// ||Validator Set Update Decided|Validator Set Update Not Decided|
/// |---|---|---|
/// |Leader in CVS (and maybe also PVS)|`true`|`true`|
/// |Leader in PVS only|`false`|`true`|
/// |Leader in neither CVS or PVS|`false`|`false`|
pub(crate) fn is_proposer(
    validator: &VerifyingKey,
    view: ViewNumber,
    validator_set_state: &ValidatorSetState,
) -> bool {
    validator == &select_leader(view, validator_set_state.committed_validator_set())
        || (!validator_set_state.update_decided()
            && validator == &select_leader(view, validator_set_state.previous_validator_set()))
}

/// Determine whether or not `replica` should phase-vote for the `Proposal` or `Nudge` that `justify`
/// was taken from. This depends on whether `replica`'s `PhaseVote`s can become part of quorum
/// certificates that directly extend `justify`.
///
/// If this predicate evaluates to `false`, then it is fruitless to vote for the `Proposal` or `Nudge`
/// that `justify` was taken from, since according to the protocol, the next leader will ignore the
/// replica's phase votes anyway.
///
/// ## `is_phase_voter` Logic
///
/// `replica`'s phase vote can become part of PCs that directly extend `justify` if-and-only-if
/// `replica` is part of the appropriate validator set in the `validator_set_state`, which is either the
/// Committed Validator Set (CVS), or the Previous Validator Set (PVS). In turn, which of CVS and PVS is
/// the appropriate validator set depends on two factors:
/// 1. Whether or not `validator_set_update.update_decided()`, and
/// 2. What `justify.phase` is:
///
/// The below table specifies which validator set `replica` must be in order to be a voter in every
/// possible combination of the two factors:
///
/// ||Validator Set Update Decided|Validator Set Update Not Decided|
/// |---|---|---|
/// |Phase == `Generic`, `Prepare`, `Precommit`, or `Decide`|CVS|PVS|
/// |Phase == `Commit`|CVS|CVS|
///
/// ## Preconditions
///
/// `justify` satisfies [`safe_pc`](crate::block_tree::invariants::safe_pc) and
/// [`is_correct`](crate::types::signed_messages::Certificate::is_correct), and the block tree updates
/// associated with this `justify` have already been applied.
pub(crate) fn is_phase_voter(
    replica: &VerifyingKey,
    validator_set_state: &ValidatorSetState,
    justify: &PhaseCertificate,
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

/// Identify the leader that `phase_vote` should be sent to, given the current `validator_set_state`.
///
/// ## `phase_vote_recipient` Logic
///
/// The leader that `phase_vote` should be sent to is the leader of `phase_vote.view + 1` in the appropriate
/// validator set in `validator_set_state`, which is either the committed validator set (CVS) or the
/// previous validator set (PVS). Which of the CVS and the PVS is the appropriate validator set depends
/// on two factors: 1. Whether or not `validator_set_update.update_decided`, and 2. What `justify.phase`
/// is:
///
/// ||Validator Set Update Decided|Validator Set Update Not Decided|
/// |---|---|---|
/// |Phase == `Generic`, `Prepare`, `Precommit`, or `Commit`|CVS|PVS|
/// |Phase == `Decide`|CVS|CVS|
pub(crate) fn phase_vote_recipient(
    phase_vote: &PhaseVote,
    validator_set_state: &ValidatorSetState,
) -> VerifyingKey {
    if validator_set_state.update_decided() {
        select_leader(
            phase_vote.view + 1,
            validator_set_state.committed_validator_set(),
        )
    } else {
        match phase_vote.phase {
            Phase::Generic | Phase::Prepare | Phase::Precommit | Phase::Commit => select_leader(
                phase_vote.view + 1,
                validator_set_state.previous_validator_set(),
            ),
            Phase::Decide => select_leader(
                phase_vote.view + 1,
                validator_set_state.committed_validator_set(),
            ),
        }
    }
}

/// Identify the leader(s) that `new_view` should be sent to, given the current `validator_set_state`.
///
/// ## `new_view_recipients` Logic
///
/// Upon exiting a view, a replica should send a `new_view` message to the leader of `new_view.view + 1`
/// in the committed validator set (CVS), *and*, if `!validator_set_state.update_decided` (not decided),
/// *also* to the leader of the same view in the previous validator set (PVS).
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
