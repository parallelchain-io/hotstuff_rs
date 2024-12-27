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

/// A single participant in the Pacemaker subprotocol.
///
/// # Usage
///
/// After creating an instance of the `Pacemaker` struct using [`new`](Self::new), the caller interacts
/// with it by calling three methods:
///
/// After creating an instance of `Pacemaker` using [`new`](Self::new), the caller should interact with
/// it by calling three methods:
/// 1. [`on_receive_msg`](Self::on_receive_msg): this method should be called whenever a `PacemakerMessage`
///    is received satisfying the method's [preconditions](Self::on_receive_msg#preconditions).
/// 2. [`tick`](Self::tick): this method should be called *as often as is practical*.
/// 3. [`query`](Self::query): whenever `on_receive_msg` or `tick` is called, the internal view counter of
///    the `Pacemaker` may be updated. The caller should call `query` whenever it needs to see this
///    counter.
pub(crate) struct Pacemaker<N: Network> {
    config: PacemakerConfiguration,
    state: PacemakerState,
    view_info: ViewInfo,
    sender: SenderHandle<N>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network> Pacemaker<N> {
    /// Create a new `Pacemaker` instance..
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

    /// Query the Pacemaker for its current `ViewInfo`.
    pub(crate) fn query(&self) -> &ViewInfo {
        &self.view_info
    }

    /// Cause the Pacemaker to check the current time ("clock tick"), possibly updating its internal state
    /// or causing it to send messages to other replicas.
    pub(crate) fn tick<K: KVStore>(
        &mut self,
        block_tree: &BlockTreeSingleton<K>,
    ) -> Result<(), PacemakerError> {
        let cur_view = self.view_info.view;
        let validator_set_state = block_tree.validator_set_state()?;

        // 1. Check if the current view has timed out.
        if Instant::now() > self.view_info.deadline {
            Event::ViewTimeout(ViewTimeoutEvent {
                timestamp: SystemTime::now(),
                view: cur_view,
            })
            .publish(&self.event_publisher);

            // 1.1. If the current view is an Epoch-Change view, broadcast a `TimeoutVote`, then extend the view.
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

                // We extend the view timeout so that we will broadcast a `TimeoutVote` when this view times out again.
                self.extend_view()?

            // 1.2. Else, if the current view is not a normal view, simply update the Pacemaker instance's local
            // view the next view.
            } else {
                self.update_view(cur_view + 1, &validator_set_state)?;
            }

            return Ok(());
        }

        // 2. Update the Pacemaker's timeout vote Collector Pair if the current active validator sets have
        // changed.
        let _ = self
            .state
            .timeout_vote_collectors
            .update_validator_sets(&validator_set_state);

        // 3. If the current Highest PC in the block tree is for the current view or a higher view, and we have
        //    not sent an AdvanceView message in the current view, broadcast a new one with the Highest PC.
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

            // Record in the internal state that we have broadcasted an `AdvanceView` message in this view.
            self.state.last_advance_view = Some(self.view_info.view);
        }

