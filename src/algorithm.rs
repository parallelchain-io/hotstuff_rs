/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
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


use std::sync::mpsc::{Receiver, TryRecvError};
use std::cmp::max;
use std::time::Instant;
use std::thread::{self, JoinHandle};
use crate::logging::{info, debug};
use crate::app::{ProduceBlockRequest, ValidateBlockResponse, ValidateBlockRequest};
use crate::app::ProduceBlockResponse;
use crate::messages::{Keypair, ProgressMessage, Vote, NewView, Proposal, Nudge, SyncRequest};
use crate::pacemaker::Pacemaker;
use crate::types::*;
use crate::state::*;
use crate::networking::*;
use crate::app::App;

pub(crate) fn start_algorithm<K: KVStore, N: Network + 'static>(
    mut app: impl App<K> + 'static,
    me: Keypair,
    mut block_tree: BlockTree<K>,
    mut pacemaker: impl Pacemaker + 'static,
    mut pm_stub: ProgressMessageStub<N>,
    mut sync_stub: SyncClientStub<N>,
    shutdown_signal: Receiver<()>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut cur_view = 0;

        loop {
            match shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => panic!("Algorithm thread disconnected from main thread"),
            }

            cur_view = max(cur_view, max(block_tree.highest_view_entered(), block_tree.highest_qc().view)) + 1;
            block_tree.set_highest_entered_view(cur_view);

            let view_result = execute_view(&me, cur_view, &mut pacemaker, &mut block_tree, &mut pm_stub, &mut app);
            if let Err(ShouldSync) = view_result {
                sync(&mut block_tree, &mut sync_stub, &mut app, &mut pacemaker)
            }
        }
    })
}

/// Returns an Err(ShouldSync) if we encountered a quorum certificate for a future view in the execution, which suggests that a quorum of replicas are making
/// progress at a higher view, so we should probably sync up to them.
fn execute_view<K: KVStore, N: Network>(
    me: &Keypair,
    view: ViewNumber,
    pacemaker: &mut impl Pacemaker,
    mut block_tree: &mut BlockTree<K>,
    mut pm_stub: &mut ProgressMessageStub<N>,
    app: &mut impl App<K>,
) -> Result<(), ShouldSync> {
    debug::entered_view(view);
    let view_deadline = Instant::now() + pacemaker.view_timeout(view, block_tree.highest_qc().view);
    
    let cur_validators = block_tree.committed_validator_set();
    let cur_leader = pacemaker.view_leader(view, &cur_validators);
    let i_am_cur_leader = me.public() == cur_leader;

    // 1. If I am the current leader, broadcast a proposal or a nudge.
    if i_am_cur_leader {
        propose_or_nudge(view, app, block_tree, &mut pm_stub);
    }

    // 2. If I am a voter or the next leader, receive and progress messages until view timeout.     
    let mut vote_collector = VoteCollector::new(app.id(), view, cur_validators.clone());
    let mut new_view_collector = NewViewCollector::new(cur_validators.clone());
    while Instant::now() < view_deadline { 
        match pm_stub.recv(app.id(), view, view_deadline) {
            Ok((origin, msg)) => match msg {
                ProgressMessage::Proposal(proposal) => if origin == cur_leader {
                    let (voted, i_am_next_leader) = on_receive_proposal(&origin, &proposal, &me, view, &mut block_tree, app, pacemaker, pm_stub, &mut vote_collector, &mut new_view_collector);
                    if voted {
                        if !i_am_next_leader {
                            return Ok(())
                        }
                    }
                },
                ProgressMessage::Nudge(nudge) => if origin == cur_leader {
                    let (voted, i_am_next_leader) = on_receive_nudge(&origin, &nudge, &me, view, &mut block_tree, app, pacemaker, pm_stub);
                    if voted {
                        if !i_am_next_leader {
                            return Ok(())
                        }
                    }
                },
                ProgressMessage::Vote(vote) => {
                    let (highest_qc_updated, i_am_next_leader) = on_receive_vote(&origin, vote, &mut vote_collector, &mut block_tree, view, &me, pacemaker);
                    if highest_qc_updated && i_am_next_leader {
                        return Ok(())
                    }
                },
                ProgressMessage::NewView(new_view) => {
                    let received_new_views_from_quorum_and_i_am_next_leader = on_receive_new_view(&origin, new_view, &mut new_view_collector, &mut block_tree, view, &me, pacemaker);
                    if received_new_views_from_quorum_and_i_am_next_leader {
                        return Ok(())
                    }
                },
            },
            Err(ProgressMessageReceiveError::ReceivedQCFromFuture) => return Err(ShouldSync),
            Err(ProgressMessageReceiveError::Timeout) => continue,
        }
    }

    // 3. If the view times out, send a New View message.
    on_view_timeout(view, app, block_tree, pacemaker, pm_stub);

    Ok(())
}

