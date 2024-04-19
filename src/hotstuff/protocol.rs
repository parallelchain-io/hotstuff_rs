/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of the HotStuff protocol for Byzantine Fault Tolerant State Machine Replication,
//! adapted for dynamic validator sets.
//! TODO: describe how it works for dynamic validator sets.

use std::sync::mpsc::Sender;

use ed25519_dalek::VerifyingKey;

use crate::app::App;
use crate::events::Event;
use crate:: networking::{Network, SenderHandle, ValidatorSetUpdateHandle};
use crate::pacemaker::protocol::ViewInfo;
use crate::state::block_tree::BlockTree;
use crate::state::kv_store::KVStore;
use crate::types::validators::ValidatorSet;
use crate::types::{
    basic::{ChainID, ViewNumber}, 
    keypair::Keypair
};
use crate::hotstuff::messages::{Vote, HotStuffMessage, NewView, Nudge, Proposal};
use crate::hotstuff::types::{NewViewCollector, VoteCollector};

/// An implementation of the HotStuff protocol (https://arxiv.org/abs/1803.05069), adapted to enable
/// dynamic validator sets. The protocol operates on a per-view basis, where in each view the validator
/// exchanges messages with other validators, and updates the [block tree][BlockTree]. The 
/// [HotStuffState] reflects the view-specific parameters that define how the validator communicates.
pub(crate) struct HotStuff<N: Network> {
    config: HotStuffConfiguration,
    state: HotStuffState,
    view_info: ViewInfo,
    sender_handle: SenderHandle<N>,
    validator_set_update_handle: ValidatorSetUpdateHandle<N>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network> HotStuff<N> {

    pub(crate) fn new(
        config: HotStuffConfiguration,
        view_info: ViewInfo,
        sender_handle: SenderHandle<N>,
        validator_set_update_handle: ValidatorSetUpdateHandle<N>,
        init_validator_set: ValidatorSet,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        let state = HotStuffState::new(
            config.clone(), 
            view_info.view, 
            init_validator_set
        );
        Self { 
            config,
            view_info,
            state,
            sender_handle, 
            validator_set_update_handle, 
            event_publisher,
        }
    }

    pub(crate) fn on_receive_view_info<K: KVStore>(
        &mut self, 
        view_info: ViewInfo,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) {

        // 1. Broadcast a NewView message for self.cur_view to leader.

        // 2. Update cur_view and cur_leader to view and leader, next_leader tp next_leader.

        // 3. If I am cur_leader then broadcast a nudge or a proposal.

        todo!()
    }

    pub(crate) fn on_receive_msg<K: KVStore>(
        &mut self, 
        msg: HotStuffMessage,
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) {
        todo!()
    }

    fn on_receive_proposal<K: KVStore>(
        &mut self, 
        proposal: &Proposal, 
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) {
        todo!()
    }

    fn on_receive_nudge<K: KVStore>(
        &mut self, 
        nudge: &Nudge, 
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) {
        todo!()
    }

    fn on_receive_vote<K: KVStore>(
        &mut self, 
        vote: &Vote, 
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) {
        todo!()
    }

    fn on_receive_new_view<K: KVStore>(
        &mut self, 
        new_view: &NewView, 
        origin: &VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) {
        todo!()
    }

}

/// Immutable parameters that define the behaviour of the [HotStuff] protocol and should never change.
#[derive(Clone)]
pub(crate) struct HotStuffConfiguration {
    pub(crate) chain_id: ChainID,
    pub(crate) keypair: Keypair,
}

/// Internal state of the [HotStuff] protocol. Stores the [Vote] and [NewView] messages collected
/// in the current view. This state should always be updated on entering a view.
struct HotStuffState {
    vote_collector: VoteCollector,
    new_view_collector: NewViewCollector,
}

impl HotStuffState {
    /// Returns a fresh [HotStuffState] for a given view, leader, and validator set.
    fn new(
        config: HotStuffConfiguration,
        view: ViewNumber,
        validator_set: ValidatorSet,
    ) -> Self {
        Self {
            vote_collector: VoteCollector::new(config.chain_id, view, validator_set.clone()),
            new_view_collector: NewViewCollector::new(validator_set),
        }
    }
}
