/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of the HotStuff protocol for Byzantine Fault Tolerant State Machine Replication,
//! adapted for dynamic validator sets.
//! 
//! ## HotStuff for dynamic validator sets
//! 
//! The modified version of the HotStuff consensus protocol that HotStuff-rs implements consists of two
//! operating modes: a **Pipelined** mode, and a **Phased** mode.
//! 
//! ### Pipelined mode
//! 
//! The base protocol for consensus on blocks that do not update the validator set implements the
//! pipelined version of HotStuff, whereby when a replica votes for a block, it effectively votes 
//! for its ancestors too. Thus, the base protocol consists of exchanging [Proposal], [Vote], and
//! [NewView] messages, where the votes are from [Phase::Generic] phase. 
//! 
//! A view in this protocol generally proceeds as follows:
//! 1. The leader of the view proposes a block with its highestQC as the justify of the block.
//! 2. Replicas process the proposal: check if it is well-formed and cryptographically correct,
//!    query the [App] to validate the block, if so then they insert the block and apply the
//!    state updates associated with the block's justify. This may include updating the highestQC,
//!    updating the lockedQC, and committing a great-grandparent block. They also vote for the
//!    proposal.
//! 3. The next leader collects the votes into a QC, and saves it as its highestQC.
//! 
//! ### Phased mode
//! 
//! The pipelined version of HotStuff, although efficient, wouldn't be appropriate for blocks that have
//! associated validator set updates. This is because in a dynamic validator sets setting a desirable  
//! property is immediacy - if B is a block that updates the validator set from vs to vs', then its 
//! child shall be proposed and voted for by replicas from vs'.
//! 
//! To support dynamic validator sets with immediacy, the pipelined HotStuff consensus protocol is 
//! supplemented with a non-pipelined version of HotStuff with an additional "decide" phase.
//! This means that the voting for a validator-set-updating block proceeds in 4 phases, through
//! which the validators are only voting for the block, but not for its ancestors. If this 4-phase
//! consensus is interrupted, then the block has to be re-proposed, which guarantees safety.
//! 
//! The consensus phases for a validator-set-updating block B proceed as follows:
//! 1. "Prepare" phase: the leader broadcasts a proposal for B, replicas send [Phase::Prepare] votes.
//! 2. "Precommit" phase: the leader broadcasts a [Nudge] with a [Phase::Prepare] QC for B, replicas
//!     send [Phase::Precommit] votes.
//! 3. "Commit" phase: the leader broadcasts a [Nudge] with a [Phase::Precommit] QC for B, replicas
//!     send [Phase::Commit] votes.
//! 4. "Decide" phase: the leader broadcasts a [Nudge] with a [Phase::Commit] QC for B, replicas 
//!     send [Phase::Decide] votes.
//! 
//! The "decide" phase is special as it enforces a liveness-preserving transition between the two
//! validator sets. Concretely, on seeing a CommitQC, a new validator set becomes committed and its 
//! members vote "decide" for the block, with the goal of producing a DecideQC. However, in case 
//! they fail to do so, the resigning validator set is still active and ready to re-initiate the 
//! "decide" phase by broadcasting the commitQC if needed - the resigning validators only completely
//! de-activate themselves on seeing a DecideQC for the block. This guarantees the invariant that 
//! if a DecideQC exists, a quorum from the new validator set has committed the validator set update
//! and is ready to make progress.

use std::sync::mpsc::Sender;
use std::time::SystemTime;

use ed25519_dalek::VerifyingKey;

use crate::app::{App, ProduceBlockRequest, ProduceBlockResponse, ValidateBlockRequest, ValidateBlockResponse};
use crate::events::{CollectQCEvent, CommitBlockEvent, Event, InsertBlockEvent, NewViewEvent, 
                    NudgeEvent, ProposeEvent, PruneBlockEvent, ReceiveNewViewEvent, ReceiveNudgeEvent, 
                    ReceiveProposalEvent, ReceiveVoteEvent, StartViewEvent, UpdateHighestQCEvent, 
                    UpdateLockedQCEvent, UpdateValidatorSetEvent, VoteEvent};
