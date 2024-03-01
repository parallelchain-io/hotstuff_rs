/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of the Pacemaker protocol, based on the Lewis-Pye pacemaker.

use std::cmp::max;
use std::time::Duration;
use std::{collections::BTreeMap, sync::mpsc::Sender, time::Instant};

use ed25519_dalek::VerifyingKey;

use crate::events::Event;
use crate::networking::{Network, SenderHandle};
use crate::state::{BlockTree, KVStore};
use crate::types::validators::ValidatorSet;
use crate::types::{
    basic::{ChainID, ViewNumber}, 
    keypair::Keypair
};

use super::messages::{AdvanceView, PacemakerMessage, TimeoutVote};

/// A pluggable Pacemaker protocol for Byzantine View Synchronization inspired by 
/// the Lewis-Pye View Synchronization protocol (https://arxiv.org/pdf/2201.01107.pdf).
/// Its [PacemakerState] is an authoritative source of information regarding the current view
/// and its leader, and [Algorithm] should regularly query the [Pacemaker] for this information,
/// and propagate the information to [HotStuff](crate::hotstuff::protocol::HotStuff).
///
/// The [Pacemaker] exposes the following API for use in the [Algorithm]:
/// 1. [Pacemaker::new]: creates a fresh instance of the [Pacemaker],
/// 2. [Pacemaker::get_view_updates]: queries the [Pacemaker] to determine whether the view should be updated,
/// 3. [Pacemaker::tick]: measures the time and updates the internal state of the [Pacemaker] if needed,
/// 4. [Pacemaker::on_receive_msg]: updates the [PacemakerState] and possibly the [BlockTree] in response
///    to a received [PacemakerMessage].
pub(crate) struct Pacemaker<N: Network, K: KVStore> {
    chain_id : ChainID,
    keypair: Keypair,
    state: PacemakerState,
    msg_sender: SenderHandle<N, PacemakerMessage>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network, K: KVStore> Pacemaker<N, K> {

    pub(crate) fn new(
        chain_id: ChainID,
        keypair: Keypair,
        msg_sender: SenderHandle<N, PacemakerMessage>,
        block_tree: &BlockTree<K>,
        init_view: ViewNumber,
        init_leader: VerifyingKey,
        epoch_length: u32,
        view_time: Duration,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        Self {
            chain_id,
            keypair,
            state: PacemakerState::initialize(init_view, init_leader, block_tree, epoch_length, view_time),
            msg_sender,
            event_publisher,
        }
    }

    /// Query the pacemaker for updates to the current view.
    /// This method reads the internal state of the pacemaker ([PacemakerState]).
    pub(crate) fn get_view_updates(&self, view: ViewNumber) -> ViewUpdates {
        todo!()
    }

    /// Check the current time ('clock tick'), and
    /// possibly update the internal state of the pacemaker in response to the 'clock tick'.
    pub(crate) fn tick(&mut self) {
        todo!()
    }

    /// Update the internal state of the pacemaker and possibly the block tree 
    /// in response to receiving a [PacemakerMessage].
    pub(crate) fn on_receive_msg(
        &mut self, 
        msg: PacemakerMessage,
        origin: VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) {
        todo!()
    }

    /// Update the internal state of the pacemaker in response to receiving a [TimeoutVote].
    fn on_receive_timeout_vote(
        &mut self, 
        msg: TimeoutVote,
        origin: VerifyingKey,
        block_tree: &BlockTree<K>,
    ) {
        todo!()
    }

    /// Update the internal state of the pacemaker and possibly the block tree 
    /// in response to receiving an [AdvanceView].
    fn on_receive_advance_view(
        &mut self, 
        msg: AdvanceView,
        origin: VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) {
        todo!()
    }

    /// Implements the Interleaved Weighted Round Robin algorithm for selecting a view leader.
    /// For internal use by the [Pacemaker] and [PacemakerState] methods.
    pub(crate) fn view_leader(
        view: ViewNumber,
        validator_set: &ValidatorSet,
    ) -> VerifyingKey {
        todo!()
    }
}

struct PacemakerState {
    cur_view: ViewNumber,
    cur_leader: VerifyingKey,
    epoch_length: u32,
    view_time: Duration,
    timeouts: BTreeMap<ViewNumber, Instant>,
}

impl PacemakerState {

    fn initialize<K: KVStore>(
        init_view: ViewNumber,
        init_leader: VerifyingKey,
        block_tree: &BlockTree<K>, 
        epoch_length: u32, 
        view_time: Duration
    ) -> Self {
        Self { 
            cur_view: init_view, 
            cur_leader: init_leader,
            epoch_length,
            view_time,
            timeouts: BTreeMap::new(), 
        }
    }

    fn get_cur_view(&self) -> ViewNumber {
        self.cur_view
    }

    fn get_cur_leader(&self) -> VerifyingKey {
        self.cur_leader
    }

    /// Set the timeout for each view in the epoch starting from a given view.
    fn set_timeouts_for_epoch(&mut self, start_view: ViewNumber) {
        todo!()
    }

    /// Unique API for changing [PacemakerState]'s [cur_view](PacemakerState::cur_view) field.
    /// Guarantess that over the course of the protocol execution the value of this
    /// field is monotonically increasing.
    fn update_view<K: KVStore>(&mut self, block_tree: &BlockTree<K>) {

        // Get the max of self.cur_view, highest_qc().view, highested_entered_view, highest_tc().view,
        // and add 1
        todo!()
    }

}

enum ViewUpdates {
    RemainInView,
    EnterView{view: ViewNumber, leader: VerifyingKey}
}
