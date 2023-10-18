/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Functions that implement the progress protocol and sync protocol.
//!
//! This module defines the algorithm thread and the procedures used in it. The algorithm thread is the driving
//! force of a HotStuff-rs replica. It receives and sends [progress messages](crate::messages::ProgressMessage) using
//! the network to cause the [Block Tree](crate::state) to grow in all replicas in a safe and live manner, as well as
//! sends out [sync requests](crate::messages::SyncRequest) to catch the replica up to the head of the block tree.
//!
//! The algorithm thread is a forever loop of increasing view numbers. On initially entering this loop, the replica
//! compares the view of the highest qc it knows with the highest view it has previously entered, selecting the higher
//! of the two, plus 1, as the view to execute.
//!
//! ## View sequence flow
//!
//! A view proceeds in one of two modes, depending on whether the depending on whether the local replica is a
//! [validator, or a listener](crate::replica). Replicas automatically change between the two modes as the validator
//! set evolves.
//!
//! The execution of a view in validator mode [proceeds](execute_view) as follows. The execution in listener view is
//! exactly the validator mode flow minus all of the proposing and voting:
//! 1. If I am the current leader (repeat this every proposal re-broadcast duration):
//!     * If the highest qc is a generic or commit qc, call the app to produce a new block, then, broadcast it in a
//!       proposal.
//!     * Otherwise, broadcast a nudge.
//! 2. Then, poll the network for progress messages until the view timeout:
//!     * [On receiving a proposal](on_receive_proposal):
//!         - Call the [safe_block](BlockTree::safe_block) function to see if the proposed block can be inserted.
//!         - Call on the app to validate it.
//!         - If it passes both checks, insert the block into the block tree and send a vote for it to the next leader.
//!         - Then, if I *am not* the next leader, move to the next view.
//!     * [On receiving a nudge](on_receive_nudge):
//!         - Check if it is a prepare or precommit qc.
//!         - Call the [safe_qc] function on the block tree to see if the qc can inserted.
//!         - If yes, send a vote for it to the next leader.
//!         - Check on the block tree if the qc can be set as the highest qc.
//!         - If yes, set it as the highest qc.
//!         - Then, if I *am not* the next leader, move to the next view.
//!     * [On receiving a vote](on_receive_vote):
//!         - Collect the vote.
//!         - If enough matching votes have been collected to form a quorum certificate, store it as the highest qc.
//!         - Then, if I *am* the next leader, move to the next view.
//!     * [On receving a new view](on_receive_new_view):
//!         - Check if the highest qc in the new view message is higher than our highest qc.
//!         - If yes, then store it as the highest qc.
//!         - Then, if I have collected new view messages from a quorum of validators this view, and I  *am* the next
//!           leader, move to the next view.
//! 3. If the view times out and reaches this step without reaching a step that transitions it into the next view, then
//!    send a new view message containing my highest qc to the next leader.

use crate::app::App;
use crate::app::ProduceBlockResponse;
use crate::app::{ProduceBlockRequest, ValidateBlockRequest, ValidateBlockResponse};
use crate::events::*;
use crate::messages::{Keypair, NewView, Nudge, ProgressMessage, Proposal, SyncRequest, Vote};
use crate::networking::*;
use crate::pacemaker::Pacemaker;
use crate::state::*;
use crate::types::*;
use std::cmp::max;
use std::sync::mpsc::{Sender, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::{Instant, SystemTime};

pub(crate) fn start_algorithm<K: KVStore, N: Network + 'static>(
    mut app: impl App<K> + 'static,
    me: Keypair,
    chain_id: ChainID,
    mut block_tree: BlockTree<K>,
    mut pacemaker: impl Pacemaker + 'static,
    mut pm_stub: ProgressMessageStub<N>,
    mut sync_stub: SyncClientStub<N>,
    shutdown_signal: Receiver<()>,
    event_publisher: Option<Sender<Event>>,
    sync_request_limit: u32,
    sync_response_timeout: Duration,
    sync_trigger_timeout: Duration,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut cur_view = 0;
        let mut sync_trigger_timer = SyncTriggerTimer::new(sync_trigger_timeout);

        loop {
            match shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => {
                    panic!("Algorithm thread disconnected from main thread")
                }
            }

            cur_view = max(
                cur_view,
                max(
                    block_tree.highest_view_entered(),
                    block_tree.highest_qc().view,
                ),
            ) + 1;
            block_tree.set_highest_view_entered(cur_view);

            let view_result = execute_view(
                &me,
                chain_id,
                cur_view,
                &mut pacemaker,
                &mut block_tree,
                &mut pm_stub,
                &mut app,
                &event_publisher,
                &mut sync_trigger_timer,
            );
            if let Err(ShouldSync) = view_result {
                sync(&mut block_tree, &mut sync_stub, &mut app, chain_id, &event_publisher, sync_request_limit, sync_response_timeout, &mut sync_trigger_timer)
            }
        }
    })
}