struct ShouldSync;

/// Create a proposal or a nudge and broadcast it to the network.
fn propose_or_nudge<K: KVStore, N: Network>(
    cur_view: ViewNumber,
    app: &mut impl App<K>,
    block_tree: &mut BlockTree<K>,
    pm_stub: &mut ProgressMessageStub<N>,
) {
    let highest_qc = block_tree.highest_qc();
    match () {
        // Produce a proposal.
        () if highest_qc.phase.is_generic() || highest_qc.phase.is_commit() => {
            let (parent_block, child_height) = if highest_qc.is_genesis_qc() {
                (None, 0) 
            } else {
                (Some(highest_qc.block), block_tree.block_height(&highest_qc.block).unwrap() + 1)
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
                validator_set_updates: _ 
            } = app.produce_block(produce_block_request);

            let block = Block::new(child_height, highest_qc, data_hash, data);
            
            let proposed_block_hash = block.hash;
            let proposed_block_height = block.height;
            let proposal = ProgressMessage::proposal(app.id(), cur_view, block);

            pm_stub.broadcast(proposal);
            info::proposed(cur_view, &proposed_block_hash, proposed_block_height);
        },

        // Produce a nudge.
        () if highest_qc.phase.is_prepare() || highest_qc.phase.is_precommit() => {
            let nudged_block_hash = highest_qc.block;

            let highest_qc_phase = highest_qc.phase;
            let nudge = ProgressMessage::nudge(app.id(), cur_view, highest_qc);

            pm_stub.broadcast(nudge);
            info::nudged(cur_view, &nudged_block_hash, block_tree.block_height(&nudged_block_hash).unwrap(), highest_qc_phase);
        },

        _ => unreachable!()
    };
}

// Returns (whether I voted, whether I am the next leader).
fn on_receive_proposal<K: KVStore, N: Network>(
    origin: &PublicKeyBytes,
    proposal: &Proposal,
    me: &Keypair,
    cur_view: ViewNumber,
    block_tree: &mut BlockTree<K>,
    app: &mut impl App<K>,
    pacemaker: &mut impl Pacemaker,
    pm_stub: &mut ProgressMessageStub<N>,
    vote_collector: &mut VoteCollector,
    new_view_collector: &mut NewViewCollector,
) -> (bool, bool) {
    debug::received_proposal(origin, &proposal.block.hash, proposal.block.height);
    if !proposal.block.is_correct(&block_tree.committed_validator_set()) || !block_tree.safe_block(&proposal.block) {
        let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
        let i_am_next_leader = me.public() == next_leader;
        return (false, i_am_next_leader)
    }

    let parent_block = if proposal.block.justify.is_genesis_qc() {
        None
    } else {
        Some(&proposal.block.justify.block)
    };
    let validate_block_request = ValidateBlockRequest::new(
        &proposal.block,
        block_tree.app_view(parent_block),
    );

    if let ValidateBlockResponse::Valid {
        app_state_updates,
        validator_set_updates,
    } = app.validate_block(validate_block_request) {

        let validator_set_updates_because_of_commit = block_tree.insert_block(&proposal.block, app_state_updates.as_ref(), validator_set_updates.as_ref()); 
        if let Some(updates) = validator_set_updates_because_of_commit {
            pm_stub.update_validator_set(updates);
            *vote_collector = VoteCollector::new(app.id(), cur_view, block_tree.committed_validator_set());
            *new_view_collector = NewViewCollector::new(block_tree.committed_validator_set());
        }

        let i_am_validator = block_tree.committed_validator_set().contains(&me.public());
        let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
        if i_am_validator {
            let vote_phase = if validator_set_updates.is_some() { Phase::Prepare } else { Phase::Generic };
            let vote = me.vote(app.id(), cur_view, proposal.block.hash, vote_phase);

            pm_stub.send(next_leader, vote.clone());
            info::voted(cur_view, &proposal.block.hash, proposal.block.height, vote_phase);
        } 

        let i_am_next_leader = me.public() == next_leader;

        (true, i_am_next_leader)
    } else {
        let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
        let i_am_next_leader = me.public() == next_leader;

        (false, i_am_next_leader)
    }
}

