/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of the HotStuff protocol, adapted for dynamic validator sets.

use std::sync::mpsc::Sender;

use ed25519_dalek::VerifyingKey;

use crate::app::App;
use crate::events::Event;
use crate:: networking::{Network, SenderHandle, ValidatorSetUpdateHandle};
use crate::state::{BlockTree, KVStore};
use crate::types::{
    basic::{ChainID, ViewNumber}, 
    keypair::Keypair
};

use crate::hotstuff::messages::{BlockVote, HotStuffMessage, NewView, Nudge, Proposal};

pub struct HotStuff<N: Network, K: KVStore> {
    chain_id: ChainID,
    keypair: Keypair,
    sender_handle: SenderHandle<N, HotStuffMessage>,
    cur_view: ViewNumber,
    cur_leader: VerifyingKey,
    validator_set_update_handle: ValidatorSetUpdateHandle<N>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network, K: KVStore> HotStuff<N, K> {

    pub(crate) fn new(
        chain_id: ChainID,
        keypair: Keypair,
        sender_handle: SenderHandle<N, HotStuffMessage>,
        validator_set_update_handle: ValidatorSetUpdateHandle<N>,
        init_view: ViewNumber,
        init_leader: VerifyingKey,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        Self { 
            chain_id,
            keypair,
            sender_handle, 
            cur_view: init_view,
            cur_leader: init_leader,
            validator_set_update_handle, 
            event_publisher,
        }
    }

    pub(crate) fn on_enter_view(
        &mut self, view: u64, 
        leader: VerifyingKey,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) {

        // 1. Broadcast a NewView message for self.cur_view to leader

        // 2. Update cur_view and cur_leader to view and leader

        // 3. If I am cur_leader then broadcast a nudge or a proposal

        todo!()
    }

    pub(crate) fn on_receive_msg(
        &mut self, 
        msg: HotStuffMessage,
        origin: VerifyingKey,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) {
        todo!()
    }

    pub(crate) fn on_receive_proposal(
        &mut self, 
        proposal: Proposal, 
        origin: VerifyingKey,
        block_tree: &mut BlockTree<K>,
        app: &mut impl App<K>,
    ) {
        todo!()
    }

    pub(crate) fn on_receive_nudge(
        &mut self, 
        nudge: Nudge, 
        origin: VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) {
        todo!()
    }

    pub(crate) fn on_receive_vote(
        &mut self, 
        vote: BlockVote, 
        origin: VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) {
        todo!()
    }

    pub(crate) fn on_receive_new_view(
        &mut self, 
        new_view: NewView, 
        origin: VerifyingKey,
        block_tree: &mut BlockTree<K>,
    ) {
        todo!()
    }
}