/// Returns an Err(ShouldSync) if we encountered a quorum certificate for a future view in the execution, which suggests that a quorum of replicas are making
/// progress at a higher view, so we should probably sync up to them.
fn execute_view<K: KVStore, N: Network>(
    me: &Keypair,
    chain_id: ChainID,
    view: ViewNumber,
    pacemaker: &mut impl Pacemaker,
    block_tree: &mut BlockTree<K>,
    pm_stub: &mut ProgressMessageStub<N>,
    app: &mut impl App<K>,
    event_publisher: &Option<Sender<Event>>,
    sync_trigger_timer: &mut SyncTriggerTimer,
) -> Result<(), ShouldSync> {

    let timeout = pacemaker.view_timeout(view, block_tree.highest_qc().view);
    let view_deadline = Instant::now() + timeout;

    let cur_validators = block_tree.committed_validator_set();
    let cur_leader = pacemaker.view_leader(view, &cur_validators);
    let i_am_cur_leader = me.public() == cur_leader;

    Event::StartView(StartViewEvent { timestamp: SystemTime::now(), leader: cur_leader, view}).publish(event_publisher);

    // 1. If I am the current leader, broadcast a proposal or a nudge.
    if i_am_cur_leader {
        propose_or_nudge(chain_id, view, app, block_tree, pm_stub, event_publisher);
    }

    // 2. If I am a voter or the next leader, receive and progress messages until view timeout.
    let mut vote_collector = VoteCollector::new(chain_id, view, cur_validators.clone());
    let mut new_view_collector = NewViewCollector::new(cur_validators.clone());
    while Instant::now() < view_deadline {
        match pm_stub.recv(chain_id, view, view_deadline) {
            Ok((origin, msg)) => match msg {
                ProgressMessage::Proposal(proposal) => {
                    if origin == cur_leader {
                        let (voted, i_am_next_leader) = on_receive_proposal(
                            &origin,
                            &proposal,
                            me,
                            chain_id,
                            view,
                            block_tree,
                            app,
                            pacemaker,
                            pm_stub,
                            &mut vote_collector,
                            &mut new_view_collector,
                            &event_publisher,
                            sync_trigger_timer,
                        );
                        if voted && !i_am_next_leader {
                            return Ok(());
                        }
                    }
                }
                ProgressMessage::Nudge(nudge) => {
                    if origin == cur_leader {
                        let (voted, i_am_next_leader) = on_receive_nudge(
                            &origin, &nudge, me, chain_id, view, block_tree, pacemaker, pm_stub, event_publisher, sync_trigger_timer
                        );
                        if voted && !i_am_next_leader {
                            return Ok(());
                        }
                    }
                }
                ProgressMessage::Vote(vote) => {
                    let (highest_qc_updated, i_am_next_leader) = on_receive_vote(
                        &origin,
                        vote,
                        &mut vote_collector,
                        block_tree,
                        chain_id,
                        view,
                        me,
                        pacemaker,
                        event_publisher,
                    );
                    if highest_qc_updated && i_am_next_leader {
                        return Ok(());
                    }
                }
                ProgressMessage::NewView(new_view) => {
                    let received_new_views_from_quorum_and_i_am_next_leader = on_receive_new_view(
                        &origin,
                        new_view,
                        &mut new_view_collector,
                        block_tree,
                        chain_id,
                        view,
                        me,
                        pacemaker,
                        event_publisher,
                    );
                    if received_new_views_from_quorum_and_i_am_next_leader {
                        return Ok(());
                    }
                }
            },
            Err(ProgressMessageReceiveError::Timeout) => continue,
        };
        if sync_trigger_timer.timeout() {
            return Err(ShouldSync)
        }
    }

    // 3. If the view times out, send a New View message.
    Event::ViewTimeout(ViewTimeoutEvent { timestamp: SystemTime::now(), view, timeout}).publish(event_publisher);

    on_view_timeout(chain_id, view, block_tree, pacemaker, pm_stub, event_publisher);

    Ok(())
}

