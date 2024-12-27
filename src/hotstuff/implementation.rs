/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Event-driven implementation of the HotStuff subprotocol, as specified in
//! [`sequence_flow`](super::sequence_flow).
//!
//! Main type: [`HotStuff`].

use std::{sync::mpsc::Sender, time::SystemTime};

use ed25519_dalek::VerifyingKey;

use crate::{
    app::{
        App, ProduceBlockRequest, ProduceBlockResponse, ValidateBlockRequest, ValidateBlockResponse,
    },
    block_tree::{
        accessors::internal::{BlockTreeError, BlockTreeSingleton},
        invariants::{repropose_block, safe_block, safe_nudge, safe_pc},
        pluggables::KVStore,
    },
    events::{
        CollectPCEvent, Event, InsertBlockEvent, NewViewEvent, NudgeEvent, PhaseVoteEvent,
        ProposeEvent, ReceiveNewViewEvent, ReceiveNudgeEvent, ReceivePhaseVoteEvent,
        ReceiveProposalEvent, StartViewEvent,
    },
    hotstuff::{
        messages::{HotStuffMessage, NewView, Nudge, PhaseVote, Proposal},
        roles::{is_phase_voter, is_proposer, new_view_recipients},
        types::{Phase, PhaseVoteCollector},
    },
    networking::{
        network::{Network, ValidatorSetUpdateHandle},
        sending::SenderHandle,
    },
    pacemaker::implementation::ViewInfo,
    types::{
        block::Block,
        crypto_primitives::Keypair,
        data_types::{BlockHeight, ChainID},
        signed_messages::{ActiveCollectorPair, Certificate, SignedMessage},
        validator_set::ValidatorSetState,
    },
};

use super::roles::phase_vote_recipient;

