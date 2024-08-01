/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of a participant in the HotStuff subprotocol.

use std::iter::successors;
use std::sync::mpsc::Sender;
use std::time::SystemTime;

use ed25519_dalek::VerifyingKey;

use crate::app::{
    App, ProduceBlockRequest, ProduceBlockResponse, ValidateBlockRequest, ValidateBlockResponse,
};
use crate::events::{
    CollectQCEvent, CommitBlockEvent, Event, InsertBlockEvent, NewViewEvent, NudgeEvent,
    ProposeEvent, PruneBlockEvent, ReceiveNewViewEvent, ReceiveNudgeEvent, ReceiveProposalEvent,
    ReceiveVoteEvent, StartViewEvent, UpdateHighestQCEvent, UpdateLockedQCEvent,
    UpdateValidatorSetEvent, VoteEvent,
};
use crate::hotstuff::messages::{HotStuffMessage, NewView, Nudge, Proposal, Vote};
use crate::hotstuff::types::{Phase, VoteCollector};
use crate::hotstuff::voting::{is_proposer, is_voter, leaders};
use crate::messages::SignedMessage;
use crate::networking::{Network, SenderHandle, ValidatorSetUpdateHandle};
use crate::pacemaker::protocol::ViewInfo;
use crate::state::block_tree::{BlockTree, BlockTreeError};
use crate::state::kv_store::KVStore;
use crate::state::safety::{self, repropose_block, safe_block, safe_nudge, safe_qc};
use crate::state::write_batch::BlockTreeWriteBatch;
use crate::types::basic::{BlockHeight, CryptoHash};
use crate::types::block::Block;
use crate::types::collectors::{Certificate, Collectors};
use crate::types::validators::{ValidatorSetState, ValidatorSetUpdates, ValidatorSetUpdatesStatus};
use crate::types::{basic::ChainID, keypair::Keypair};

use super::types::QuorumCertificate;
use super::voting::vote_recipient;

