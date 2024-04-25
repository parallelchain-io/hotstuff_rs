/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Methods used to enforce who is allowed to propose and vote at any given point.

use ed25519_dalek::VerifyingKey;

use crate::pacemaker::protocol::select_leader;
use crate::types::{
    basic::ViewNumber, 
    validators::ValidatorSetState
};

use super::types::{Phase, QuorumCertificate};

/// Returns whether the replica with a given public key is allowed to serve as the proposer for a 
/// given view.
/// 
/// Usually, the proposer is the leader of the committed validator set. However, during the validator
/// set update period the leader of the previous validator set can also act as the proposer.
pub fn is_proposer(validator: VerifyingKey, view: ViewNumber, validator_set_state: ValidatorSetState) -> bool {
    if validator_set_state.update_complete() {
        validator == select_leader(view, validator_set_state.committed_validator_set())
    } else {
        validator == select_leader(view, validator_set_state.committed_validator_set()) ||
        validator == select_leader(view, validator_set_state.previous_validator_set())
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
pub fn is_voter(validator: VerifyingKey, validator_set_state: ValidatorSetState, justify: QuorumCertificate) -> bool {
    if validator_set_state.update_complete() {
        validator_set_state.committed_validator_set().contains(&validator)
    } else {
        match justify.phase {
            Phase::Generic | Phase::Prepare | Phase::Precommit | Phase::Decide => 
                validator_set_state.previous_validator_set().contains(&validator),
            Phase::Commit =>
                validator_set_state.committed_validator_set().contains(&validator)
        }
    }
}

