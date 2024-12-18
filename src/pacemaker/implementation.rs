/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Event-driven implementation of the Pacemaker subprotocol.
//!
//! Main type: [`Pacemaker`].

use std::{
    collections::BTreeMap,
    sync::mpsc::Sender,
    time::{Duration, Instant, SystemTime},
};

use ed25519_dalek::VerifyingKey;

use crate::{
    block_tree::{
        accessors::internal::{BlockTreeError, BlockTreeSingleton, BlockTreeWriteBatch},
        pluggables::KVStore,
    },
    events::{
        AdvanceViewEvent, CollectTCEvent, Event, ReceiveAdvanceViewEvent, ReceiveTimeoutVoteEvent,
        TimeoutVoteEvent, UpdateHighestTCEvent, ViewTimeoutEvent,
    },
    hotstuff::roles::is_validator,
    networking::{messages::Message, network::Network, sending::SenderHandle},
    pacemaker::{
        messages::{AdvanceView, PacemakerMessage, ProgressCertificate, TimeoutVote},
        types::TimeoutVoteCollector,
    },
    types::{
        crypto_primitives::Keypair,
        data_types::{ChainID, EpochLength, ViewNumber},
        signed_messages::{ActiveCollectorPair, Certificate, SignedMessage},
        validator_set::{ValidatorSet, ValidatorSetState},
    },
};