        Ok(())
    }

    /// Execute the required steps in the Pacemaker subprotocol upon receiving a `PacemakerMessage` from the
    /// replica identified by `origin`.
    ///
    /// # Precondition
    ///
    /// [`msg.view()`](PacemakerMessage::view) must be greater than or equal to the current view.
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

    /// Execute the required steps in the Pacemaker subprotocol upon receiving a `TimeoutVote` message from
    /// the replica identified by `origin`.
    ///
    /// # Precondition
    ///
    /// `timeout_vote.view >= self.query().view`
    fn on_receive_timeout_vote<K: KVStore>(
        &mut self,
        timeout_vote: TimeoutVote,
        origin: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
    ) -> Result<(), PacemakerError> {
        Event::ReceiveTimeoutVote(ReceiveTimeoutVoteEvent {
            timestamp: SystemTime::now(),
            origin: origin.clone(),
            timeout_vote: timeout_vote.clone(),
        })
        .publish(&self.event_publisher);

        // 1. If the sending replica is not a validator, ignore its TimeoutVote.
        //
        // TODO: even though as a non-validator we do not need to collect `TimeoutVote`s into
        //       `TimeoutCertificate`s, it is still a good idea to at least inspect `timeout_vote.highest_tc`.
        let validator_set_state = block_tree.validator_set_state()?;
        if !is_validator(origin, &validator_set_state) {
            return Ok(());
        };

        // 2. Check whether the `TimeoutVote` is cryptographically correct and the current view is an Epoch-Change
        //    View.
        if timeout_vote.is_correct(origin)
            && is_epoch_change_view(&timeout_vote.view, self.config.epoch_length)
        {
            let fallback_tc = match &timeout_vote.highest_tc {
                Some(tc) if tc.is_correct(block_tree)? => Some(tc.clone()),
                _ => None,
            };

            // 3. Try to collect the TimeoutVote into a new `TimeoutCertificate`.
            if let Some(new_tc) = self
                .state
                .timeout_vote_collectors
                .collect(origin, timeout_vote)
            {
                Event::CollectTC(CollectTCEvent {
                    timestamp: SystemTime::now(),
                    timeout_certificate: new_tc.clone(),
                })
                .publish(&self.event_publisher);

                // 3.1. If a newly collected Timeout Certificate has a higher view than `highest_tc`, update `highest_tc`.
                //
                // Note: we do not call `update_view` in this conditional block. We will call it when we receive an
                //       AdvanceView message (e.g., the AdvanceView message we may send out in this conditional block).
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

                    // 3.2. If we are a validator and we haven't broadcasted an AdvanceView message in the current view,
                    //      broadcast an AdvanceView message containing the newly collected TimeoutCertificate.
                    if is_validator(&self.config.keypair.public(), &validator_set_state)
                        && self.state.last_advance_view.is_none()
                        || self
                            .state
                            .last_advance_view
                            .is_some_and(|v| v < self.view_info.view)
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

            // 4. Else, if we fail to collect a new TimeoutCertificate, process the `fallback_tc` in the vote.
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

    /// Execute the required steps in the Pacemaker protocol upon receiving an `AdvanceView` message.
    ///
    /// ## Preconditions
    ///
    /// `advance_view.progress_certificate.view() >= self.query().view`
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

        // 1. If the `origin` is not a validator, ignore the `AdvanceView` message.
        let validator_set_state = block_tree.validator_set_state()?;
        if !is_validator(origin, &validator_set_state) {
            return Ok(());
        }

        // 2. Check whether the progress certificate contained in the Advance View message is "valid".
        let progress_certificate = advance_view.progress_certificate.clone();
        let is_valid = match &progress_certificate {
            ProgressCertificate::PhaseCertificate(pc) => pc.is_correct(block_tree)?,
            ProgressCertificate::TimeoutCertificate(tc) => {
                tc.is_correct(&block_tree)?
                    && is_epoch_change_view(&tc.view, self.config.epoch_length)
            }
        };

        if is_valid {
            // 3. If the received certificate is a TimeoutCertificate and has a higher view number than `highest_tc`,
            //    update the `highest_tc`.
            //
            // Note: we do not update `highest_pc` here, since checking the safety of PCs and updating `highest_pc`
            //       is a responsibility of the HotStuff sub-protocol.
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

            // 4. If we are a validator and we haven't broadcasted an AdvanceView message in the current view,
            //    re-broadcast the received AdvanceView message.
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

            // 5. Check if about to enter a new epoch, and if so then set the timeouts for the new epoch.
            let next_view = progress_certificate.view() + 1;
            self.update_view(next_view, &validator_set_state)?
        }

        Ok(())
    }

    /// Update the Pacemaker's state in order to enter a specified `next_view`.
    ///
    /// # Preconditions
    ///
    /// This function should only be called if `next_view` is greater than the current view. Otherwise, an
    /// [`UpdateViewError`] will be returned.
    fn update_view(
        &mut self,
        next_view: ViewNumber,
        validator_set_state: &ValidatorSetState,
    ) -> Result<(), PacemakerError> {
        let cur_view = self.view_info.view;

        // 1. Return an error if the precondition that `next_view` must be greater than the current view is
        //    violated.
        if next_view <= cur_view {
            return Err(UpdateViewError::NonIncreasingViewError {
                cur_view,
                next_view,
            }
            .into());
        }

        // 2. If about to enter a new epoch, set timeouts for the new epoch.
        if epoch(cur_view, self.config.epoch_length) != epoch(next_view, self.config.epoch_length) {
            self.state.update_timeouts(next_view, &self.config);
        }

        // 3. Update the Pacemaker's `view_info` state.
        self.view_info = ViewInfo::new(
            next_view,
            *self
                .state
                .timeouts
                .get(&next_view)
                .ok_or(UpdateViewError::GetViewTimeoutError { view: next_view })?,
        );

        // 4. Replace our current `timeout_vote_collectors` with new ones for the view we just entered.
        self.state.timeout_vote_collectors = <ActiveCollectorPair<TimeoutVoteCollector>>::new(
            self.config.chain_id,
            next_view,
            validator_set_state,
        );

        Ok(())
    }

    /// Extend the timeout of the current view, which must be an Epoch-Change View.
    ///
    /// # Errors
    ///
    /// This function should only be called if the current view is an Epoch-Change View. Otherwise, an
    /// [`ExtendViewError`] will be returned.
    fn extend_view(&mut self) -> Result<(), ExtendViewError> {
        // 1. Confirm that the current view is an Epoch-Change View.
        let cur_view = self.view_info.view;
        if !is_epoch_change_view(&cur_view, self.config.epoch_length) {
            return Err(ExtendViewError::TriedToExtendNonEpochView {
                view: cur_view.clone(),
            });
        };

        // 2. Increase the timeout of the current view inside `PacemakerState`.
        self.state
            .extend_epoch_change_view_timeout(self.view_info.view, &self.config);

        // 3. Increase the timeout of the current view inside `ViewInfo`.
        let new_timeout = self
            .state
            .timeouts
            .get(&cur_view)
            .ok_or(ExtendViewError::GetViewTimeoutError { view: cur_view })?;
        self.view_info = self.view_info.with_new_timeout(*new_timeout);

        Ok(())
    }
}

/// Configuration variables for the [`Pacemaker`] struct.
#[derive(Clone)]
pub(crate) struct PacemakerConfiguration {
    /// The Chain ID of the blockchain that the current replica is to track.
    pub(crate) chain_id: ChainID,

    /// The keypair with which the Pacemaker implementation should sign `TimeoutVote`s.
    pub(crate) keypair: Keypair,

    /// How many views are in an epoch.
    pub(crate) epoch_length: EpochLength,

    /// How much time can elapse in a view before it times out.
    pub(crate) max_view_time: Duration,
}

/// In-memory state of a [`Pacemaker`].
struct PacemakerState {
    /// Mapping between current and future view numbers and the timeout assigned to each.
    timeouts: BTreeMap<ViewNumber, Instant>,

    /// `TimeoutVoteCollector`s for the at-most two validator sets that are active in the current view.
    timeout_vote_collectors: ActiveCollectorPair<TimeoutVoteCollector>,

    /// The view in which this replica last broadcasted an [`AdvanceView`] message.
    last_advance_view: Option<ViewNumber>,
}

impl PacemakerState {
    /// Initializes a `PacemakerState` upon starting the Pacemaker subprotocol.
    fn initialize(
        config: &PacemakerConfiguration,
        init_view: ViewNumber,
        validator_set_state: &ValidatorSetState,
    ) -> Self {
        /// Return initial timeouts on starting the protocol from a given `start_view`.
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
                let time_to_view_deadline = Duration::from_secs(
                    config.max_view_time.as_secs() * (view - start_view.int() + 1),
                );
                timeouts.insert(ViewNumber::new(view), start_time + time_to_view_deadline);
            }

            timeouts
        }

        Self {
            timeouts: initial_timeouts(init_view, config),
            timeout_vote_collectors: <ActiveCollectorPair<TimeoutVoteCollector>>::new(
                config.chain_id,
                init_view,
                validator_set_state,
            ),
            last_advance_view: None,
        }
    }

    /// Update the `PacemakerState`'s timeouts upon entering the epoch with the given `epoch_start_view`.
    fn update_timeouts(&mut self, epoch_start_view: ViewNumber, config: &PacemakerConfiguration) {
        // Remove timeouts for expired views.
        self.timeouts = self.timeouts.split_off(&epoch_start_view);

        // Compute the `ViewNumber` of the view that ends the epoch that `epoch_start_view` starts.
        let epoch_change_view = {
            let epoch_num = epoch(epoch_start_view, config.epoch_length);
            epoch_num * config.epoch_length.int() as u64
        };

        // Set the current time as the epoch's start time.
        let epoch_start_time = Instant::now();

        // Populate `self.timeouts` with the timeouts of the views in the newly-entered epoch.
        for view in epoch_start_view.int()..=epoch_change_view {
            let time_to_view_deadline = Duration::from_secs(
                config.max_view_time.as_secs() * (view - epoch_start_view.int() + 1),
            );
            self.timeouts.insert(
                ViewNumber::new(view),
                epoch_start_time + time_to_view_deadline,
            );
        }
    }

    /// Extend the timeout of the `epoch_change_view` by another `config.max_view_time`.
    ///
    /// # Preconditions
    ///
    /// The caller must ensure that `epoch_change_view` is actually an epoch-change view.
    fn extend_epoch_change_view_timeout(
        &mut self,
        epoch_change_view: ViewNumber,
        config: &PacemakerConfiguration,
    ) {
        self.timeouts
            .insert(epoch_change_view, Instant::now() + config.max_view_time);
    }
}