struct ShouldSync;

/// Create a proposal or a nudge and broadcast it to the network.
fn propose_or_nudge<K: KVStore, N: Network>(
    chain_id: ChainID,
    cur_view: ViewNumber,
    app: &mut impl App<K>,
    block_tree: &mut BlockTree<K>,
    pm_stub: &mut ProgressMessageStub<N>,
    event_publisher: &Option<Sender<Event>>,
) {
    let highest_qc = block_tree.highest_qc();
    match highest_qc.phase {
        // Produce a proposal.
        Phase::Generic | Phase::Commit(_) => {
            let (parent_block, child_height) = if highest_qc.is_genesis_qc() {
                (None, 0)
            } else {
                (
                    Some(highest_qc.block),
                    block_tree.block_height(&highest_qc.block).unwrap() + 1,
                )
            };

            let produce_block_request = ProduceBlockRequest::new(
                cur_view,
                parent_block,
                block_tree.app_view(parent_block.as_ref()),
            );

            let ProduceBlockResponse {
                data,
                data_hash,
                app_state_updates: _,
                validator_set_updates: _,
            } = app.produce_block(produce_block_request);

            let block = Block::new(child_height, highest_qc, data_hash, data);

            let proposal_msg = ProgressMessage::proposal(chain_id, cur_view, block);
            let proposal = if let ProgressMessage::Proposal(proposal) = proposal_msg.clone() {
                proposal
            } else {
                panic!("must be ProgressMessage::Proposal(proposal)")
            };

            pm_stub.broadcast(proposal_msg);
            Event::Propose(ProposeEvent { timestamp: SystemTime::now(), proposal}).publish(event_publisher)
        }

        // Produce a nudge.
        Phase::Prepare | Phase::Precommit(_) => {
            let nudge_msg = ProgressMessage::nudge(chain_id, cur_view, highest_qc);
            let nudge = if let ProgressMessage::Nudge(nudge) = nudge_msg.clone() {
                nudge
            } else {
                panic!("must be ProgressMessage::Nudge(nudge)")
            };

            pm_stub.broadcast(nudge_msg);
            Event::Nudge(NudgeEvent { timestamp: SystemTime::now(), nudge}).publish(event_publisher)
        }
    };
}

