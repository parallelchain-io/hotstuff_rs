/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of the HotStuff protocol for Byzantine Fault Tolerant State Machine Replication,
//! adapted for dynamic validator sets.
//! TODO: describe how it works for dynamic validator sets.

use std::sync::mpsc::Sender;
use std::time::SystemTime;

use ed25519_dalek::VerifyingKey;

use crate::app::App;
use crate::events::{CommitBlockEvent, Event, PruneBlockEvent, UpdateHighestQCEvent, UpdateLockedQCEvent, UpdateValidatorSetEvent};
use crate:: networking::{Network, SenderHandle, ValidatorSetUpdateHandle};
use crate::pacemaker::protocol::ViewInfo;
use crate::state::block_tree::{self, BlockTree, BlockTreeError};
use crate::state::kv_store::KVStore;
use crate::state::safety;
use crate::state::write_batch::BlockTreeWriteBatch;
use crate::types::basic::CryptoHash;
use crate::types::validators::{ValidatorSet, ValidatorSetUpdates};
use crate::types::{
    basic::{ChainID, ViewNumber}, 
    keypair::Keypair
};
use crate::hotstuff::messages::{Vote, HotStuffMessage, NewView, Nudge, Proposal};
use crate::hotstuff::types::{NewViewCollector, VoteCollector};

use super::types::QuorumCertificate;

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

#[derive(Debug)]
pub enum HotStuffError {
    BlockTreeError(BlockTreeError)
}

impl From<BlockTreeError> for HotStuffError {
    fn from(value: BlockTreeError) -> Self {
        HotStuffError::BlockTreeError(value)
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
/// committing a block.
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
        wb.set_validator_set_update_complete(true)?
        // todo: emit an event for this
    }

    block_tree.write(wb);

    publish_update_block_tree_events(event_publisher, update_highest_qc, update_locked_qc, &committed_blocks);

    // Safety: a block that updates the validator set must be followed by a block that contains a commit
    // qc. A block becomes committed immediately if followed by a commit qc. Therefore, under normal
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