/// A Pacemaker protocol for Byzantine View Synchronization inspired by the
/// [Lewis-Pye View Synchronization protocol](https://arxiv.org/pdf/2201.01107.pdf). Its
/// [`PacemakerState`] is the authoritative source of information regarding the current view and its
/// leader, and [`Algorithm`][crate::algorithm::Algorithm] should regularly query the [`Pacemaker`] for
/// this information ([`ViewInfo`]), and propagate the information to [`hotstuff`](crate::hotstuff).
///
/// The Pacemaker exposes the following API for use in the Algorithm:
/// 1. [Pacemaker::new]: creates a fresh instance of the [`Pacemaker`],
/// 2. [Pacemaker::view_info]: queries the Pacemaker for [`ViewInfo`], which can be used to determine
///    whether the view should be updated,
/// 3. [Pacemaker::tick]: updates the internal state of the Pacemaker and broadcasts a message if needed
///    in response to a time measurement,
/// 4. [Pacemaker::on_receive_msg]: updates the [`PacemakerState`] and possibly the block tree, as well
///    as broadcasts messages, in response to a received [`PacemakerMessage`].
///
/// If any of these actions fail, a [`PacemakerError`] is returned.
pub(crate) struct Pacemaker<N: Network> {
    config: PacemakerConfiguration,
    state: PacemakerState,
    view_info: ViewInfo,
    sender: SenderHandle<N>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network> Pacemaker<N> {
    /// Create a new [`Pacemaker`] instance.
    pub(crate) fn new(
        config: PacemakerConfiguration,
        sender: SenderHandle<N>,
        init_view: ViewNumber,
        init_validator_set_state: &ValidatorSetState,
        event_publisher: Option<Sender<Event>>,
    ) -> Result<Self, PacemakerError> {
        let state = PacemakerState::initialize(&config, init_view, init_validator_set_state);
        let timeout = state
            .timeouts
            .get(&init_view)
            .clone()
            .ok_or(UpdateViewError::GetViewTimeoutError { view: init_view })?;
        let view_info = ViewInfo::new(init_view, *timeout);
        Ok(Self {
            config,
            state,
            view_info,
            sender,
            event_publisher,
        })
    }

    /// Query the pacemaker for [`ViewInfo`].
    pub(crate) fn view_info(&self) -> &ViewInfo {
        &self.view_info
    }

    /// Check the current time ('clock tick'), and possibly send messages and update the
    /// [internal state of the pacemaker][PacemakerState] in response to the 'clock tick'.
    ///
    /// First, in response to a clock tick indicating a view timeout the state can be updated in two ways:
    /// 1. If it is an epoch-change view, then its deadline should be extended, and a timeout vote should
    ///    be broadcasted.
    /// 2. If it is a not an epoch-change view, then the view should be updated to the subsequent view.
    ///
    /// Additionally, tick should check if there is a PC for the current view (whether an epoch-change
    /// view or not) available in the block tree, and if so it should broadcast the PC in an advance
    /// view message.
    ///
    /// It should also check of the validator set state has been updated, and if so it should update the
    /// timeout vote collectors accordingly.
    pub(crate) fn tick<K: KVStore>(
        &mut self,
        block_tree: &BlockTreeSingleton<K>,
    ) -> Result<(), PacemakerError> {
        let cur_view = self.view_info.view;
        let validator_set_state = block_tree.validator_set_state()?;

        // 1. Check if the current view has timed out, and proceed accordingly.
        if Instant::now() > self.view_info.deadline {
            Event::ViewTimeout(ViewTimeoutEvent {
                timestamp: SystemTime::now(),
                view: cur_view,
            })
            .publish(&self.event_publisher);

            if is_epoch_change_view(&cur_view, self.config.epoch_length) {
                if is_validator(&self.config.keypair.public(), &validator_set_state) {
                    let pacemaker_message = PacemakerMessage::timeout_vote(
                        &self.config.keypair,
                        self.config.chain_id,
                        cur_view,
                        block_tree.highest_tc()?,
                    );
                    self.sender
                        .broadcast(Message::from(pacemaker_message.clone()));
                    if let PacemakerMessage::TimeoutVote(timeout_vote) = pacemaker_message {
                        Event::TimeoutVote(TimeoutVoteEvent {
                            timestamp: SystemTime::now(),
                            timeout_vote,
                        })
                        .publish(&self.event_publisher)
                    }
                }
                self.extend_view()?
            } else {
                self.update_view(cur_view + 1, &validator_set_state)?;
            }

            return Ok(());
        }

        // 2. Check if the timeout vote collectors need to be updated in response to a validator set
        //    state update.
        let _ = self
            .state
            .timeout_vote_collectors
            .update_validator_sets(&validator_set_state);

        // 3. Check if a PC for the current view is available and if I am a validator, and if so
        //    broadcast an advance view message.
        if block_tree.highest_pc()?.view >= cur_view
            && !block_tree.highest_pc()?.is_genesis_pc()
            && is_validator(&self.config.keypair.public(), &validator_set_state)
            && (self.state.last_advance_view.is_none()
                || self.state.last_advance_view.is_some_and(|v| v < cur_view))
        {
            let pacemaker_message = PacemakerMessage::advance_view(
                ProgressCertificate::PhaseCertificate(block_tree.highest_pc()?),
            );
            self.sender
                .broadcast(Message::from(pacemaker_message.clone()));
            if let PacemakerMessage::AdvanceView(advance_view) = pacemaker_message {
                Event::AdvanceView(AdvanceViewEvent {
                    timestamp: SystemTime::now(),
                    advance_view,
                })
                .publish(&self.event_publisher)
            }
            self.state.last_advance_view = Some(self.view_info.view);
        }

        Ok(())
    }

    /// Update the internal state of the pacemaker and possibly the block tree, in response to receiving a
    /// [`PacemakerMessage`]. Broadcast an [`AdvanceView`] message in case a
    /// [`TimeoutCertificate`][crate::pacemaker::types::TimeoutCertificate] was collected, or in case a valid
    /// AdvanceView message was received from a peer.
    pub(crate) fn on_receive_msg<K: KVStore>(
        &mut self,
        msg: PacemakerMessage,
        origin: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
    ) -> Result<(), PacemakerError> {
        match msg {
            PacemakerMessage::TimeoutVote(timeout_vote) => {
                self.on_receive_timeout_vote(timeout_vote, origin, block_tree)?
            }
            PacemakerMessage::AdvanceView(advance_view) => {
                self.on_receive_advance_view(advance_view, origin, block_tree)?
            }
        }
        Ok(())
    }

    /// Update the [internal state of the pacemaker][PacemakerState] in response to receiving a
    /// [`TimeoutVote`].
    ///
    /// If a [`TimeoutCertificate`][crate::pacemaker::types::TimeoutCertificate] is collected, the replica
    /// should try to update its `highest_tc` and broadcast the collected `TimeoutCertificate`. The vote may
    /// be rejected if the receiver replica is lagging behind the quorum from which the vote is sent. In
    /// such case the replica can use the sender's `highest_tc` attached to the vote to move ahead.
    ///
    /// ## Preconditions
    ///
    /// The Timeout Vote may be for any view greater or equal to the current view, but only timeout votes
    /// for the current view will be collected.
    fn on_receive_timeout_vote<K: KVStore>(
        &mut self,
        timeout_vote: TimeoutVote,
        signer: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
    ) -> Result<(), PacemakerError> {
        Event::ReceiveTimeoutVote(ReceiveTimeoutVoteEvent {
            timestamp: SystemTime::now(),
            origin: signer.clone(),
            timeout_vote: timeout_vote.clone(),
        })
        .publish(&self.event_publisher);

        // If we are not a validator, ignore the Timeout Vote.
        let validator_set_state = block_tree.validator_set_state()?;
        if !is_validator(signer, &validator_set_state) {
            return Ok(());
        };

        // Check whether the Timeout Vote is cryptographically correct and that the current view is an epoch-change view.
        if timeout_vote.is_correct(signer)
            && is_epoch_change_view(&timeout_vote.view, self.config.epoch_length)
        {
            let fallback_tc = match &timeout_vote.highest_tc {
                Some(tc) if tc.is_correct(block_tree)? => Some(tc.clone()),
                _ => None,
            };

            // Try to collect the Timeout Vote to create a new Timeout Certificate.
            if let Some(new_tc) = self
                .state
                .timeout_vote_collectors
                .collect(signer, timeout_vote)
            {
                Event::CollectTC(CollectTCEvent {
                    timestamp: SystemTime::now(),
                    timeout_certificate: new_tc.clone(),
                })
                .publish(&self.event_publisher);

                // If a newly collected Timeout Certificate has a higher view than `highest_tc`, update `highest_tc`.
                //
                // Note: we do not call `update_view` in this conditional block. We will call it when we receive an
                // Advance View message (e.g., the Advance View message we may send out in this conditional block).
                if block_tree.highest_tc()?.is_none()
                    || new_tc.view > block_tree.highest_tc()?.unwrap().view
                {
                    let mut wb = BlockTreeWriteBatch::new();
                    wb.set_highest_tc(&new_tc)?;
                    block_tree.write(wb);
                    Event::UpdateHighestTC(UpdateHighestTCEvent {
                        timestamp: SystemTime::now(),
                        highest_tc: new_tc.clone(),
                    })
                    .publish(&self.event_publisher);

                    // If I am a validator and I haven't broadcasted an Advance View message in the current view, broadcast
                    // an Advance View message containing the newly collected Timeout Certificate.
                    if is_validator(&self.config.keypair.public(), &validator_set_state)
                        && (self.state.last_advance_view.is_none()
                            || self
                                .state
                                .last_advance_view
                                .is_some_and(|v| v < self.view_info.view))
                    {
                        let pacemaker_message = PacemakerMessage::advance_view(
                            ProgressCertificate::TimeoutCertificate(new_tc),
                        );
                        self.sender
                            .broadcast(Message::from(pacemaker_message.clone()));
                        if let PacemakerMessage::AdvanceView(advance_view) = pacemaker_message {
                            Event::AdvanceView(AdvanceViewEvent {
                                timestamp: SystemTime::now(),
                                advance_view,
                            })
                            .publish(&self.event_publisher)
                        }
                        self.state.last_advance_view = Some(self.view_info.view);
                    }
                }

            // If we fail to collect a new Timeout Certificate, process the `fallback_tc` in the vote.
            } else if let Some(tc) = fallback_tc {
                // In case the replica is behind, the "fallback tc" contained in the timeout vote message
                // serves to prove to it that a quorum is ahead and lets the replica catch up.
                if block_tree.highest_tc()?.is_none()
                    || tc.view > block_tree.highest_tc()?.unwrap().view
                {
                    block_tree.set_highest_tc(&tc)?;
                    Event::UpdateHighestTC(UpdateHighestTCEvent {
                        timestamp: SystemTime::now(),
                        highest_tc: tc.clone(),
                    })
                    .publish(&self.event_publisher);

                    // Check if about to enter a new epoch, and if so then set the timeouts for the new epoch.
                    let next_view = tc.view + 1;
                    self.update_view(next_view, &validator_set_state)?
                }
            }
        }
        Ok(())
    }

    /// Update the [internal state of the pacemaker][PacemakerState] and possibly the block tree in response
    /// to receiving an [`AdvanceView`] message.
    ///
    /// ## Preconditions
    ///
    /// The Advance View message must be for the current or higher view.
    fn on_receive_advance_view<K: KVStore>(
        &mut self,
        advance_view: AdvanceView,
        origin: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
    ) -> Result<(), PacemakerError> {
        Event::ReceiveAdvanceView(ReceiveAdvanceViewEvent {
            timestamp: SystemTime::now(),
            origin: origin.clone(),
            advance_view: advance_view.clone(),
        })
        .publish(&self.event_publisher);

        // If the sender is not a validator, ignore the Advance View.
        let validator_set_state = block_tree.validator_set_state()?;
        if !is_validator(origin, &validator_set_state) {
            return Ok(());
        }

        // Check whether the progress certificate contained in the Advance View message is "valid". What this
        // entails differs depending on whether the certificate is a Phase Certificate or a Timeout
        // Certificate.
        let progress_certificate = advance_view.progress_certificate.clone();
        let is_valid = match &progress_certificate {
            ProgressCertificate::PhaseCertificate(pc) => pc.is_correct(block_tree)?,
            ProgressCertificate::TimeoutCertificate(tc) => {
                tc.is_correct(&block_tree)?
                    && is_epoch_change_view(&tc.view, self.config.epoch_length)
            }
        };

        if is_valid {
            // If the received certificate is a Timeout Certificate and has a higher view number than `highest_tc`,
            // update the `highest_tc`.
            //
            // Note: we do not update `highest_pc` here, since checking the safety of PCs and updating `highest_pc`
            // is a responsibility of the HotStuff sub-protocol.
            if let ProgressCertificate::TimeoutCertificate(tc) = &progress_certificate {
                if block_tree.highest_tc()?.is_none()
                    || tc.view > block_tree.highest_tc()?.unwrap().view
                {
                    block_tree.set_highest_tc(&tc)?;
                    Event::UpdateHighestTC(UpdateHighestTCEvent {
                        timestamp: SystemTime::now(),
                        highest_tc: tc.clone(),
                    })
                    .publish(&self.event_publisher);
                }
            };

            // If I am a validator and I haven't broadcasted an Advance View message in the current view,
            // re-broadcast the received Advance View message.
            if is_validator(
                &self.config.keypair.public(),
                &block_tree.validator_set_state()?,
            ) && (self.state.last_advance_view.is_none()
                || self
                    .state
                    .last_advance_view
                    .is_some_and(|v| v < self.view_info.view))
            {
                self.sender
                    .broadcast(Message::from(PacemakerMessage::AdvanceView(
                        advance_view.clone(),
                    )));
                Event::AdvanceView(AdvanceViewEvent {
                    timestamp: SystemTime::now(),
                    advance_view,
                })
                .publish(&self.event_publisher);
                self.state.last_advance_view = Some(self.view_info.view);
            }

            // Check if about to enter a new epoch, and if so then set the timeouts for the new epoch.
            let next_view = progress_certificate.view() + 1;
            self.update_view(next_view, &validator_set_state)?
        }

        Ok(())
    }

    /// Update the current view to the specified `next_view`. This may involve setting timeouts for the
    /// views of a new epoch, in case the next view is in a future epoch.
    ///
    /// This method, by being the unique method used to update the pacemaker [`ViewInfo`], and checking if the
    /// next view is greater than the current view, guarantees that views are monotonically increasing.
    ///
    /// ## Errors
    ///
    /// This function should only be called if `next_view` is greater than the current view. Otherwise, an
    /// [`UpdateViewError`] will be returned.
    fn update_view(
        &mut self,
        next_view: ViewNumber,
        validator_set_state: &ValidatorSetState,
    ) -> Result<(), PacemakerError> {
        let cur_view = self.view_info.view;
        if next_view <= cur_view {
            return Err(UpdateViewError::NonIncreasingViewError {
                cur_view,
                next_view,
            }
            .into());
        }

        // If about to enter a new epoch, set timeouts for the new epoch.
        if epoch(cur_view, self.config.epoch_length) != epoch(next_view, self.config.epoch_length) {
            self.state.set_timeouts(next_view, &self.config)
        }

        // Update the view.
        self.view_info = ViewInfo::new(
            next_view,
            *self
                .state
                .timeouts
                .get(&next_view)
                .ok_or(UpdateViewError::GetViewTimeoutError { view: next_view })?,
        );

        // Update the timeout vote collectors.
        self.state.timeout_vote_collectors = <ActiveCollectorPair<TimeoutVoteCollector>>::new(
            self.config.chain_id,
            next_view,
            validator_set_state,
        );

        Ok(())
    }

    /// Extend the timeout of the current view
    ///
    /// # Errors
    ///
    /// This function should only be called if the current view is an epoch-change view. Otherwise, an
    /// [`ExtendViewError`] will be returned.
    fn extend_view(&mut self) -> Result<(), ExtendViewError> {
        // Confirm that the current view is an epoch-change view.
        let cur_view = self.view_info.view;
        if !is_epoch_change_view(&cur_view, self.config.epoch_length) {
            return Err(ExtendViewError::TriedToExtendNonEpochView {
                view: cur_view.clone(),
            });
        };

        self.state
            .extend_epoch_view_timeout(self.view_info.view, &self.config);
        let new_timeout = self
            .state
            .timeouts
            .get(&cur_view)
            .ok_or(ExtendViewError::GetViewTimeoutError { view: cur_view })?;
        self.view_info = self.view_info.with_new_timeout(*new_timeout);
        Ok(())
    }
}

/// Immutable parameters that determine the behaviour of the [`Pacemaker`] and should never change after
/// a replica starts.
#[derive(Clone)]
pub(crate) struct PacemakerConfiguration {
    pub(crate) chain_id: ChainID,
    pub(crate) keypair: Keypair,
    pub(crate) epoch_length: EpochLength,