// Returns (whether I voted, whether I am the next leader).
fn on_receive_nudge<K: KVStore, N: Network>(
    origin: &PublicKeyBytes,
    nudge: &Nudge,
    me: &Keypair,
    cur_view: ViewNumber,
    block_tree: &mut BlockTree<K>,
    app: &mut impl App<K>,
    pacemaker: &mut impl Pacemaker,
    pm_stub: &mut ProgressMessageStub<N>,
) -> (bool, bool) {
    debug::received_nudge(origin, &nudge.justify.block, nudge.justify.phase);

    if nudge.justify.phase.is_generic() 
        || nudge.justify.phase.is_commit()
        || !nudge.justify.is_correct(&block_tree.committed_validator_set()) 
        || !block_tree.safe_qc(&nudge.justify) {
        let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
        let i_am_next_leader = me.public() == next_leader;
        return (false, i_am_next_leader) 
    }

    let mut wb = BlockTreeWriteBatch::new();
    if nudge.justify.view > block_tree.highest_qc().view {
        wb.set_highest_qc(&nudge.justify);
    }
    let next_phase = match nudge.justify.phase {
        Phase::Prepare => {
            Phase::Precommit(nudge.justify.view)
        },

        Phase::Precommit(prepare_qc_view) => {
            if prepare_qc_view > block_tree.locked_view() {
                wb.set_locked_view(prepare_qc_view);
            }

            Phase::Commit(nudge.justify.view)
        },

        _ => unreachable!(),
    };
    block_tree.write(wb);

    let vote = me.vote(app.id(), cur_view, nudge.justify.block, next_phase);

    let i_am_validator = block_tree.committed_validator_set().contains(&me.public());
    let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());
    if i_am_validator {
        pm_stub.send(next_leader, vote.clone());
        info::voted(cur_view, &nudge.justify.block, block_tree.block_height(&nudge.justify.block).unwrap(), next_phase);
    }

    let i_am_next_leader = me.public() == next_leader;

    (true, i_am_next_leader)
}

// Returns (whether a new highest qc was collected, i am the next leader).
fn on_receive_vote<K: KVStore>(
    signer: &PublicKeyBytes,
    vote: Vote,
    votes: &mut VoteCollector,
    block_tree: &mut BlockTree<K>,
    cur_view: ViewNumber,
    me: &Keypair,
    pacemaker: &mut impl Pacemaker,
) -> (bool, bool) {
    debug::received_vote(&signer, &vote.block, vote.phase);

    let highest_qc_updated = if vote.is_correct(signer) {
        if let Some(new_qc) = votes.collect(signer, vote) {
            debug::collected_qc(&new_qc.block, new_qc.phase);

            let mut wb = BlockTreeWriteBatch::new();
            if let Phase::Precommit(prepare_qc_view) = new_qc.phase {
                if prepare_qc_view > block_tree.locked_view() {
                    wb.set_locked_view(prepare_qc_view)
                }
            }

            let highest_qc_updated = if new_qc.view > block_tree.highest_qc().view {
                wb.set_highest_qc(&new_qc);
                true
            } else {
                false
            };
            block_tree.write(wb);

            highest_qc_updated
        } else {
            false
        }
    } else {
        false
    };

    let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set()); 
    let i_am_next_leader = me.public() == next_leader;

    (highest_qc_updated, i_am_next_leader)
}