// Returns (whether I voted, whether I am the next leader).
fn on_receive_proposal<K: KVStore, N: Network>(
    origin: &PublicKey,
    proposal: &Proposal,
    me: &Keypair,
    chain_id: ChainID,
    cur_view: ViewNumber,
    block_tree: &mut BlockTree<K>,
    app: &mut impl App<K>,
    pacemaker: &mut impl Pacemaker,
    pm_stub: &mut ProgressMessageStub<N>,
    vote_collector: &mut VoteCollector,
    new_view_collector: &mut NewViewCollector,
    event_publisher: &Option<Sender<Event>>,
    sync_trigger_timer: &mut SyncTriggerTimer,
) -> (bool, bool) {
    Event::ReceiveProposal(ReceiveProposalEvent { timestamp: SystemTime::now(), origin: *origin, proposal: proposal.clone()}).publish(event_publisher);

    if !proposal
        .block
        .is_correct(&block_tree.committed_validator_set())
        || !block_tree.safe_block(&proposal.block, chain_id)
    {
        let next_leader =
            pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
        let i_am_next_leader = me.public() == next_leader;
        return (false, i_am_next_leader);
    }

    let parent_block = if proposal.block.justify.is_genesis_qc() {
        None
    } else {
        Some(&proposal.block.justify.block)
    };
    let validate_block_request =
        ValidateBlockRequest::new(&proposal.block, block_tree.app_view(parent_block));

    if let ValidateBlockResponse::Valid {
        app_state_updates,
        validator_set_updates,
    } = app.validate_block(validate_block_request)
    {   
        // Progress: on receiving acceptable proposal
        sync_trigger_timer.update();

        let validator_set_updates_because_of_commit = block_tree.insert_block(
            &proposal.block,
            app_state_updates.as_ref(),
            validator_set_updates.as_ref(),
            event_publisher,
        );
        if let Some(updates) = validator_set_updates_because_of_commit {
            pm_stub.update_validator_set(updates);
            *vote_collector = VoteCollector::new(
                chain_id,
                cur_view,
                block_tree.committed_validator_set(),
            );
            *new_view_collector = NewViewCollector::new(block_tree.committed_validator_set());
        }

        let i_am_validator = block_tree.committed_validator_set().contains(&me.public());
        let next_leader =
            pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
        if i_am_validator {
            let vote_phase = if validator_set_updates.is_some() {
                Phase::Prepare
            } else {
                Phase::Generic
            };
            let vote_msg = me.vote(chain_id, cur_view, proposal.block.hash, vote_phase);
            let vote = if let ProgressMessage::Vote(vote) = vote_msg.clone() {
                vote
            } else {
                panic!("must be ProgressMessage::Vote(vote)")
            };

            pm_stub.send(next_leader, vote_msg);
            Event::Vote(VoteEvent { timestamp: SystemTime::now(), vote}).publish(event_publisher)
        }

        let i_am_next_leader = me.public() == next_leader;

        (true, i_am_next_leader)
    } else {
        let next_leader =
            pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
        let i_am_next_leader = me.public() == next_leader;

        (false, i_am_next_leader)
    }
}