    // How much time can elapse in a view before timing out.
    pub(crate) max_view_time: Duration,
}

/// Internal state of the [`Pacemaker`]. Keeps track of the timeouts allocated to current and future views
/// (if any), and the [timeout votes][TimeoutVote] collected for the current view.
struct PacemakerState {
    timeouts: BTreeMap<ViewNumber, Instant>,
    timeout_vote_collectors: ActiveCollectorPair<TimeoutVoteCollector>,

    // The view in which this replica last broadcasted an Advance View message.
    last_advance_view: Option<ViewNumber>,
}

impl PacemakerState {
    /// Initializes the [`PacemakerState`] on starting the protocol. Should only be called at the start of
    /// the protocol.
    fn initialize(
        config: &PacemakerConfiguration,
        init_view: ViewNumber,
        validator_set_state: &ValidatorSetState,
    ) -> Self {
        Self {
            timeouts: Self::initial_timeouts(init_view, config),
            timeout_vote_collectors: <ActiveCollectorPair<TimeoutVoteCollector>>::new(
                config.chain_id,
                init_view,
                validator_set_state,
            ),
            last_advance_view: None,
        }
    }

    /// Return initial timeouts on starting the protocol from a given start view.
    fn initial_timeouts(
        start_view: ViewNumber,
        config: &PacemakerConfiguration,
    ) -> BTreeMap<ViewNumber, Instant> {
        let mut timeouts = BTreeMap::new();

        let epoch = epoch(start_view, config.epoch_length);
        let epoch_view = epoch * config.epoch_length.int() as u64;

        let start_time = Instant::now();

        // Add timeouts for all remaining views in the epoch of start_view.
        for view in start_view.int()..=epoch_view {
            let time_to_view_deadline =
                Duration::from_secs(config.max_view_time.as_secs() * (view - start_view.int() + 1));
            timeouts.insert(ViewNumber::new(view), start_time + time_to_view_deadline);
        }

        return timeouts;
    }