/// A single participant in the HotStuff subprotocol.
///
/// # Usage
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
/// 2. [`on_receive_msg`](Self::on_receive_msg): called when a new [`HotStuffMessage`] is received.
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
    phase_vote_collectors: ActiveCollectorPair<PhaseVoteCollector>,
    sender_handle: SenderHandle<N>,
    validator_set_update_handle: ValidatorSetUpdateHandle<N>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network> HotStuff<N> {
    /// Create a new HotStuff subprotocol participant.
    pub(crate) fn new(
        config: HotStuffConfiguration,
        view_info: ViewInfo,
        sender_handle: SenderHandle<N>,
        validator_set_update_handle: ValidatorSetUpdateHandle<N>,
        init_validator_set_state: ValidatorSetState,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        let phase_vote_collectors = <ActiveCollectorPair<PhaseVoteCollector>>::new(
            config.chain_id,
            view_info.view,
            &init_validator_set_state,
        );
        let proposal_status = ProposalStatus::WaitingForProposal;
        Self {
            config,
            view_info,
            proposal_status,
            phase_vote_collectors,
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
    /// # Precondition
    ///
    /// [`is_view_outdated`](Self::is_view_outdated) returns true. This is the case when the Pacemaker has updated
    /// `ViewInfo` but the update has not been made available to the [`HotStuff`] struct yet.
    ///
    /// # Specification
    ///
    /// [Enter View](super::sequence_flow#enter-view).
    pub(crate) fn enter_view<K: KVStore>(
        &mut self,
        new_view_info: ViewInfo,
        block_tree: &mut BlockTreeSingleton<K>,
        app: &mut impl App<K>,
    ) -> Result<(), HotStuffError> {
        let validator_set_state = block_tree.validator_set_state()?;

        // 1. Send a NewView message for the current view to the next leader(s).
        let new_view = NewView {
            chain_id: self.config.chain_id,
            view: self.view_info.view,
            highest_pc: block_tree.highest_pc()?,
        };

        match new_view_recipients(&new_view, &validator_set_state) {
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
        self.phase_vote_collectors = <ActiveCollectorPair<PhaseVoteCollector>>::new(
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

            // Otherwise, propose a new block or nudge based on highest_pc.
            let highest_pc = block_tree.highest_pc()?;
            match highest_pc.phase {
                // Produce and broadcast a new Proposal.
                Phase::Generic | Phase::Decide => {
                    let (parent_block, child_height) = if highest_pc.is_genesis_pc() {
                        (None, BlockHeight::new(0))
                    } else {
                        let parent_height = block_tree.block_height(&highest_pc.block)?.ok_or(
                            BlockTreeError::BlockExpectedButNotFound {
                                block: highest_pc.block,
                            },
                        )?;
                        (Some(highest_pc.block), parent_height + 1)
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

                    let block = Block::new(child_height, highest_pc, data_hash, data);

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
                        justify: highest_pc,
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
    ///     - [`on_receive_phase_vote`](Self::on_receive_phase_vote).
    ///     - [`on_receive_new_view`](Self::on_receive_new_view).
    pub(crate) fn on_receive_msg<K: KVStore>(
        &mut self,
        msg: HotStuffMessage,
        origin: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
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
            HotStuffMessage::PhaseVote(vote) => {
                self.on_receive_phase_vote(vote, origin, block_tree)
            }
            HotStuffMessage::NewView(new_view) => {
                self.on_receive_new_view(new_view, origin, block_tree)
            }
        }
    }

    /// Process a newly received `proposal`.
    ///
    /// # Preconditions
    ///
    /// [`is_proposer(origin, self.view_info.view, &block_tree.validator_set_state()?)`](is_proposer).
    ///
    /// # Specification
    ///
    /// [On Receive Proposal](super::sequence_flow#on-receive-proposal).
    fn on_receive_proposal<K: KVStore>(
        &mut self,
        proposal: Proposal,
        origin: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
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
        let parent_block = if proposal.block.justify.is_genesis_pc() {
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
            block_tree.insert(
                &proposal.block,
                app_state_updates.as_ref(),
                validator_set_updates.as_ref(),
            )?;
            Event::InsertBlock(InsertBlockEvent {
                timestamp: SystemTime::now(),
                block: proposal.block.clone(),
            })
            .publish(&self.event_publisher);

            // 3. Trigger block tree updates: update highestPC, lock, commit.
            let committed_validator_set_updates =
                block_tree.update(&proposal.block.justify, &self.event_publisher)?;

            if let Some(vs_updates) = committed_validator_set_updates {
                self.validator_set_update_handle
                    .update_validator_set(vs_updates)
            }

            // 4. Access the possibly updated validator set state, and update the vote collectors if needed.
            let validator_set_state = block_tree.validator_set_state()?;

            let _ = self
                .phase_vote_collectors
                .update_validator_sets(&validator_set_state);

            // 5. Vote, if I am allowed to vote and if I haven't voted in this view yet.
            if is_phase_voter(
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

                let phase_vote = PhaseVote::new(
                    &self.config.keypair,
                    self.config.chain_id,
                    self.view_info.view,
                    proposal.block.hash,
                    vote_phase,
                );
                let vote_recipient = phase_vote_recipient(&phase_vote, &validator_set_state);
                self.sender_handle
                    .send::<HotStuffMessage>(vote_recipient, phase_vote.clone().into());

                block_tree.set_highest_view_phase_voted(self.view_info.view)?;
                Event::PhaseVote(PhaseVoteEvent {
                    timestamp: SystemTime::now(),
                    vote: phase_vote.clone(),
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
    ///
    /// # Preconditions
    ///
    /// [`is_proposer(origin, self.view_info.view, &block_tree.validator_set_state()?)`](is_proposer).
    ///
    /// # Specification
    ///
    /// [On Receive Nudge](super::sequence_flow#on-receive-nudge).
    fn on_receive_nudge<K: KVStore>(
        &mut self,
        nudge: Nudge,
        origin: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
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

        // 2. Trigger block tree updates: update highestPC, lock, commit.
        let committed_validator_set_updates =
            block_tree.update(&nudge.justify, &self.event_publisher)?;

        if let Some(vs_updates) = committed_validator_set_updates {
            self.validator_set_update_handle
                .update_validator_set(vs_updates)
        }

        // 3. Access the possibly updated validator set state, and update the vote collectors if needed.
        let validator_set_state = block_tree.validator_set_state()?;

        let _ = self
            .phase_vote_collectors
            .update_validator_sets(&validator_set_state);

        // 4. Vote, if I am allowed to vote and if I haven't voted in this view yet.
        if is_phase_voter(
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

            let vote = PhaseVote::new(
                &self.config.keypair,
                self.config.chain_id,
                self.view_info.view,
                nudge.justify.block,
                vote_phase,
            );
            let vote_recipient = phase_vote_recipient(&vote, &validator_set_state);
            self.sender_handle
                .send::<HotStuffMessage>(vote_recipient, vote.clone().into());

            block_tree.set_highest_view_phase_voted(self.view_info.view)?;
            Event::PhaseVote(PhaseVoteEvent {
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

    /// Process a received `phase_vote`.
    ///
    /// # Specification
    ///
    /// [On Receive Phase Vote](super::sequence_flow#on-receive-phase-vote).
    fn on_receive_phase_vote<K: KVStore>(
        &mut self,
        phase_vote: PhaseVote,
        signer: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
    ) -> Result<(), HotStuffError> {
        Event::ReceivePhaseVote(ReceivePhaseVoteEvent {
            timestamp: SystemTime::now(),
            origin: *signer,
            phase_vote: phase_vote.clone(),
        })
        .publish(&self.event_publisher);

        // 1. Collect the vote if correct.
        if phase_vote.is_correct(signer) {
            if let Some(new_pc) = self.phase_vote_collectors.collect(signer, phase_vote) {
                Event::CollectPC(CollectPCEvent {
                    timestamp: SystemTime::now(),
                    phase_certificate: new_pc.clone(),
                })
                .publish(&self.event_publisher);

                // If the newly collected PC is not correct or not safe, then ignore it and return.
                if !new_pc.is_correct(block_tree)?
                    || !safe_pc(&new_pc, block_tree, self.config.chain_id)?
                {
                    return Ok(());
                }

                // 2. Trigger block tree updates: update highestPC, lock, commit (if new PC collected).
                let committed_validator_set_updates =
                    block_tree.update(&new_pc, &self.event_publisher)?;

                if let Some(vs_updates) = committed_validator_set_updates {
                    self.validator_set_update_handle
                        .update_validator_set(vs_updates)
                }

                // 3. Access the possibly updated validator set state, and update the vote collectors if needed
                // (if new PC collected).
                let validator_set_state = block_tree.validator_set_state()?;

                let _ = self
                    .phase_vote_collectors
                    .update_validator_sets(&validator_set_state);
            }
        }

        Ok(())
    }

    /// Process the received NewView.
    ///
    /// # Specification
    ///
    /// [On Receive New View](super::sequence_flow#on-receive-new-view).
    fn on_receive_new_view<K: KVStore>(
        &mut self,
        new_view: NewView,
        origin: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
    ) -> Result<(), HotStuffError> {
        Event::ReceiveNewView(ReceiveNewViewEvent {
            timestamp: SystemTime::now(),
            origin: *origin,
            new_view: new_view.clone(),
        })
        .publish(&self.event_publisher);

        // 1. Check if the highest_pc in the NewView message is correct and safe.
        if new_view.highest_pc.is_correct(block_tree)?
            && safe_pc(&new_view.highest_pc, block_tree, self.config.chain_id)?
        {
            // 2. Trigger block tree updates: update highestPC, lock, commit (if new PC collected).
            let committed_validator_set_updates =
                block_tree.update(&new_view.highest_pc, &self.event_publisher)?;

            if let Some(vs_updates) = committed_validator_set_updates {
                self.validator_set_update_handle
                    .update_validator_set(vs_updates)
            }

            // 3. Access the possibly updated validator set state, and update the phase vote collectors if needed
            // (if new PC collected).
            let validator_set_state = block_tree.validator_set_state()?;

            let _ = self
                .phase_vote_collectors
                .update_validator_sets(&validator_set_state);
        }

        Ok(())
    }
}

/// Configuration parameters for the [`HotStuff`] struct.
#[derive(Clone)]
pub(crate) struct HotStuffConfiguration {
    /// The Chain ID of the blockchain that the current replica is to track.
    pub(crate) chain_id: ChainID,

    /// The keypair with which the HotStuff implementation should sign `PhaseVote`s.
    pub(crate) keypair: Keypair,
}

/// The different ways a call to a method of the `HotStuff` struct can fail.
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
/// losing the information about the highest view the replica has phase-voted in, does lead to safety
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
    ///
    /// Note: this can evaluate to true only during the validator set update period.
    fn have_all_leaders_proposed(&self) -> bool {
        matches!(self, ProposalStatus::AllLeadersProposed)
    }
}