use crate::hotstuff::voting::{is_proposer, is_voter, leaders};
use crate::messages::SignedMessage;
use crate:: networking::{Network, SenderHandle, ValidatorSetUpdateHandle};
use crate::pacemaker::protocol::ViewInfo;
use crate::state::block_tree::{BlockTree, BlockTreeError};
use crate::state::kv_store::KVStore;
use crate::state::safety::{self, repropose_block, safe_block, safe_nudge, safe_qc};
use crate::state::write_batch::BlockTreeWriteBatch;
use crate::types::basic::{BlockHeight, CryptoHash};
use crate::types::block::Block;
use crate::types::collectors::{Certificate, Collectors};
use crate::types::validators::{ValidatorSetState, ValidatorSetUpdates};
use crate::types::{
    basic::ChainID, 
    keypair::Keypair
};
use crate::hotstuff::messages::{Vote, HotStuffMessage, NewView, Nudge, Proposal};
use crate::hotstuff::types::{Phase, VoteCollector};

use super::types::QuorumCertificate;
use super::voting::vote_recipient;

/// An implementation of the HotStuff protocol (https://arxiv.org/abs/1803.05069), adapted to enable
/// dynamic validator sets. The protocol operates on a per-view basis, where in each view the validator
/// exchanges messages with other validators, and updates the [block tree][BlockTree].
pub(crate) struct HotStuff<N: Network> {
    config: HotStuffConfiguration,
    view_info: ViewInfo,
    view_status: ViewStatus,
    vote_collectors: Collectors<VoteCollector>,
    sender_handle: SenderHandle<N>,
    validator_set_update_handle: ValidatorSetUpdateHandle<N>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network> HotStuff<N> {

    /// Create a new HotStuff instance.
    pub(crate) fn new(
        config: HotStuffConfiguration,
        view_info: ViewInfo,
        sender_handle: SenderHandle<N>,
        validator_set_update_handle: ValidatorSetUpdateHandle<N>,
        init_validator_set_state: ValidatorSetState,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        let vote_collectors = 
            <Collectors<VoteCollector>>::new(config.chain_id, view_info.view, &init_validator_set_state);
        let view_status = ViewStatus::WaitingForProposal;
        Self { 
            config,
            view_info,
            view_status,
            vote_collectors,
            sender_handle,
            validator_set_update_handle, 
            event_publisher,
        }
    }

    /// Retrieve the view info currently used by the HotStuff protocol.
    pub(crate) fn view_info(&self) -> &ViewInfo {
        &self.view_info
    }

    /// On receiving new [ViewInfo] from the [Pacemaker](crate::pacemaker::protocol::Pacemaker): send messages
    /// and perform state updates associated with exiting the current view, update the local view info. 
    /// 
    /// This involves the following steps:
    /// 1. Exit the current view: send a [NewView] message to the leader of the next view.
    /// 2. Update the internal view info and view status, as well as the vote collectors. 
    /// 3. If serving as a leader of the newly entered view, propose or nudge.
    /// 
    /// ## Precondition
    /// The [Pacemaker](crate::pacemaker::protocol::Pacemaker) updated the view info.
    pub(crate) fn on_receive_view_info<K: KVStore>(
        &mut self, 
        view_info: ViewInfo,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) -> Result<(), HotStuffError> {

        let validator_set_state = block_tree.validator_set_state()?;

        // 1. Send a NewView message for the current view to the next leader(s).
        let new_view_msg = 
            HotStuffMessage::new_view(self.config.chain_id, self.view_info.view, block_tree.highest_qc()?);

        match leaders(self.view_info.view+1, &validator_set_state) {
            (committed_vs_leader, None) => self.sender_handle.send(committed_vs_leader, new_view_msg.clone()),
            (committed_vs_leader, Some(prev_vs_leader)) => {
                self.sender_handle.send(committed_vs_leader, new_view_msg.clone());
                self.sender_handle.send(prev_vs_leader, new_view_msg.clone())
            }
        }

        if let HotStuffMessage::NewView(new_view) = new_view_msg {
            Event::NewView(NewViewEvent{timestamp: SystemTime::now(), new_view}).publish(&self.event_publisher)
        }

        // 2. Update current view info and status.
        self.view_info = view_info;
        self.view_status = ViewStatus::WaitingForProposal;
        

        // 3. Update the vote collectors to collect votes for the updated view.
        self.vote_collectors = <Collectors<VoteCollector>>::new(self.config.chain_id, self.view_info.view, &validator_set_state);

        Event::StartView(StartViewEvent{timestamp: SystemTime::now(), view: self.view_info.view.clone()}).publish(&self.event_publisher);

        block_tree.set_highest_view_entered(self.view_info.view)?;

        // 4. If I am a proposer for this view then broadcast a nudge or a proposal.
        if is_proposer(&self.config.keypair.public(), self.view_info.view, &validator_set_state) {

            // Check if I need to re-propose an existing block. This may be required in case a chain of consecutive
            // views of voting for a validator-set-updating block proposed earlier has been interrupted.
            if let Some(block_hash) = repropose_block(self.view_info.view, block_tree)? {
                let block = 
                    block_tree.block(&block_hash)?
                    .ok_or(BlockTreeError::BlockExpectedButNotFound{block: block_hash})?;

                let proposal_msg = HotStuffMessage::proposal(self.config.chain_id, self.view_info.view, block);
                self.sender_handle.broadcast(proposal_msg);
                return Ok(())
            }

            // Otherwise, propose a new block or nudge based on highest_qc.
            let highest_qc = block_tree.highest_qc()?;
            match highest_qc.phase {
                // Produce a block proposal.
                Phase::Generic | Phase::Decide => {
                    let (parent_block, child_height) = if highest_qc.is_genesis_qc() {
                        (None, BlockHeight::new(0))
                    } else {
                        let parent_height = 
                            block_tree.block_height(&highest_qc.block)?
                            .ok_or(BlockTreeError::BlockExpectedButNotFound{block: highest_qc.block})?;
                        (
                            Some(highest_qc.block),
                            parent_height + 1,
                        )
                    };
                    
                    let produce_block_request = ProduceBlockRequest::new(
                        self.view_info.view,
                        parent_block,
                        block_tree.app_view(parent_block.as_ref())?,
                    );
        
                    let ProduceBlockResponse {
                        data,
                        data_hash,
                        app_state_updates: _,
                        validator_set_updates: _,
                    } = app.produce_block(produce_block_request);
        
                    let block = Block::new(child_height, highest_qc, data_hash, data);

                    let proposal_msg = HotStuffMessage::proposal(self.config.chain_id, self.view_info.view, block);

                    self.sender_handle.broadcast(proposal_msg.clone());

                    if let HotStuffMessage::Proposal(proposal) = proposal_msg {
                        Event::Propose(ProposeEvent{timestamp: SystemTime::now(), proposal}).publish(&self.event_publisher)
                    }

                },
                // Produce a nudge.
                Phase::Prepare | Phase::Precommit | Phase::Commit => {
                    let nudge_msg = HotStuffMessage::nudge(self.config.chain_id, self.view_info.view, highest_qc);

                    self.sender_handle.broadcast(nudge_msg.clone());

                    if let HotStuffMessage::Nudge(nudge) = nudge_msg {
                        Event::Nudge(NudgeEvent{timestamp: SystemTime::now(), nudge}).publish(&self.event_publisher)
                    }
                }
            }

        }

        Ok(())
    }

    /// Process the newly received message for the current view according to the protocol.
    pub(crate) fn on_receive_msg<K: KVStore>(
        &mut self, 
        msg: HotStuffMessage,
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) -> Result<(), HotStuffError> {

        // If nudge or proposal received, check if the sender is a proposer for this view,
        // and check if the replica is still accepting nudges and proposals. If the checks
        // fail, ignore the message.
        if msg.is_nudge() || msg.is_proposal() {
            let validator_set_state = block_tree.validator_set_state()?;

            if !is_proposer(origin, self.view_info.view, &validator_set_state) {
                return Ok(())
            }

            if self.view_status.is_leader_proposed(origin) || self.view_status.is_view_completed() {
                return Ok(())
            }

        }

        // If the check above passes, process the message.
        match msg {
            HotStuffMessage::Proposal(proposal) => self.on_receive_proposal(proposal, origin, block_tree, app),
            HotStuffMessage::Nudge(nudge) => self.on_receive_nudge(nudge, origin, block_tree),
            HotStuffMessage::Vote(vote) => self.on_receive_vote(vote, origin, block_tree),
            HotStuffMessage::NewView(new_view) => self.on_receive_new_view(new_view, origin, block_tree),
        }
    }

    /// Process the received proposal.
    fn on_receive_proposal<K: KVStore>(
        &mut self, 
        proposal: Proposal, 
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) -> Result<(), HotStuffError> {

        Event::ReceiveProposal(ReceiveProposalEvent{timestamp: SystemTime::now(), origin: *origin, proposal: proposal.clone()})
        .publish(&self.event_publisher);

        // 1. Check if block is correct and safe.
        if !proposal.block.is_correct(block_tree)? || !safe_block(&proposal.block, block_tree, self.config.chain_id)? {
            // Ensure that proposals or nudges from this leader should no longer be accepted in this view.
            match self.view_status {
                ViewStatus::WaitingForProposal => {
                    self.view_status = ViewStatus::LeaderProposed{leader: *origin}
                },
                ViewStatus::LeaderProposed{leader: _} => {
                    self.view_status = ViewStatus::ViewCompleted
                },
                _ => {}
            }
            return Ok(())
        }

        // 2. Validate the block, and insert if valid.
        let parent_block = if proposal.block.justify.is_genesis_qc() {
            None
        } else {
            Some(&proposal.block.justify.block)
        };
        let validate_block_request =
            ValidateBlockRequest::new(&proposal.block, block_tree.app_view(parent_block)?);
    
        if let ValidateBlockResponse::Valid {
            app_state_updates,
            validator_set_updates,
        } = app.validate_block(validate_block_request) {

            block_tree.insert_block(&proposal.block, app_state_updates.as_ref(), validator_set_updates.as_ref())?;
            Event::InsertBlock(InsertBlockEvent{timestamp: SystemTime::now(), block: proposal.block.clone()}).publish(&self.event_publisher);

            // 3. Trigger block tree updates: update highestQC, lock, commit.
            let committed_validator_set_updates = 
                update_block_tree(&proposal.block.justify, block_tree, &self.event_publisher)?;

            if let Some(vs_updates) = committed_validator_set_updates {
                self.validator_set_update_handle.update_validator_set(vs_updates)
            }

            // 4. Access the possibly updated validator set state, and update the vote collectors if needed.
            let validator_set_state = block_tree.validator_set_state()?;

            if self.vote_collectors.should_update_validator_sets(&validator_set_state) {
                self.vote_collectors.update_validator_sets(&validator_set_state)
            }

            // 5. Vote, if I am allowed to vote and if I haven't voted in this view yet.
            if is_voter(&self.config.keypair.public(), &validator_set_state, &proposal.block.justify) 
                && (block_tree.highest_view_voted()?.is_none() || block_tree.highest_view_voted()?.unwrap() < self.view_info.view) {

                    let vote_phase = if validator_set_updates.is_some() {
                        Phase::Prepare
                    } else {
                        Phase::Generic
                    };

                    let vote_msg = 
                        HotStuffMessage::vote(
                            &self.config.keypair,
                            self.config.chain_id, 
                            self.view_info.view, 
                            proposal.block.hash, 
                            vote_phase
                        );

                    if let HotStuffMessage::Vote(ref vote) = vote_msg {
                        let vote_recipient = vote_recipient(&vote, &validator_set_state);
                        self.sender_handle.send(vote_recipient, vote_msg.clone());
                        block_tree.set_highest_view_voted(self.view_info.view)?;
                        Event::Vote(VoteEvent{timestamp: SystemTime::now(), vote: vote.clone()}).publish(&self.event_publisher)
                    }
            }

        }

        // 6. Stop accepting proposals or nudges from this leader in this view.
        match self.view_status {
            ViewStatus::WaitingForProposal => {
                self.view_status = ViewStatus::LeaderProposed{leader: *origin}
            },
            ViewStatus::LeaderProposed{leader: _} => {
                self.view_status = ViewStatus::ViewCompleted
            },
            _ => {}
        }

        Ok(())
    }

    /// Process the received nudge.
    fn on_receive_nudge<K: KVStore>(
        &mut self, 
        nudge: Nudge, 
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) -> Result<(), HotStuffError> {

        Event::ReceiveNudge(ReceiveNudgeEvent{timestamp: SystemTime::now(), origin: *origin, nudge: nudge.clone()})
        .publish(&self.event_publisher);

        // 1. Check if the nudge is correct and safe.
        if !nudge.justify.is_correct(block_tree)? || !safe_nudge(&nudge, self.view_info.view, block_tree, self.config.chain_id)? {
            // Take note that proposals or nudges from this leader should no longer be accepted in this view.
            match self.view_status {
                ViewStatus::WaitingForProposal => {
                    self.view_status = ViewStatus::LeaderProposed{leader: *origin}
                },
                ViewStatus::LeaderProposed{leader: _} => {
                    self.view_status = ViewStatus::ViewCompleted
                },
                _ => {}
            }
            return Ok(())
        }

        // 2. Trigger block tree updates: update highestQC, lock, commit.
        let committed_validator_set_updates = 
            update_block_tree(&nudge.justify, block_tree, &self.event_publisher)?;

        if let Some(vs_updates) = committed_validator_set_updates {
            self.validator_set_update_handle.update_validator_set(vs_updates)
        }

        // 3. Access the possibly updated validator set state, and update the vote collectors if needed.
        let validator_set_state = block_tree.validator_set_state()?;

        if self.vote_collectors.should_update_validator_sets(&validator_set_state) {
            self.vote_collectors.update_validator_sets(&validator_set_state)
        }

        // 4. Vote, if I am allowed to vote and if I haven't voted in this view yet.
        if is_voter(&self.config.keypair.public(), &validator_set_state, &nudge.justify) 
            && (block_tree.highest_view_voted()?.is_none() || block_tree.highest_view_voted()?.unwrap() < self.view_info.view) {

                let vote_phase = match nudge.justify.phase {
                    Phase::Prepare => Phase::Precommit,
                    Phase::Precommit => Phase::Commit,
                    Phase::Commit => Phase::Decide,
                    _ => panic!() // Safety: if safe_nudge check passed this cannot be the case.
                };


                let vote_msg = 
                    HotStuffMessage::vote(
                        &self.config.keypair,
                        self.config.chain_id, 
                        self.view_info.view, 
                        nudge.justify.block, 
                        vote_phase
                    );

                if let HotStuffMessage::Vote(ref vote) = &vote_msg {
                    let vote_recipient = vote_recipient(&vote, &validator_set_state);
                    self.sender_handle.send(vote_recipient, vote_msg.clone());
                    block_tree.set_highest_view_voted(self.view_info.view)?;
                    Event::Vote(VoteEvent{timestamp: SystemTime::now(), vote: vote.clone()}).publish(&self.event_publisher);
                }
        }

        // 5. Stop accepting proposals or nudges from this leader in this view.
        match self.view_status {
            ViewStatus::WaitingForProposal => {
                self.view_status = ViewStatus::LeaderProposed{leader: *origin}
            },
            ViewStatus::LeaderProposed{leader: _} => {
                self.view_status = ViewStatus::ViewCompleted
            },
            _ => {}
        }

        Ok(())
    }

    /// Process the received vote.
    fn on_receive_vote<K: KVStore>(
        &mut self, 
        vote: Vote, 
        signer: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) -> Result<(), HotStuffError> {

        Event::ReceiveVote(ReceiveVoteEvent{timestamp: SystemTime::now(), origin: *signer, vote: vote.clone()})
        .publish(&self.event_publisher);

        // 1. Collect the vote if correct.
        if vote.is_correct(signer) {
            if let Some(new_qc) = self.vote_collectors.collect(signer, vote) {

                Event::CollectQC(CollectQCEvent{timestamp: SystemTime::now(), quorum_certificate: new_qc.clone()})
                .publish(&self.event_publisher);

                // If the newly collected QC is not correct or not safe, then ignore it and return.
                if !new_qc.is_correct(block_tree)? || !safe_qc(&new_qc, block_tree, self.config.chain_id)? {
                    return Ok(())
                }

                // 2. Trigger block tree updates: update highestQC, lock, commit (if new QC collected).
                let committed_validator_set_updates = 
                    update_block_tree(&new_qc, block_tree, &self.event_publisher)?;

                if let Some(vs_updates) = committed_validator_set_updates {
                    self.validator_set_update_handle.update_validator_set(vs_updates)
                }

                // 3. Access the possibly updated validator set state, and update the vote collectors if needed
                // (if new QC collected).
                let validator_set_state = block_tree.validator_set_state()?;

                if self.vote_collectors.should_update_validator_sets(&validator_set_state) {
                    self.vote_collectors.update_validator_sets(&validator_set_state)
                }
            }
        }
        
        Ok(())
    }

    /// Process the received NewView.
    fn on_receive_new_view<K: KVStore>(
        &mut self, 
        new_view: NewView, 
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) -> Result<(), HotStuffError> {

        Event::ReceiveNewView(ReceiveNewViewEvent{timestamp: SystemTime::now(), origin: *origin, new_view: new_view.clone()})
        .publish(&self.event_publisher);
        
        // 1. Check if the highest_qc in the NewView message is correct and safe.
        if new_view.highest_qc.is_correct(block_tree)? && safe_qc(&new_view.highest_qc, block_tree, self.config.chain_id)? {

            // 2. Trigger block tree updates: update highestQC, lock, commit (if new QC collected).
            let committed_validator_set_updates = 
                update_block_tree(&new_view.highest_qc, block_tree, &self.event_publisher)?;

            if let Some(vs_updates) = committed_validator_set_updates {
                self.validator_set_update_handle.update_validator_set(vs_updates)
            }

            // 3. Access the possibly updated validator set state, and update the vote collectors if needed
            // (if new QC collected).
            let validator_set_state = block_tree.validator_set_state()?;

            if self.vote_collectors.should_update_validator_sets(&validator_set_state) {
                self.vote_collectors.update_validator_sets(&validator_set_state)
            }
        }

        Ok(())
    }

}

/// Immutable parameters that define the behaviour of the [HotStuff] protocol and should never change.
#[derive(Clone)]
pub(crate) struct HotStuffConfiguration {
    pub(crate) chain_id: ChainID,
    pub(crate) keypair: Keypair,
}

#[derive(Debug)]
pub enum HotStuffError {
    BlockTreeError(BlockTreeError)
}

impl From<BlockTreeError> for HotStuffError {
    fn from(value: BlockTreeError) -> Self {
        HotStuffError::BlockTreeError(value)
    }
}

/// Captures the state of progress in a view. Keeping track of the `ViewStatus` is important for
/// ensuring that a leader can only propose once in a view - any subsequent proposals will be
/// ignored.
/// 
/// ## Variants
/// 
/// The `ViewStatus` can either be:
/// 1. [`WaitingForProposal`](ViewStatus::WaitingForProposal): No proposal or nudge was seen in this 
///    view so far. Proposals and nudges from a valid leader (proposer) can be accepted.
/// 2. [`LeaderProposed`](ViewStatus::LeaderProposed): The leader with a given public key has already
///    proposed or nudged. no more proposals or nudges from this leader can be accepted.
/// 3. [`ViewCompleted`](ViewStatus::ViewCompleted): The view is completed (for use during the validator
///    set update period). All leaders for the view have proposed or nudged, hence no more proposals or
///    nudges can be accepted in this view.
/// 
/// ## Persistence
/// 
/// Note that a variable of this type is stored in memory allocated to the program at runtime, rather
/// than persistent storage. This is because losing the information stored in `ViewStatus`, unlike
/// losing the information about the highest view the replica has voted in, cannot lead to safety
/// violations. In the worst case, it can only enable temporary liveness violations.
pub enum ViewStatus {
    WaitingForProposal,
    LeaderProposed{leader: VerifyingKey},
    ViewCompleted,
}

impl ViewStatus {
    /// Has this leader already proposed/nudged in the current view?
    fn is_leader_proposed(&self, leader: &VerifyingKey) -> bool {
        match self {
            ViewStatus::LeaderProposed{leader: validator} => {
                validator == leader
            },
            _ => false
        }
    }

