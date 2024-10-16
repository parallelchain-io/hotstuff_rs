//! Subprotocol for committing `Block`s.
//!
//! ## Operating modes
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
//! [`PhaseVote`](messages::PhaseVote), and [`NewView`](messages::NewView) messages, where the votes are
//! from the [`Generic`](types::Phase::Generic) phase.
//!
//! A view in the pipelined mode generally proceeds as follows:
//! 1. The leader of the view proposes a block with its highestPC as the justify of the block.
//! 2. Replicas process the proposal: check if it is well-formed and cryptographically correct, query
//!    the [`App`](crate::app::App) to validate the block, if so then they insert the block and apply
//!    the state updates associated with the block's justify. This may include updating the highestPC,
//!    updating the lockedPC, and committing a great-grandparent block. They also vote for the proposal.
//! 3. The next leader collects the votes into a PC, and saves it as its highestPC.
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
//! 2. "Precommit" phase: the leader broadcasts a [`Nudge`](messages::Nudge) with a `Prepare` PC for B,
//!    replicas send [`Precommit`](types::Phase::Precommit) votes.
//! 3. "Commit" phase: the leader broadcasts a `Nudge` with a `Precommit` PC for B, replicas send
//!    [`Commit`](types::Phase::Commit) votes.
//! 4. "Decide" phase: the leader broadcasts a `Nudge` with a `Commit` PC for B, replicas send
//!    [`Decide`](types::Phase::Decide) votes.
//!
//! The "Decide" phase is special as it enforces a liveness-preserving transition between the two
//! validator sets. Concretely, on seeing a CommitPC, a new validator set becomes committed and its
//! members vote "decide" for the block, with the goal of producing a DecidePC. However, in case
//! they fail to do so, the resigning validator set is still active and ready to re-initiate the
//! "decide" phase by broadcasting the commitPC if needed - the resigning validators only completely
//! de-activate themselves on seeing a DecidePC for the block. This guarantees the invariant that
//! if a DecidePC exists, a quorum from the new validator set has committed the validator set update
//! and is ready to make progress.
//!
//! ## Validator Set Update protocol
//!
//! HotStuff-rs' modified HotStuff SMR adds an extra "decide" phase to the phased HotStuff protocol.
//! This phase serves as the transition phase for the validator set update, and is associated with
//! the following invariant which implies the liveness as well as immediacy of a validator set update:
//!
//! > "If there exists a valid decidePC for a validator-set-updating block, then a quorum from the new
//! > validator set has committed the block together with the validator-set-update, and hence is ready
//! > to act as the newly committed validator set"
//!
//! Concretely, suppose B is a block that updates the validator set from vs to vs'. Once a commitPC
//! for B is seen, vs' becomes the committed validator set, and vs is stored as the previous validator
//! set, with the update being marked as incomplete. While the update is incomplete, vs may still be
//! active: it is allowed to re-broadcast the proposal for B or nudges for B. This is to account for the
//! possibility that due to asynchrony or byzantine behaviour not enough replicas may have received the
//! commitPC, and hence progressing to a succesful "decide" phase is not possible without such
//! re-broadcast. Once a valid decidePC signed by a quorum of validators from vs' is seen, the update is
//! marked as complete and vs becomes inactive.

pub mod messages;

pub mod types;

pub(crate) mod protocol;

pub(crate) mod specification;

pub(crate) mod roles;
