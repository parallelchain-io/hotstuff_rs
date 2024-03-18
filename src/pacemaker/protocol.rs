/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of the Pacemaker protocol, based on the Lewis-Pye pacemaker.
//! TODO: describe how it works.

use std::time::Duration;
use std::{collections::BTreeMap, sync::mpsc::Sender, time::Instant};

use ed25519_dalek::VerifyingKey;

use crate::events::Event;
use crate::networking::{Network, SenderHandle};
use crate::state::{BlockTree, KVStore};
use crate::types::basic::EpochLength;
use crate::types::validators::ValidatorSet;
use crate::types::{
    basic::{ChainID, ViewNumber}, 
    keypair::Keypair
};
use crate::pacemaker::messages::{AdvanceView, PacemakerMessage, TimeoutVote};
use crate::pacemaker::types::TimeoutVoteCollector;

/// A pluggable Pacemaker protocol for Byzantine View Synchronization inspired by 
/// the Lewis-Pye View Synchronization protocol (https://arxiv.org/pdf/2201.01107.pdf).
/// Its [PacemakerState] is an authoritative source of information regarding the current view
/// and its leader, and [Algorithm][crate::algorithm::Algorithm] should regularly query the 
/// [Pacemaker] for this information, and propagate the information to 
/// [HotStuff](crate::hotstuff::protocol::HotStuff).
///
/// The [Pacemaker] exposes the following API for use in the [Algorithm]:
/// 1. [Pacemaker::new]: creates a fresh instance of the [Pacemaker],
/// 2. [Pacemaker::updates]: queries the [Pacemaker] to determine whether the view should be updated,
/// 3. [Pacemaker::tick]: measures the time and updates the internal state of the [Pacemaker] if needed,
/// 4. [Pacemaker::on_receive_msg]: updates the [PacemakerState] and possibly the [BlockTree] in response
///    to a received [PacemakerMessage].
pub(crate) struct Pacemaker<N: Network> {
    config: PacemakerConfiguration,
    state: PacemakerState,
    sender: SenderHandle<N>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network> Pacemaker<N> {

    pub(crate) fn new(
        config: PacemakerConfiguration,
        sender: SenderHandle<N>,
        init_view: ViewNumber,
        init_validator_set: ValidatorSet,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        let init_leader = select_leader(init_view, &init_validator_set);
        let state = PacemakerState::initialize(config.clone(), init_view, init_leader, init_validator_set);
        Self {
            config,
            state,
            sender,
            event_publisher,
        }
    }

    /// Query the pacemaker for updates to the current view.
    /// This method reads the internal state of the pacemaker ([PacemakerState]),
    /// and returns updates relative to the given view.
    pub(crate) fn updates(&self, view: ViewNumber) -> PacemakerUpdates {
        todo!()
    }

    /// Check the current time ('clock tick'), and
    /// possibly update the [internal state of the pacemaker][PacemakerState] 
    /// in response to the 'clock tick'.
    pub(crate) fn tick(&mut self) {
        todo!()
    }

    /// Returns the deadline associated with the view stored in [PacemakerState::timeouts], if any.
    pub(crate) fn view_deadline(&self, view: ViewNumber) -> Option<Instant> {
        todo!()
    }

    // Returns the leader of a given view and given validator set.
    pub(crate) fn view_leader(&self, view: ViewNumber, validator_set: &ValidatorSet) -> VerifyingKey {
        todo!()
    }

    /// Update the internal state of the pacemaker and possibly the block tree 
    /// in response to receiving a [PacemakerMessage].
    pub(crate) fn on_receive_msg<K: KVStore>(
        &mut self, 
        msg: PacemakerMessage,
        origin: VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) {
        todo!()
    }

    /// Update the [internal state of the pacemaker][PacemakerState]
    /// in response to receiving a [TimeoutVote].
    fn on_receive_timeout_vote<K: KVStore>(
        &mut self, 
        msg: TimeoutVote,
        origin: VerifyingKey,
        block_tree: &BlockTree<K>,
    ) {
        todo!()
    }

    /// Update the [internal state of the pacemaker][PacemakerState]
    /// and possibly the block tree in response to receiving an [AdvanceView].
    fn on_receive_advance_view<K: KVStore>(
        &mut self, 
        msg: AdvanceView,
        origin: VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) {
        todo!()
    }

}

/// Immutable parameters that determine the behaviour of the [Pacemaker] and should never change.
#[derive(Clone)]
pub(crate) struct PacemakerConfiguration {
    pub(crate) chain_id: ChainID,
    pub(crate) keypair: Keypair,
    pub(crate) epoch_length: EpochLength,
    pub(crate) max_view_time: Duration,
}

/// Internal state of the [Pacemaker]. Keeps track of the current
/// view and leader, the timeouts allocated to current and future views (if any),
/// and the [timeout votes][TimeoutVote] collected for the current view.
struct PacemakerState {
    cur_view: ViewNumber,
    cur_leader: VerifyingKey,
    timeouts: BTreeMap<ViewNumber, Instant>,
    timeout_vote_collector: TimeoutVoteCollector,
}

impl PacemakerState {

    /// Initializes the [PacemakerState] on starting the protocol.
    fn initialize(
        config: PacemakerConfiguration,
        init_view: ViewNumber,
        init_leader: VerifyingKey,
        validator_set: ValidatorSet,
    ) -> Self {
        // TODO: set timeout for init_view/init-epoch.
        Self { 
            cur_view: init_view, 
            cur_leader: init_leader,
            timeouts: BTreeMap::new(),
            timeout_vote_collector: TimeoutVoteCollector::new(config.chain_id, init_view, validator_set),
        }
    }

    /// Set the timeout for each view in the epoch starting from a given view.
    fn set_timeouts(&mut self, start_view: ViewNumber, config: PacemakerConfiguration) {
        todo!()
    }

    /// Unique API for changing [PacemakerState]'s [cur_view](PacemakerState::cur_view) 
    /// and [cur_leader][PacemakerState::cur_leader] fields.
    /// Guarantess that over the course of the protocol execution the value of this
    /// field is monotonically increasing. If the view and leader were updated,
    /// it returns the old view and the new view respectively, else returns None.
    fn update_view<K: KVStore>(
        &mut self, 
        block_tree: &BlockTree<K>, 
        config: PacemakerConfiguration) 
        -> Option<(ViewNumber, ViewNumber)> {

        // Get the max of self.cur_view, highest_qc().view, highested_entered_view, highest_tc().view,
        // and add 1
        todo!()
    }

}

pub(crate) enum PacemakerUpdates {
    RemainInView,
    EnterView{view: ViewNumber, leader: VerifyingKey, next_leader: VerifyingKey},
}

/// Implements the Interleaved Weighted Round Robin algorithm for selecting a view leader.
/// For internal use by the [Pacemaker] and [PacemakerState] methods.
fn select_leader(
    view: ViewNumber,
    validator_set: &ValidatorSet,
) -> VerifyingKey {
    todo!()
}