// Returns (whether I voted, whether I am the next leader).
fn on_receive_nudge<K: KVStore, N: Network>(
    origin: &PublicKey,
    nudge: &Nudge,
    me: &Keypair,
    chain_id: ChainID,
    cur_view: ViewNumber,
    block_tree: &mut BlockTree<K>,
    pacemaker: &mut impl Pacemaker,
    pm_stub: &mut ProgressMessageStub<N>,
    event_publisher: &Option<Sender<Event>>,
    sync_trigger_timer: &mut SyncTriggerTimer,
) -> (bool, bool) {
    Event::ReceiveNudge(ReceiveNudgeEvent { timestamp: SystemTime::now(), origin: *origin, nudge: nudge.clone()}).publish(event_publisher);

    if nudge.justify.phase.is_generic()
        || nudge.justify.phase.is_commit()
        || !nudge
            .justify
            .is_correct(&block_tree.committed_validator_set())
        || !block_tree.safe_qc(&nudge.justify, chain_id)
    {
        let next_leader =
            pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
        let i_am_next_leader = me.public() == next_leader;
        return (false, i_am_next_leader);
    }

    // Progress: on receiving acceptable nudge
    sync_trigger_timer.update();

    let mut wb = BlockTreeWriteBatch::new();
    let mut update_highest_qc: Option<QuorumCertificate> = None;
    let mut update_locked_view: Option<ViewNumber> = None;
    if nudge.justify.view > block_tree.highest_qc().view {
        wb.set_highest_qc(&nudge.justify);
        update_highest_qc = Some(nudge.justify.clone());
    }
    let next_phase = match nudge.justify.phase {
        Phase::Prepare => {
            let current_block = nudge.justify.block;
            let current_block_justify_view = block_tree.block_justify(&current_block).unwrap().view;
            if current_block_justify_view > block_tree.locked_view() {
                wb.set_locked_view(current_block_justify_view);
                update_locked_view = Some(current_block_justify_view);
            }
            Phase::Precommit(nudge.justify.view)
        },

        Phase::Precommit(prepare_qc_view) => {
            if prepare_qc_view > block_tree.locked_view() {
                wb.set_locked_view(prepare_qc_view);
                update_locked_view = Some(prepare_qc_view);
            }

            Phase::Commit(nudge.justify.view)
        }

        Phase::Generic | Phase::Commit(_) => unreachable!(),
    };
    block_tree.write(wb);

    if let Some(highest_qc) = update_highest_qc {
        Event::UpdateHighestQC(UpdateHighestQCEvent { timestamp: SystemTime::now(), highest_qc}).publish(event_publisher)
    }
    if let Some(locked_view) = update_locked_view {
        Event::UpdateLockedView(UpdateLockedViewEvent { timestamp: SystemTime::now(), locked_view}).publish(event_publisher)
    }

    let vote_msg = me.vote(chain_id, cur_view, nudge.justify.block, next_phase);
    let vote = if let ProgressMessage::Vote(vote) = vote_msg.clone() {
        vote
    } else {
        panic!("must be ProgressMessage::Vote(vote)")
    };

    let i_am_validator = block_tree.committed_validator_set().contains(&me.public());
    let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
    if i_am_validator {
        pm_stub.send(next_leader, vote_msg);
        Event::Vote(VoteEvent{ timestamp: SystemTime::now(), vote}).publish(event_publisher);
    }

    let i_am_next_leader = me.public() == next_leader;

    (true, i_am_next_leader)
}

// Returns (whether a new highest qc was collected, i am the next leader).
fn on_receive_vote<K: KVStore>(
    signer: &PublicKey,
    vote: Vote,
    votes: &mut VoteCollector,
    block_tree: &mut BlockTree<K>,
    chain_id: ChainID,
    cur_view: ViewNumber,
    me: &Keypair,
    pacemaker: &mut impl Pacemaker,
    event_publisher: &Option<Sender<Event>>,
) -> (bool, bool) {
    Event::ReceiveVote(ReceiveVoteEvent { timestamp: SystemTime::now(), origin: *signer, vote: vote.clone()}).publish(event_publisher);

    let highest_qc_updated = if vote.is_correct(signer) {
        if let Some(new_qc) = votes.collect(signer, vote) {
            if block_tree.safe_qc(&new_qc, chain_id) {
                Event::CollectQC(CollectQCEvent { timestamp: SystemTime::now(), quorum_certificate: new_qc.clone()}).publish(event_publisher);

                let mut wb = BlockTreeWriteBatch::new();
                let mut update_highest_qc: Option<QuorumCertificate> = None;
                let mut update_locked_view: Option<ViewNumber> = None;

                if Phase::Prepare == new_qc.phase {
                    let current_block_justify_view = block_tree.block_justify(&new_qc.block).unwrap().view;
                    if current_block_justify_view > block_tree.locked_view() {
                        wb.set_locked_view(current_block_justify_view);
                        update_locked_view = Some(current_block_justify_view);
                    }
                }

                if let Phase::Precommit(prepare_qc_view) = new_qc.phase {
                    if prepare_qc_view > block_tree.locked_view() {
                        wb.set_locked_view(prepare_qc_view);
                        update_locked_view = Some(prepare_qc_view);
                    }
                }

                let highest_qc_updated = if new_qc.view > block_tree.highest_qc().view {
                    wb.set_highest_qc(&new_qc);
                    update_highest_qc = Some(new_qc.clone());
                    true
                } else {
                    false
                };
                block_tree.write(wb);
                if let Some(highest_qc) = update_highest_qc {
                    Event::UpdateHighestQC(UpdateHighestQCEvent { timestamp: SystemTime::now(), highest_qc}).publish(event_publisher);
                }
                if let Some(locked_view) = update_locked_view {
                    Event::UpdateLockedView(UpdateLockedViewEvent { timestamp: SystemTime::now(), locked_view}).publish(event_publisher);
                }

                highest_qc_updated
            } else {
                // The newly-formed QC is not safe (e.g., its pointing to a block this replica doesn't know).
                false
            }
        } else {
            // The vote is not enough to form a new QC.
            false
        }
    } else {
        // The vote is not signed correctly.
        false
    };

    let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
    let i_am_next_leader = me.public() == next_leader;

    (highest_qc_updated, i_am_next_leader)
}