    /// Set the timeout for each view in the epoch starting from a given view.
    fn set_timeouts(&mut self, start_view: ViewNumber, config: &PacemakerConfiguration) {
        // Remove the timeouts for expired views.
        self.timeouts = self.timeouts.split_off(&start_view);

        let epoch = epoch(start_view, config.epoch_length);
        let epoch_view = epoch * config.epoch_length.int() as u64;

        let start_time = Instant::now();

        // Add timeouts for all remaining views in the epoch of start_view.
        for view in start_view.int()..=epoch_view {
            let time_to_view_deadline =
                Duration::from_secs(config.max_view_time.as_secs() * (view - start_view.int() + 1));
            self.timeouts
                .insert(ViewNumber::new(view), start_time + time_to_view_deadline);
        }
    }

    /// Extend the timeout of the epoch-change view by another max_view_time.
    ///
    /// Required: The caller must ensure that the view is an epoch-change view.
    fn extend_epoch_view_timeout(
        &mut self,
        epoch_view: ViewNumber,
        config: &PacemakerConfiguration,
    ) {
        self.timeouts
            .insert(epoch_view, Instant::now() + config.max_view_time);
    }
}

/// The pacemaker can fail in two fundamental ways:
/// 1. In updating the view, which involves creating [`ViewInfo`] for the new view.
/// 2. In extending its current view, which involves setting a new deadline in its ViewInfo.
///
/// Both of these failures correspond to violations of key invariants of the protocol.
/// Hence, this error is irrecoverable and on seeing it, the caller should panic.
#[derive(Debug)]
pub enum PacemakerError {
    UpdateViewError(UpdateViewError),
    ExtendViewError(ExtendViewError),
    BlockTreeError(BlockTreeError),
}

impl From<BlockTreeError> for PacemakerError {
    fn from(value: BlockTreeError) -> Self {
        PacemakerError::BlockTreeError(value)
    }
}

impl From<UpdateViewError> for PacemakerError {
    fn from(value: UpdateViewError) -> Self {
        PacemakerError::UpdateViewError(value)
    }
}

impl From<ExtendViewError> for PacemakerError {
    fn from(value: ExtendViewError) -> Self {
        PacemakerError::ExtendViewError(value)
    }
}

/// Updating the view can fail in two ways:
/// 1. If an attempt is made to update the view to a lower view than the current view. If successful,
///    such action would violate the invariant the views obtained through [Pacemaker::view_info] are
///    monotonically increasing.
/// 2. If the timeout for the target view cannot be found in the [`PacemakerState`]. If succesful,
///    such action would violate the invariant that on providing the current view through
///    [`Pacemaker::view_info`] the Pacemaker must also provide its deadline.
#[derive(Debug)]
pub enum UpdateViewError {
    NonIncreasingViewError {
        cur_view: ViewNumber,
        next_view: ViewNumber,
    },
    GetViewTimeoutError {
        view: ViewNumber,
    },
}

/// Extending the view can fail in two ways:
/// 1. If an attempt is made to extend a view that is not an epoch-change view. If succesful, such
///    action would violate the invariant that only epoch-change views can be updated.
/// 2. If the current timeout for the view cannot be obtained from the [`PacemakerState`]. If successful,
///    such action would violate the invariant that a view can only be extended if its timeout is known.
#[derive(Debug)]
pub enum ExtendViewError {
    TriedToExtendNonEpochView { view: ViewNumber },
    GetViewTimeoutError { view: ViewNumber },
}

/// Describes a view (most often the current view), in terms of its view number and its view deadline (the
/// instant in time in which the view should end if no progress was made).
#[derive(PartialEq, Eq, Clone)]
pub(crate) struct ViewInfo {
    pub(crate) view: ViewNumber,
    pub(crate) deadline: Instant,
}

impl ViewInfo {
    pub(crate) fn new(view: ViewNumber, deadline: Instant) -> Self {
        Self { view, deadline }
    }