/// Enumerates the different ways a call to any of [`Pacemaker`]'s methods can fail.
#[derive(Debug)]
pub enum PacemakerError {
    /// See: [`UpdateViewError`].
    UpdateViewError(UpdateViewError),

    /// See: [`ExtendViewError`].
    ExtendViewError(ExtendViewError),

    /// See: [`BlockTreeError`]
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

/// Enumerates the different ways a [`Pacemaker::update_view`] call can fail.
#[derive(Debug)]
pub enum UpdateViewError {
    /// An attempt was made to update the current view to a lower view. This violates the invariant that views
    /// must be monotonically increasing.
    NonIncreasingViewError {
        /// The current view.
        cur_view: ViewNumber,

        /// The lower view that the caller tried to change the current view to.
        next_view: ViewNumber,
    },

    /// The timeout for a requested view cannot be found in the [`PacemakerState`]. This violates the invariant
    /// that the `Pacemaker` should be able to provide the timeout of any view it returns from
    /// [`view_info`](Pacemaker::view_info).
    GetViewTimeoutError { view: ViewNumber },
}

/// Enumerates the different ways a [`Pacemaker::extend_view`] call can fail.
#[derive(Debug)]
pub enum ExtendViewError {
    /// An attempt was made to extend a view that is not an Epoch-Change view.
    TriedToExtendNonEpochView { view: ViewNumber },

    /// Same as [`UpdateViewError::GetViewTimeoutError`].
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
    /// Create a new `ViewInfo` instance containing the provided parameters.
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

/// Deterministically select a replica in `validator_set` to become the leader of `view` using the
/// [Interleaved WRR](https://en.wikipedia.org/wiki/Weighted_round_robin#Interleaved_WRR) algorithm.
///
/// [Read more](super#leader-selection).
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