// returns (whether I have collected new view messages from a quorum of validators && I am the next leader)
fn on_receive_new_view<K: KVStore>(
    origin: &PublicKey,
    new_view: NewView,
    new_view_collector: &mut NewViewCollector,
    block_tree: &mut BlockTree<K>,
    chain_id: ChainID,
    cur_view: ViewNumber,
    me: &Keypair,
    pacemaker: &mut impl Pacemaker,
    event_publisher: &Option<Sender<Event>>,
) -> bool {
    Event::ReceiveNewView(ReceiveNewViewEvent { timestamp: SystemTime::now(), origin: *origin, new_view: new_view.clone()}).publish(event_publisher);

    if new_view
        .highest_qc
        .is_correct(&block_tree.committed_validator_set())
        && block_tree.safe_qc(&new_view.highest_qc, chain_id)
    {
        let mut wb = BlockTreeWriteBatch::new();
        let mut update_highest_qc: Option<QuorumCertificate> = None;
        let mut update_locked_view: Option<ViewNumber> = None;

        if Phase::Prepare == new_view.highest_qc.phase {
            let current_block_justify_view = block_tree.block_justify(&new_view.highest_qc.block).unwrap().view;
            if current_block_justify_view > block_tree.locked_view() {
                wb.set_locked_view(current_block_justify_view);
                update_locked_view = Some(current_block_justify_view);
            }
        }

        if let Phase::Precommit(prepare_qc_view) = new_view.highest_qc.phase {
            if prepare_qc_view > block_tree.locked_view() {
                wb.set_locked_view(prepare_qc_view);
                update_locked_view = Some(prepare_qc_view);
            }
        }

        if new_view.highest_qc.view > block_tree.highest_qc().view {
            wb.set_highest_qc(&new_view.highest_qc);
            update_highest_qc = Some(new_view.highest_qc);
        };

        block_tree.write(wb);
        if let Some(highest_qc) = update_highest_qc {
            Event::UpdateHighestQC(UpdateHighestQCEvent { timestamp: SystemTime::now(), highest_qc}).publish(event_publisher)
        }
        if let Some(locked_view) = update_locked_view {
            Event::UpdateLockedView(UpdateLockedViewEvent { timestamp: SystemTime::now(), locked_view}).publish(event_publisher)
        }

        if new_view_collector.collect(origin) {
            let next_leader =
                pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
            let i_am_next_leader = me.public() == next_leader;

            return i_am_next_leader;
        }
    }

    false
}

fn on_view_timeout<K: KVStore, N: Network>(
    chain_id: ChainID,
    cur_view: ViewNumber,
    block_tree: &BlockTree<K>,
    pacemaker: &mut impl Pacemaker,
    pm_stub: &mut ProgressMessageStub<N>,
    event_publisher: &Option<Sender<Event>>,
) {
    let new_view_msg = ProgressMessage::new_view(chain_id, cur_view, block_tree.highest_qc());
    let new_view = if let ProgressMessage::NewView(new_view) = new_view_msg.clone() {
        new_view
    } else {
        panic!("must be ProgressMessage::NewView(new_view)")
    };

    let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());

    pm_stub.send(next_leader, new_view_msg);
    Event::NewView(NewViewEvent { timestamp: SystemTime::now(), new_view}).publish(event_publisher);
}