    /// Return a given [ViewInfo] with updated timeout.
    pub(crate) fn with_new_timeout(&self, new_deadline: Instant) -> Self {
        Self {
            view: self.view,
            deadline: new_deadline,
        }
    }
}

/// Implements the [Interleaved Weighted Round Robin](https://en.wikipedia.org/wiki/Weighted_round_robin#Interleaved_WRR)
/// algorithm for selecting a view leader. For internal use by the [`Pacemaker`] and [`PacemakerState`] methods.
pub fn select_leader(view: ViewNumber, validator_set: &ValidatorSet) -> VerifyingKey {
    // Length of the abstract array.
    let p_total = validator_set.total_power();
    // Total number of validators.
    let n = validator_set.len();
    // Index in the abstract array.
    let index = view.int() % (p_total.int() as u64);
    // Max. power among the validators.
    let p_max = validator_set
        .validators_and_powers()
        .iter()
        .map(|(_, power)| power.int())
        .max()
        .expect("The validator set cannot be empty!")
        .clone();

    let mut counter = 0;

    // Search for a validator at given index in the abstract array of leaders.
    for threshold in 1..=p_max {
        for k in 0..=(n - 1) {
            let validator = validator_set.validators().nth(k).unwrap();
            if validator_set.power(validator).unwrap().int() >= threshold {
                if counter == index {
                    return *validator;
                }
                counter += 1
            }
        }
    }

    // Safety: If index not found, panic. This should never happen.
    unreachable!("Cannot select a leader: index not found!")
}

/// Check whether `view` is an epoch-change view given the configured `epoch_length`.
fn is_epoch_change_view(view: &ViewNumber, epoch_length: EpochLength) -> bool {
    view.int() % (epoch_length.int() as u64) == 0
}

/// Compute the current epoch based on the current `view` and the configured `epoch_length`.
fn epoch(view: ViewNumber, epoch_length: EpochLength) -> u64 {
    view.int().div_ceil(epoch_length.int() as u64)
}

/// Tests if the number of times each validator is selected as a leader is proportional to its power.
#[test]
fn select_leader_fairness_test() {
    use crate::types::{data_types::Power, update_sets::ValidatorSetUpdates};
    use ed25519_dalek::{SigningKey, VerifyingKey};
    use rand_core::OsRng;

    let mut csprg = OsRng {};
    let n = 20;
    let keypairs: Vec<SigningKey> = (0..n).map(|_| SigningKey::generate(&mut csprg)).collect();
    let public_keys: Vec<VerifyingKey> = keypairs
        .iter()
        .map(|keypair| keypair.verifying_key())
        .collect();

    let mut validator_set = ValidatorSet::new();
    let mut validator_set_updates = ValidatorSetUpdates::new();
    public_keys
        .iter()
        .zip(0..n)
        .for_each(|(validator, power)| validator_set_updates.insert(*validator, Power::new(power)));
    validator_set.apply_updates(&validator_set_updates);

    let total_power = validator_set.total_power().int() as u64;
    let leader_sequence: Vec<VerifyingKey> = (0..total_power)
        .into_iter()
        .map(|v| select_leader(ViewNumber::new(v), &validator_set))
        .collect();

    validator_set.validators().for_each(|validator| {
        assert_eq!(
            leader_sequence
                .iter()
                .filter(|leader| leader == &validator)
                .count(),
            validator_set.power(validator).unwrap().int() as usize
        )
    })
}