    /// Have all (max. 2) leaders already proposed/nudged in this view?
    /// Note: this can evaluate to true only during the validator set update period.
    fn is_view_completed(&self) -> bool {
        matches!(self, ViewStatus::ViewCompleted)
    }
}

/// Performs the necessary block tree updates on seeing a safe [qc](QuorumCertificate) justifying a 
/// [nugde](Nudge) or a [block](crate::types::block::Block). These updates may include:
/// 1. Updating the highestQC,
/// 2. Updating the lockedQC,
/// 3. Commiting block(s).
/// 4. Setting the validator set updates associated with a block as completed.
/// 
/// Returns optional [validator set updates](crate::types::validators::ValidatorSetUpdates) caused by
/// committing a block. Note that even if multiple blocks are committed on calling this method, only max. 1
/// of them can have associated validator set updates. This is because validator-set-updating blocks are
/// committed via a non-pipelined protocol.
/// 
/// # Precondition
/// The block or nudge with this justify must satisfy [safety::safe_block] or [safety::safe_nudge] respectively.
fn update_block_tree<K: KVStore>(
    justify: &QuorumCertificate, 
    block_tree: &mut BlockTree<K>,
    event_publisher: &Option<Sender<Event>>) 
    -> Result<Option<ValidatorSetUpdates>, HotStuffError> 
{   
    let mut wb = BlockTreeWriteBatch::new();

    let mut update_locked_qc: Option<QuorumCertificate> = None;
    let mut update_highest_qc: Option<QuorumCertificate> = None;
    let mut committed_blocks: Vec<(CryptoHash, Option<ValidatorSetUpdates>)> = Vec::new();

    // 1. Update highestQC if needed.
    if justify.view > block_tree.highest_qc()?.view {
        wb.set_highest_qc(justify)?;
        update_highest_qc = Some(justify.clone())
    }

    // 2. Update lockedQC if needed.
    if let Some(new_locked_qc) = safety::qc_to_lock(justify, block_tree)? {
        wb.set_locked_qc(&new_locked_qc)?;
        update_locked_qc = Some(new_locked_qc)
    }

    // 3. Commit block(s) if needed.
    if let Some(block) = safety::block_to_commit(justify, block_tree)? {
        committed_blocks = block_tree.commit_block(&mut wb, &block)?;
    }

    // 4. Set validator set updates as completed if needed.
    if justify.phase.is_decide() {
        wb.set_validator_set_update_completed(true)?
    }

    block_tree.write(wb);

    publish_update_block_tree_events(event_publisher, update_highest_qc, update_locked_qc, &committed_blocks);

    // Safety: a block that updates the validator set must be followed by a block that contains a decide
    // qc. A block becomes committed immediately if its commitQC or decideQC is seen. Therefore, under normal
    // operation, at most 1 validator-set-updating block can be committed at a time.
    let resulting_vs_update = 
        committed_blocks.into_iter().rev()
        .find_map(|(_, validator_set_updates_opt)| validator_set_updates_opt);

    Ok(resulting_vs_update)
}


/// Publish all events resulting from calling [update_block_tree]. These events have to do with changing
/// persistent state, and  possibly include: [UpdateHighestQCEvent], [UpdateLockedQCEvent], 
/// [PruneBlockEvent], [CommitBlockEvent], [UpdateValidatorSetEvent].
/// 
/// Invariant: this method is invoked immediately after the corresponding changes are written to the [BlockTree].
fn publish_update_block_tree_events(
    event_publisher: &Option<Sender<Event>>,
    update_highest_qc: Option<QuorumCertificate>,
    update_locked_qc: Option<QuorumCertificate>,
    committed_blocks: &Vec<(CryptoHash, Option<ValidatorSetUpdates>)>
) {

    if let Some(highest_qc) = update_highest_qc {
        Event::UpdateHighestQC(UpdateHighestQCEvent { timestamp: SystemTime::now(), highest_qc}).publish(event_publisher)
    };

    if let Some(locked_qc) = update_locked_qc {
        Event::UpdateLockedQC(UpdateLockedQCEvent { timestamp: SystemTime::now(), locked_qc}).publish(event_publisher)
    };

    committed_blocks
    .iter()
    .for_each(|(b, validator_set_updates_opt)| {
        Event::PruneBlock(PruneBlockEvent { timestamp: SystemTime::now(), block: b.clone()}).publish(event_publisher);
        Event::CommitBlock(CommitBlockEvent { timestamp: SystemTime::now(), block: b.clone()}).publish(event_publisher);
        if let Some(validator_set_updates) = validator_set_updates_opt {
            Event::UpdateValidatorSet(UpdateValidatorSetEvent 
                { 
                    timestamp: SystemTime::now(),
                    cause_block: *b,
                    validator_set_updates: validator_set_updates.clone()
                }
            )
            .publish(event_publisher);
        }
    });
}