fn sync<K: KVStore, N: Network>(
    block_tree: &mut BlockTree<K>,
    sync_stub: &mut SyncClientStub<N>,
    app: &mut impl App<K>,
    chain_id: ChainID,
    event_publisher: &Option<Sender<Event>>,
    sync_request_limit: u32,
    sync_response_timeout: Duration,
    sync_trigger_timer: &mut SyncTriggerTimer,
) {
    // Pick random validator.
    if let Some(peer) = block_tree.committed_validator_set().random() {
        sync_with(peer, block_tree, sync_stub, app, chain_id, event_publisher, sync_request_limit, sync_response_timeout, sync_trigger_timer);
    }
}

fn sync_with<K: KVStore, N: Network>(
    peer: &PublicKey,
    block_tree: &mut BlockTree<K>,
    sync_stub: &mut SyncClientStub<N>,
    app: &mut impl App<K>,
    chain_id: ChainID,
    event_publisher: &Option<Sender<Event>>,
    sync_request_limit: u32,
    sync_response_timeout: Duration,
    sync_trigger_timer: &mut SyncTriggerTimer,
) {
    Event::StartSync(StartSyncEvent { timestamp: SystemTime::now(), peer: peer.clone()}).publish(event_publisher);
    let mut blocks_synced = 0;

    loop {
        let request = SyncRequest {
            start_height: if let Some(height) = block_tree.highest_committed_block_height() {
                height + 1
            } else {
                0
            },
            limit: sync_request_limit,
        };
        sync_stub.send_request(*peer, request);

        match sync_stub.recv_response(*peer, Instant::now() + sync_response_timeout) {
            Some(response) => {
                let new_blocks: Vec<Block> = response
                    .blocks
                    .into_iter()
                    .skip_while(|block| block_tree.contains(&block.hash))
                    .collect();
                if new_blocks.is_empty() {
                    Event::EndSync(EndSyncEvent { timestamp: SystemTime::now(), peer: *peer, blocks_synced}).publish(event_publisher);
                    return;
                }

                for block in new_blocks {
                    if !block.is_correct(&block_tree.committed_validator_set())
                        || !block_tree.safe_block(&block, chain_id)
                    {
                        Event::EndSync(EndSyncEvent { timestamp: SystemTime::now(), peer: *peer, blocks_synced}).publish(event_publisher);
                        return;
                    }

                    let parent_block = if block.justify.is_genesis_qc() {
                        None
                    } else {
                        Some(&block.justify.block)
                    };
                    let validate_block_request =
                        ValidateBlockRequest::new(&block, block_tree.app_view(parent_block));
                    if let ValidateBlockResponse::Valid {
                        app_state_updates,
                        validator_set_updates,
                    } = app.validate_block_for_sync(validate_block_request)
                    {   
                        // Progress: on receiving acceptable block via sync
                        sync_trigger_timer.update();

                        let validator_set_updates_because_of_commit = block_tree.insert_block(
                            &block,
                            app_state_updates.as_ref(),
                            validator_set_updates.as_ref(),
                            event_publisher,
                        );

                        if let Some(updates) = validator_set_updates_because_of_commit {
                            sync_stub.update_validator_set(updates);
                        }

                        blocks_synced += 1;
                    } else {
                        Event::EndSync(EndSyncEvent { timestamp: SystemTime::now(), peer: *peer, blocks_synced}).publish(event_publisher);
                        return;
                    }
                }

                if block_tree.safe_qc(&response.highest_qc, chain_id)
                    && response.highest_qc.view > block_tree.highest_qc().view
                {   
                    block_tree.set_highest_qc(&response.highest_qc);
                    Event::UpdateHighestQC(UpdateHighestQCEvent { timestamp: SystemTime::now(), highest_qc: response.highest_qc.clone()}).publish(event_publisher)
                }

            },
            None => {
                Event::EndSync(EndSyncEvent { timestamp: SystemTime::now(), peer: *peer, blocks_synced}).publish(event_publisher);
                return;
            }
        }
    }
}