/// A participant in the HotStuff subprotocol.
///
/// ## Usage
///
/// The `HotStuff` struct is meant to be used in an "event-oriented" fashion (note that "event" here
/// does not refer to the [`Event`] enum defined in the events module, but to "event" in the abstract
/// sense).
///
/// Reflecting this event-orientation, the two most significant crate-public methods of this struct are
/// "event handlers", which are to be called when specific things happen to the replica. These methods
/// are:
/// 1. [`enter_view`](Self::enter_view): called when
///    [`Pacemaker`](crate::pacemaker::protocol::Pacemaker) causes the replica to enter a new view.
/// 2. [`on_receive_msg`](Self::on_receive_msg): called when a new [`HotStuffMessage`](HotStuffMessage)
///    is received.
///
/// Besides these two, HotStuff has one more crate-public method, namely
/// [`is_view_outdated`](Self::is_view_outdated). This should be called after querying `Pacemaker` for
/// the latest [`ViewInfo`] to decide whether or not the `ViewInfo` is truly "new", that is, whether or
/// not it is more up-to-date than the latest `ViewInfo` the `HotStuff` struct has received through its
/// `enter_view` method.
pub(crate) struct HotStuff<N: Network> {
    config: HotStuffConfiguration,
    view_info: ViewInfo,
    proposal_status: ProposalStatus,
    vote_collectors: Collectors<VoteCollector>,
    sender_handle: SenderHandle<N>,
    validator_set_update_handle: ValidatorSetUpdateHandle<N>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network> HotStuff<N> {
    /// Create a new `HotStuff` participant.
    pub(crate) fn new(
        config: HotStuffConfiguration,
        view_info: ViewInfo,
        sender_handle: SenderHandle<N>,
        validator_set_update_handle: ValidatorSetUpdateHandle<N>,
        init_validator_set_state: ValidatorSetState,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        let vote_collectors = <Collectors<VoteCollector>>::new(
            config.chain_id,
            view_info.view,
            &init_validator_set_state,
        );
        let proposal_status = ProposalStatus::WaitingForProposal;
        Self {
            config,
            view_info,
            proposal_status,
            vote_collectors,
            sender_handle,
            validator_set_update_handle,
            event_publisher,
        }
    }

    /// Checks whether the HotStuff internal view is outdated with respect to the view from [`ViewInfo`] provided
    /// by the [`Pacemaker`](crate::pacemaker::protocol::Pacemaker).
    ///
    /// ## Next step
    ///
    /// If this function returns `true`, [`enter_view`](Self::enter_view) should be called with the latest `ViewInfo`
    /// at the soonest possible opportunity.
    pub(crate) fn is_view_outdated(&self, new_view_info: &ViewInfo) -> bool {
        new_view_info.view != self.view_info.view
    }

    /// On receiving a new [`ViewInfo`] from the [`Pacemaker`](crate::pacemaker::protocol::Pacemaker), send
    /// messages and perform state updates associated with exiting the current view, and update the local
    /// view info.
    ///
    /// ## Internal procedure
    ///
    /// This function executes the following steps:
    /// 1. Send a [`NewView`] message for the current view to the leader of the next view.
    /// 2. Update the internal view info, proposal status, and vote collectors to reflect `new_view_info`.
    /// 3. Set `highest_view_entered` in the block tree to the new view, then emit a `StartView` event.
    /// 4. If serving as a leader of the newly entered view, broadcast a `Proposal` or a `Nudge`.
    ///
    /// ## Precondition
    ///
    /// [`is_view_outdated`](Self::is_view_outdated) returns true. This is the case when the Pacemaker has updated
    /// `ViewInfo` but the update has not been made available to the [`HotStuff`] struct yet.
    pub(crate) fn enter_view<K: KVStore>(
        &mut self,
        new_view_info: ViewInfo,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) -> Result<(), HotStuffError> {
        let validator_set_state = block_tree.validator_set_state()?;

        // 1. Send a NewView message for the current view to the next leader(s).
        let new_view = NewView {
            chain_id: self.config.chain_id,
            view: self.view_info.view,
            highest_qc: block_tree.highest_qc()?,
        };

        match leaders(self.view_info.view + 1, &validator_set_state) {
            (committed_vs_leader, None) => self
                .sender_handle
                .send::<HotStuffMessage>(committed_vs_leader, new_view.clone().into()),
            (committed_vs_leader, Some(prev_vs_leader)) => {
                self.sender_handle
                    .send::<HotStuffMessage>(committed_vs_leader, new_view.clone().into());
                self.sender_handle
                    .send::<HotStuffMessage>(prev_vs_leader, new_view.clone().into());
            }
        }

        Event::NewView(NewViewEvent {
            timestamp: SystemTime::now(),
            new_view,
        })
        .publish(&self.event_publisher);

        // 2. Update the struct's internal view info, proposal status, and vote collectors to collect
        //    votes for the new view.
        self.view_info = new_view_info;
        self.proposal_status = ProposalStatus::WaitingForProposal;
        self.vote_collectors = <Collectors<VoteCollector>>::new(
            self.config.chain_id,
            self.view_info.view,
            &validator_set_state,
        );

        // 3. Set `highest_view_entered` in the block tree to the new view, then emit a `StartView` event.
        block_tree.set_highest_view_entered(self.view_info.view)?;

        Event::StartView(StartViewEvent {
            timestamp: SystemTime::now(),
            view: self.view_info.view.clone(),
        })
        .publish(&self.event_publisher);

        // 4. If I am a proposer for the new view, then broadcast a `Proposal` or a `Nudge`.
        if is_proposer(
            &self.config.keypair.public(),
            self.view_info.view,
            &validator_set_state,
        ) {
            // If a chain of consecutive views of voting for a validator-set-updating block has been interrupted, then
            // re-propose an existing block.
            if let Some(block_hash) = repropose_block(self.view_info.view, block_tree)? {
                let block = block_tree
                    .block(&block_hash)?
                    .ok_or(BlockTreeError::BlockExpectedButNotFound { block: block_hash })?;

                let proposal = Proposal {
                    chain_id: self.config.chain_id,
                    view: self.view_info.view,
                    block,
                };
                self.sender_handle
                    .broadcast::<HotStuffMessage>(proposal.clone().into());

                Event::Propose(ProposeEvent {
                    timestamp: SystemTime::now(),
                    proposal,
                })
                .publish(&self.event_publisher);

                return Ok(());
            }

            // Otherwise, propose a new block or nudge based on highest_qc.
            let highest_qc = block_tree.highest_qc()?;
            match highest_qc.phase {
                // Produce and broadcast a new Proposal.
                Phase::Generic | Phase::Decide => {
                    let (parent_block, child_height) = if highest_qc.is_genesis_qc() {
                        (None, BlockHeight::new(0))
                    } else {
                        let parent_height = block_tree.block_height(&highest_qc.block)?.ok_or(
                            BlockTreeError::BlockExpectedButNotFound {
                                block: highest_qc.block,
                            },
                        )?;
                        (Some(highest_qc.block), parent_height + 1)
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

                    let proposal = Proposal {
                        chain_id: self.config.chain_id,
                        view: self.view_info.view,
                        block,
                    };
                    self.sender_handle
                        .broadcast::<HotStuffMessage>(proposal.clone().into());

                    Event::Propose(ProposeEvent {
                        timestamp: SystemTime::now(),
                        proposal,
                    })
                    .publish(&self.event_publisher);
                }
                // Produce and broadcast a Nudge.
                Phase::Prepare | Phase::Precommit | Phase::Commit => {
                    let nudge = Nudge {
                        chain_id: self.config.chain_id,
                        view: self.view_info.view,
                        justify: highest_qc,
                    };

                    self.sender_handle
                        .broadcast::<HotStuffMessage>(nudge.clone().into());

                    Event::Nudge(NudgeEvent {
                        timestamp: SystemTime::now(),
                        nudge,
                    })
                    .publish(&self.event_publisher)
                }
            }
        }

        Ok(())
    }

    /// Process a newly received message for the current view according to the HotStuff subprotocol.
    ///
    /// ## Internal procedure
    ///
    /// This function executes the following steps:
    /// 1. If `msg` is a `Proposal` or a `Nudge`, check if the sender is a proposer for the current view
    ///    and check if the replica is still accepting nudges and proposals. If these checks fail, return
    ///    immediately.
    /// 2. If the checks pass, call one of the following 4 internal event handlers depending on the variant
    ///    of the received message:
    ///     - [`on_receive_proposal`](Self::on_receive_proposal).
    ///     - [`on_receive_nudge`](Self::on_receive_nudge).
    ///     - [`on_receive_vote`](Self::on_receive_vote).
    ///     - [`on_receive_new_view`](Self::on_receive_new_view).
    pub(crate) fn on_receive_msg<K: KVStore>(
        &mut self,
        msg: HotStuffMessage,
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) -> Result<(), HotStuffError> {
        // 1. If Proposal or Nudge received, check if the sender is a proposer for this view,
        // and check if the replica is still accepting nudges and proposals. If the checks
        // fail, ignore the message.
        if matches!(msg, HotStuffMessage::Proposal(_)) || matches!(msg, HotStuffMessage::Nudge(_)) {
            let validator_set_state = block_tree.validator_set_state()?;

            if !is_proposer(origin, self.view_info.view, &validator_set_state) {
                return Ok(());
            }

            if self.proposal_status.has_one_leader_proposed(origin)
                || self.proposal_status.have_all_leaders_proposed()
            {
                return Ok(());
            }
        }

        // 2. If the check above pass, process the message.
        match msg {
            HotStuffMessage::Proposal(proposal) => {
                self.on_receive_proposal(proposal, origin, block_tree, app)
            }
            HotStuffMessage::Nudge(nudge) => self.on_receive_nudge(nudge, origin, block_tree),
            HotStuffMessage::Vote(vote) => self.on_receive_vote(vote, origin, block_tree),
            HotStuffMessage::NewView(new_view) => {
                self.on_receive_new_view(new_view, origin, block_tree)
            }
        }
    }

    /// Process a newly received `proposal`.
    fn on_receive_proposal<K: KVStore>(
        &mut self,
        proposal: Proposal,
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) -> Result<(), HotStuffError> {
        Event::ReceiveProposal(ReceiveProposalEvent {
            timestamp: SystemTime::now(),
            origin: *origin,
            proposal: proposal.clone(),
        })
        .publish(&self.event_publisher);

        // 1. Check if block is correct and safe.
        if !proposal.block.is_correct(block_tree)?
            || !safe_block(&proposal.block, block_tree, self.config.chain_id)?
        {
            // Ensure that proposals or nudges from this leader should no longer be accepted in this view.
            match self.proposal_status {
                ProposalStatus::WaitingForProposal => {
                    self.proposal_status = ProposalStatus::OneLeaderProposed { leader: *origin }
                }
                ProposalStatus::OneLeaderProposed { leader: _ } => {
                    self.proposal_status = ProposalStatus::AllLeadersProposed
                }
                _ => {}
            }
            return Ok(());
        }

        // 2. Validate the block using the app, and insert it into the block tree if it is valid.
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
        } = app.validate_block(validate_block_request)
        {
            block_tree.insert_block(
                &proposal.block,
                app_state_updates.as_ref(),
                validator_set_updates.as_ref(),
            )?;
            Event::InsertBlock(InsertBlockEvent {
                timestamp: SystemTime::now(),
                block: proposal.block.clone(),
            })
            .publish(&self.event_publisher);

            // 3. Trigger block tree updates: update highestQC, lock, commit.
            let committed_validator_set_updates =
                update_block_tree(&proposal.block.justify, block_tree, &self.event_publisher)?;

            if let Some(vs_updates) = committed_validator_set_updates {
                self.validator_set_update_handle
                    .update_validator_set(vs_updates)
            }

            // 4. Access the possibly updated validator set state, and update the vote collectors if needed.
            let validator_set_state = block_tree.validator_set_state()?;

            let _ = self
                .vote_collectors
                .update_validator_sets(&validator_set_state);

            // 5. Vote, if I am allowed to vote and if I haven't voted in this view yet.
            if is_voter(
                &self.config.keypair.public(),
                &validator_set_state,
                &proposal.block.justify,
            ) && (block_tree.highest_view_voted()?.is_none()
                || block_tree.highest_view_voted()?.unwrap() < self.view_info.view)
            {
                let vote_phase = if validator_set_updates.is_some() {
                    Phase::Prepare
                } else {
                    Phase::Generic
                };

                let vote = Vote::new(
                    &self.config.keypair,
                    self.config.chain_id,
                    self.view_info.view,
                    proposal.block.hash,
                    vote_phase,
                );
                let vote_recipient = vote_recipient(&vote, &validator_set_state);
                self.sender_handle
                    .send::<HotStuffMessage>(vote_recipient, vote.clone().into());

                block_tree.set_highest_view_voted(self.view_info.view)?;
                Event::Vote(VoteEvent {
                    timestamp: SystemTime::now(),
                    vote: vote.clone(),
                })
                .publish(&self.event_publisher)
            }
        }

        // 6. Stop accepting proposals or nudges from this leader in this view.
        match self.proposal_status {
            ProposalStatus::WaitingForProposal => {
                self.proposal_status = ProposalStatus::OneLeaderProposed { leader: *origin }
            }
            ProposalStatus::OneLeaderProposed { leader: _ } => {
                self.proposal_status = ProposalStatus::AllLeadersProposed
            }
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
        Event::ReceiveNudge(ReceiveNudgeEvent {
            timestamp: SystemTime::now(),
            origin: *origin,
            nudge: nudge.clone(),
        })
        .publish(&self.event_publisher);

        // 1. Check if the nudge is correct and safe.
        if !nudge.justify.is_correct(block_tree)?
            || !safe_nudge(
                &nudge,
                self.view_info.view,
                block_tree,
                self.config.chain_id,
            )?
        {
            // Take note that proposals or nudges from this leader should no longer be accepted in this view.
            match self.proposal_status {
                ProposalStatus::WaitingForProposal => {
                    self.proposal_status = ProposalStatus::OneLeaderProposed { leader: *origin }
                }
                ProposalStatus::OneLeaderProposed { leader: _ } => {
                    self.proposal_status = ProposalStatus::AllLeadersProposed
                }
                _ => {}
            }
            return Ok(());
        }

        // 2. Trigger block tree updates: update highestQC, lock, commit.
        let committed_validator_set_updates =
            update_block_tree(&nudge.justify, block_tree, &self.event_publisher)?;

        if let Some(vs_updates) = committed_validator_set_updates {
            self.validator_set_update_handle
                .update_validator_set(vs_updates)
        }

        // 3. Access the possibly updated validator set state, and update the vote collectors if needed.
        let validator_set_state = block_tree.validator_set_state()?;

        let _ = self
            .vote_collectors
            .update_validator_sets(&validator_set_state);

        // 4. Vote, if I am allowed to vote and if I haven't voted in this view yet.
        if is_voter(
            &self.config.keypair.public(),
            &validator_set_state,
            &nudge.justify,
        ) && (block_tree.highest_view_voted()?.is_none()
            || block_tree.highest_view_voted()?.unwrap() < self.view_info.view)
        {
            let vote_phase = match nudge.justify.phase {
                Phase::Prepare => Phase::Precommit,
                Phase::Precommit => Phase::Commit,
                Phase::Commit => Phase::Decide,
                _ => unreachable!("if `safe_nudge` check passed then `vote_phase` should be either `Precommit`, `Commit`, or `Decide`"),
            };

            let vote = Vote::new(
                &self.config.keypair,
                self.config.chain_id,
                self.view_info.view,
                nudge.justify.block,
                vote_phase,
            );
            let vote_recipient = vote_recipient(&vote, &validator_set_state);
            self.sender_handle
                .send::<HotStuffMessage>(vote_recipient, vote.clone().into());

            block_tree.set_highest_view_voted(self.view_info.view)?;
            Event::Vote(VoteEvent {
                timestamp: SystemTime::now(),
                vote: vote.clone(),
            })
            .publish(&self.event_publisher);
        }

        // 5. Stop accepting proposals or nudges from this leader in this view.
        match self.proposal_status {
            ProposalStatus::WaitingForProposal => {
                self.proposal_status = ProposalStatus::OneLeaderProposed { leader: *origin }
            }
            ProposalStatus::OneLeaderProposed { leader: _ } => {
                self.proposal_status = ProposalStatus::AllLeadersProposed
            }
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
        Event::ReceiveVote(ReceiveVoteEvent {
            timestamp: SystemTime::now(),
            origin: *signer,
            vote: vote.clone(),
        })
        .publish(&self.event_publisher);

        // 1. Collect the vote if correct.
        if vote.is_correct(signer) {
            if let Some(new_qc) = self.vote_collectors.collect(signer, vote) {
                Event::CollectQC(CollectQCEvent {
                    timestamp: SystemTime::now(),
                    quorum_certificate: new_qc.clone(),
                })
                .publish(&self.event_publisher);

                // If the newly collected QC is not correct or not safe, then ignore it and return.
                if !new_qc.is_correct(block_tree)?
                    || !safe_qc(&new_qc, block_tree, self.config.chain_id)?
                {
                    return Ok(());
                }

                // 2. Trigger block tree updates: update highestQC, lock, commit (if new QC collected).
                let committed_validator_set_updates =
                    update_block_tree(&new_qc, block_tree, &self.event_publisher)?;

                if let Some(vs_updates) = committed_validator_set_updates {
                    self.validator_set_update_handle
                        .update_validator_set(vs_updates)
                }

                // 3. Access the possibly updated validator set state, and update the vote collectors if needed
                // (if new QC collected).
                let validator_set_state = block_tree.validator_set_state()?;

                let _ = self
                    .vote_collectors
                    .update_validator_sets(&validator_set_state);
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
        Event::ReceiveNewView(ReceiveNewViewEvent {
            timestamp: SystemTime::now(),
            origin: *origin,
            new_view: new_view.clone(),
        })
        .publish(&self.event_publisher);

        // 1. Check if the highest_qc in the NewView message is correct and safe.
        if new_view.highest_qc.is_correct(block_tree)?
            && safe_qc(&new_view.highest_qc, block_tree, self.config.chain_id)?
        {
            // 2. Trigger block tree updates: update highestQC, lock, commit (if new QC collected).
            let committed_validator_set_updates =
                update_block_tree(&new_view.highest_qc, block_tree, &self.event_publisher)?;

            if let Some(vs_updates) = committed_validator_set_updates {
                self.validator_set_update_handle
                    .update_validator_set(vs_updates)
            }

            // 3. Access the possibly updated validator set state, and update the vote collectors if needed
            // (if new QC collected).
            let validator_set_state = block_tree.validator_set_state()?;

            let _ = self
                .vote_collectors
                .update_validator_sets(&validator_set_state);
        }

        Ok(())
    }
}

/// Immutable parameters that define the behaviour of the [`HotStuff`] struct and should never change
/// after a replica starts.
#[derive(Clone)]
pub(crate) struct HotStuffConfiguration {
    pub(crate) chain_id: ChainID,
    pub(crate) keypair: Keypair,
}

/// The different ways calls to methods of the `HotStuff` struct can fail.
#[derive(Debug)]
pub enum HotStuffError {
    BlockTreeError(BlockTreeError),
}

impl From<BlockTreeError> for HotStuffError {
    fn from(value: BlockTreeError) -> Self {
        HotStuffError::BlockTreeError(value)
    }
}

/// Keeps track of the set of `Proposal`s that have been received by the `HotStuff` struct in the current
/// view.
///
/// Keeping track of `ProposalStatus` ensures that a leader can only propose once in a view -- any
/// subsequent proposals will be ignored.
///
/// ## Persistence
///
/// Note that a variable of this type is stored in memory allocated to the program at runtime, rather
/// than persistent storage. This is because losing the information stored in `ProposalStatus`, unlike
/// losing the information about the highest view the replica has voted in, does lead to safety
/// violations. In the worst case, it can only enable temporary liveness violations.
pub enum ProposalStatus {
    /// No proposal or nudge was seen in this view so far. Proposals and nudges from a valid leader
    /// (proposer) can be accepted.
    WaitingForProposal,

    /// The leader with a given public key has already proposed or nudged. No more proposals or nudges
    /// from this leader can be accepted.
    OneLeaderProposed { leader: VerifyingKey },

    /// All leaders for the view have proposed or nudged, hence no more proposals or nudges can be accepted
    /// in this view.
    AllLeadersProposed,
}

impl ProposalStatus {
    /// Has this leader already proposed/nudged in the current view?
    fn has_one_leader_proposed(&self, leader: &VerifyingKey) -> bool {
        match self {
            ProposalStatus::OneLeaderProposed { leader: validator } => validator == leader,
            _ => false,
        }
    }

    /// Have all (max. 2) leaders already proposed/nudged in this view?
    /// Note: this can evaluate to true only during the validator set update period.
    fn have_all_leaders_proposed(&self) -> bool {
        matches!(self, ProposalStatus::AllLeadersProposed)
    }
}

/// Commits a block and its ancestors if they have not been committed already.
///
/// Returns the hashes of the newly committed blocks, with the updates they caused to the validator set,
/// in sequence (from lowest height to highest height).
pub(crate) fn commit_block<K: KVStore>(
    block_tree: &mut BlockTree<K>,
    wb: &mut BlockTreeWriteBatch<K::WriteBatch>,
    block: &CryptoHash,
) -> Result<Vec<(CryptoHash, Option<ValidatorSetUpdates>)>, BlockTreeError> {
    // Obtain an iterator over the "block" and its ancestors, all the way until genesis, from newest ("block") to oldest.
    let blocks_iter = successors(Some(*block), |b| {
        block_tree
            .block_justify(b)
            .ok()
            .map(|qc| {
                if !qc.is_genesis_qc() {
                    Some(qc.block)
                } else {
                    None
                }
            })
            .flatten()
    });

    // Newest committed block height, we do not consider the blocks from this height downwards.
    let min_height = block_tree.highest_committed_block_height()?;

    // Obtain an iterator over the uncomitted blocks among "block" and its ancestors from oldest to newest,
    // the newest block being "block".
    // This is required because we want to commit blocks in correct order, applying updates from oldest to
    // newest.
    let uncommitted_blocks_iter = blocks_iter.take_while(|b| {
        min_height.is_none()
            || min_height.is_some_and(|h| block_tree.block_height(b).unwrap().unwrap() > h)
    });
    let uncommitted_blocks = uncommitted_blocks_iter.collect::<Vec<CryptoHash>>();
    let uncommitted_blocks_ordered_iter = uncommitted_blocks.iter().rev();

    // Helper closure that
    // (1) commits block b, applying all related updates to the write batch,
    // (2) extends the vector of blocks committed so far (accumulator) with b together with the optional
    //     validator set updates associated with b,
    // (3) returns the extended vector of blocks committed so far (updated accumulator).
    let commit = |committed_blocks_res: Result<
        Vec<(CryptoHash, Option<ValidatorSetUpdates>)>,
        BlockTreeError,
    >,
                  b: &CryptoHash|
     -> Result<Vec<(CryptoHash, Option<ValidatorSetUpdates>)>, BlockTreeError> {
        let mut committed_blocks = committed_blocks_res?;

        let block_height = block_tree
            .block_height(b)?
            .ok_or(BlockTreeError::BlockExpectedButNotFound { block: b.clone() })?;
        // Work steps:

        // Set block at height.
        wb.set_block_at_height(block_height, b)?;

        // Delete all of block's siblings.
        block_tree.delete_siblings(wb, b)?;

        // Apply pending app state updates.
        if let Some(pending_app_state_updates) = block_tree.pending_app_state_updates(b)? {
            wb.apply_app_state_updates(&pending_app_state_updates);
            wb.delete_pending_app_state_updates(b);
        }

        // Apply pending validator set updates.
        if let ValidatorSetUpdatesStatus::Pending(validator_set_updates) =
            block_tree.validator_set_updates_status(b)?
        {
            let mut committed_validator_set = block_tree.committed_validator_set()?;
            let previous_validator_set = committed_validator_set.clone();
            committed_validator_set.apply_updates(&validator_set_updates);

            wb.set_committed_validator_set(&committed_validator_set)?;
            wb.set_previous_validator_set(&previous_validator_set)?;
            wb.set_validator_set_update_block_height(block_height)?;
            wb.set_validator_set_update_decided(false)?;
            wb.set_committed_validator_set_updates(block)?;

            committed_blocks.push((*b, Some(validator_set_updates.clone())));
        } else {
            committed_blocks.push((*b, None));
        }

        // Update the highest committed block.
        wb.set_highest_committed_block(b)?;

        // Return the blocks committed so far together with their corresponding validator set updates.
        Ok(committed_blocks)
    };

    // Iterate over the uncommitted blocks from oldest to newest,
    // (1) applying related updates (by mutating the write batch), and
    // (2) building up the vector of committed blocks (by pushing the newely committed blocks to
    //     the accumulator vector).
    // Finally, return the accumulator.
    uncommitted_blocks_ordered_iter.fold(Ok(Vec::new()), commit)
}

/// Performs the necessary block tree updates on seeing a safe [qc](QuorumCertificate) justifying a
/// [nugde](Nudge) or a [block](crate::types::block::Block). These updates may include:
/// 1. Updating the highestQC,
/// 2. Updating the lockedQC,
/// 3. Commiting block(s).
/// 4. Setting the validator set updates associated with a block as decided.
///
/// Returns optional [validator set updates](crate::types::validators::ValidatorSetUpdates) caused by
/// committing a block. Note that even if multiple blocks are committed on calling this method, only max. 1
/// of them can have associated validator set updates. This is because validator-set-updating blocks are
/// committed via a non-pipelined protocol.
///
/// # Precondition
/// The block or nudge with this justify must satisfy [safety::safe_block] or [safety::safe_nudge] respectively.
pub(crate) fn update_block_tree<K: KVStore>(
    justify: &QuorumCertificate,
    block_tree: &mut BlockTree<K>,
    event_publisher: &Option<Sender<Event>>,
) -> Result<Option<ValidatorSetUpdates>, BlockTreeError> {
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
        committed_blocks = commit_block(block_tree, &mut wb, &block)?;
    }

    // 4. Set validator set updates as decided if needed.
    if justify.phase.is_decide() {
        wb.set_validator_set_update_decided(true)?
    }

    block_tree.write(wb);

    publish_update_block_tree_events(
        event_publisher,
        update_highest_qc,
        update_locked_qc,
        &committed_blocks,
    );

    // Safety: a block that updates the validator set must be followed by a block that contains a decide
    // qc. A block becomes committed immediately if its commitQC or decideQC is seen. Therefore, under normal
    // operation, at most 1 validator-set-updating block can be committed at a time.
    let resulting_vs_update = committed_blocks
        .into_iter()
        .rev()
        .find_map(|(_, validator_set_updates_opt)| validator_set_updates_opt);

    Ok(resulting_vs_update)
}

/// Publish all events resulting from calling [update_block_tree]. These events have to do with changing
/// persistent state, and  possibly include: [`UpdateHighestQCEvent`], [`UpdateLockedQCEvent`],
/// [`PruneBlockEvent`], [`CommitBlockEvent`], [`UpdateValidatorSetEvent`].
///
/// Invariant: this method is invoked immediately after the corresponding changes are written to the [`BlockTree`].
fn publish_update_block_tree_events(
    event_publisher: &Option<Sender<Event>>,
    update_highest_qc: Option<QuorumCertificate>,
    update_locked_qc: Option<QuorumCertificate>,
    committed_blocks: &Vec<(CryptoHash, Option<ValidatorSetUpdates>)>,
) {
    if let Some(highest_qc) = update_highest_qc {
        Event::UpdateHighestQC(UpdateHighestQCEvent {
            timestamp: SystemTime::now(),
            highest_qc,
        })
        .publish(event_publisher)
    };

    if let Some(locked_qc) = update_locked_qc {
        Event::UpdateLockedQC(UpdateLockedQCEvent {
            timestamp: SystemTime::now(),
            locked_qc,
        })
        .publish(event_publisher)
    };

    committed_blocks
        .iter()
        .for_each(|(b, validator_set_updates_opt)| {
            Event::PruneBlock(PruneBlockEvent {
                timestamp: SystemTime::now(),
                block: b.clone(),
            })
            .publish(event_publisher);
            Event::CommitBlock(CommitBlockEvent {
                timestamp: SystemTime::now(),
                block: b.clone(),
            })
            .publish(event_publisher);
            if let Some(validator_set_updates) = validator_set_updates_opt {
                Event::UpdateValidatorSet(UpdateValidatorSetEvent {
                    timestamp: SystemTime::now(),
                    cause_block: *b,
                    validator_set_updates: validator_set_updates.clone(),
                })
                .publish(event_publisher);
            }
        });
}
