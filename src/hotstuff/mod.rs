//! A version of the [HotStuff](https://arxiv.org/abs/1803.05069) protocol for Byzantine Fault Tolerant
//! State Machine Replication, with adaptations designed to enable dynamically-changing validator sets.
//!
//! ## HotStuff for dynamic validator sets
//!
//! The modified version of the HotStuff consensus protocol that HotStuff-rs implements dynamically
//! switches between two operating modes:
//! 1. [**Pipelined mode**](#pipelined-mode): used to commit non-validator-set-updating blocks.
//! 2. [**Phased mode**](#phased-mode): used to commit validator-set-updating blocks.
//!
//! ### Pipelined mode
//!
//! To commit blocks that do not update the validator set, HotStuff-rs uses the pipelined version of
//! HotStuff, whereby when a replica votes for a block, it effectively votes for its ancestors too. The
//! pipelined version of HotStuff consists of exchanging [`Proposal`](messages::Proposal),
//! [`Vote`](messages::Vote), and [`NewView`](messages::NewView) messages, where the votes are from the
//! [`Generic`](types::Phase::Generic) phase.
//!
//! A view in the pipelined mode generally proceeds as follows:
//! 1. The leader of the view proposes a block with its highestQC as the justify of the block.
//! 2. Replicas process the proposal: check if it is well-formed and cryptographically correct, query
//!    the [`App`](crate::app::App) to validate the block, if so then they insert the block and apply
//!    the state updates associated with the block's justify. This may include updating the highestQC,
//!    updating the lockedQC, and committing a great-grandparent block. They also vote for the proposal.
//! 3. The next leader collects the votes into a QC, and saves it as its highestQC.
//!
//! ### Phased mode
//!
//! The pipelined version of HotStuff, although efficient, wouldn't be appropriate for blocks that have
//! associated validator set updates. This is because in a dynamic validator sets setting a desirable  
//! property is **immediacy** -- if B is a block that updates the validator set from vs to vs', then its
//! child shall be proposed and voted for by replicas from vs'.
//!
//! To support dynamic validator sets with immediacy, the pipelined HotStuff consensus protocol is
//! supplemented with a "phased" version of HotStuff with an additional "decide" phase. This means that
//! the voting for a validator-set-updating block proceeds in 4 phases, through which the validators are
//! only voting for the block, but not for its ancestors. If this 4-phase consensus is interrupted, then
//! the block has to be re-proposed, which guarantees safety.
//!
//! The consensus phases for a validator-set-updating block B proceed as follows:
//! 1. "Prepare" phase: the leader broadcasts a proposal for B, replicas send
//!     [`Prepare`](types::Phase::Prepare) votes.
//! 2. "Precommit" phase: the leader broadcasts a [`Nudge`](messages::Nudge) with a `Prepare` QC for B,
//!    replicas send [`Precommit`](types::Phase::Precommit) votes.
//! 3. "Commit" phase: the leader broadcasts a `Nudge` with a `Precommit` QC for B, replicas send
//!    [`Commit`](types::Phase::Commit) votes.
//! 4. "Decide" phase: the leader broadcasts a `Nudge` with a `Commit` QC for B, replicas send
//!    [`Decide`](types::Phase::Decide) votes.
//!
//! The "Decide" phase is special as it enforces a liveness-preserving transition between the two
//! validator sets. Concretely, on seeing a CommitQC, a new validator set becomes committed and its
//! members vote "decide" for the block, with the goal of producing a DecideQC. However, in case
//! they fail to do so, the resigning validator set is still active and ready to re-initiate the
//! "decide" phase by broadcasting the commitQC if needed - the resigning validators only completely
//! de-activate themselves on seeing a DecideQC for the block. This guarantees the invariant that
//! if a DecideQC exists, a quorum from the new validator set has committed the validator set update
//! and is ready to make progress.

pub mod messages;

pub mod types;

pub(crate) mod protocol;

pub(crate) mod voting;