// returns (whether I have collected new view messages from a quorum of validators && I am the next leader)
fn on_receive_new_view<K: KVStore>(
    origin: &PublicKeyBytes,
    new_view: NewView,
    new_view_collector: &mut NewViewCollector,
    block_tree: &mut BlockTree<K>,
    cur_view: ViewNumber,
    me: &Keypair,
    pacemaker: &mut impl Pacemaker,
) -> bool {
    debug::received_new_view(&origin, new_view.view, &new_view.highest_qc.block, new_view.highest_qc.phase);

    if new_view.highest_qc.is_correct(&block_tree.committed_validator_set()) {
        let mut wb = BlockTreeWriteBatch::new();

        if let Phase::Precommit(prepare_qc_view) = new_view.highest_qc.phase {
            if prepare_qc_view > block_tree.locked_view() {
                wb.set_locked_view(prepare_qc_view)
            }
        }

        if new_view.highest_qc.view > block_tree.highest_qc().view {
            wb.set_highest_qc(&new_view.highest_qc);
        };

        block_tree.write(wb);
        
        if new_view_collector.collect(origin) {
            let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set()); 
            let i_am_next_leader = me.public() == next_leader;
            
            return i_am_next_leader
        }
    }

    false
}

fn on_view_timeout<K: KVStore, N: Network>(
    cur_view: ViewNumber,
    app: &impl App<K>,
    block_tree: &BlockTree<K>,
    pacemaker: &mut impl Pacemaker,
    pm_stub: &mut ProgressMessageStub<N>,
) {
    let highest_qc = block_tree.highest_qc();
    info::view_timed_out(cur_view, &highest_qc.block, highest_qc.phase);
    let new_view = ProgressMessage::new_view(app.id(), cur_view, block_tree.highest_qc());

    let next_leader = pacemaker.view_leader(cur_view + 1, &block_tree.committed_validator_set());

    pm_stub.send(next_leader, new_view.clone());
}

fn sync<K: KVStore, N: Network>(
    block_tree: &mut BlockTree<K>, 
    sync_stub: &mut SyncClientStub<N>,
    app: &mut impl App<K>,
    pacemaker: &mut impl Pacemaker,
) {
    // Pick random validator.
    if let Some(peer) = block_tree.committed_validator_set().random() {
        sync_with(peer, block_tree, sync_stub, app, pacemaker);
    }
}

fn sync_with<K: KVStore, N: Network>(
    peer: &PublicKeyBytes,
    block_tree: &mut BlockTree<K>,
    sync_stub: &mut SyncClientStub<N>,
    app: &mut impl App<K>,
    pacemaker: &mut impl Pacemaker,
) {
    info::start_syncing(peer);

    loop {
        let request = SyncRequest {
            start_height: if let Some(height) = block_tree.highest_committed_block_height() { height + 1 } else { 0 },
            limit: pacemaker.sync_request_limit(),
        };
        sync_stub.send_request(*peer, request);

        if let Some(response) = sync_stub.recv_response(*peer, Instant::now() + pacemaker.sync_response_timeout()) {
            if response.blocks.len() == 0 {
                return
            }
            for block in response.blocks {
                if !block.is_correct(&block_tree.committed_validator_set()) {
                    return
                }

                if !block_tree.safe_block(&block) {
                    // We continue here instead of returning because one expected reason why a block in the response may fail the safe
                    // block predicate may be that it has already been included in the block tree. This doesn't discount the possibility
                    // that one of its descendants may be new. 
                    continue;
                }

                let parent_block = if block.justify.is_genesis_qc() {
                    None
                } else {
                    Some(&block.justify.block)
                };
                let validate_block_request = ValidateBlockRequest::new(
                    &block,
                    block_tree.app_view(parent_block)
                );
                if let ValidateBlockResponse::Valid {
                    app_state_updates,
                    validator_set_updates
                } = app.validate_block(validate_block_request) {
                    let validator_set_updates_because_of_commit = block_tree.insert_block(&block, app_state_updates.as_ref(), validator_set_updates.as_ref());

                    if let Some(updates) = validator_set_updates_because_of_commit {
                        sync_stub.update_validator_set(updates);
                    }
                } else {
                    return
                }
            }

            if response.highest_qc.view > block_tree.highest_qc().view {
                block_tree.set_highest_qc(&response.highest_qc)
            }
        }
    }
}
