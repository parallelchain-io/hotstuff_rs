/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! This module defines the algorithm thread and the procedures used in it. The algorithm thread is the driving
//! force of a HotStuff-rs replica, it receives and sends [progress messages](crate::messages::ProgressMessage) using 
//! the network to cause the [Block Tree](crate::state) to grow in all replicas in a safe and live manner.
//! 
//! The algorithm thread is a forever loop of increasing view numbers. On initially entering this loop, the replica
//! compares the view of the highest qc it knows with the highest view it has previously entered, selecting the higher
//! of the two, plus 1, as the view to execute.
//! 
//! A view proceeds in one of two modes, depending on whether the depending on whether the local replica is a
//! [validator or a listener](crate::replica). Replicas automatically change between the two modes as the validator
//! set evolves.
//! 
//! The execution of a view in validator mode [proceeds](execute_view) as follows. The execution in listener view is
//! exactly the validator mode flow minus all of the proposing and voting:
//! 1. If I am the current leader (repeat this every proposal re-broadcast duration):
//!     * If the highest qc is a generic or commit qc, call the app to produce a new block, broadcast it in a proposal,
//!       and send a vote to the next leader.
//!     * Otherwise, broadcast a nudge and send a vote to the next leader.
//!     * Then, if I *am not* the next leader, move to the next view.
//! 2. Then, poll the network for progress messages until the view timeout:
//!     * [On receiving a proposal](on_receive_proposal):
//!         - [Call](BlockTree::block_can_be_inserted) the block tree to see if the proposed block can be inserted.
//!         - Call on the app to validate it.
//!         - If it passes both checks, insert the block into the block tree and send a vote for it to the next leader.
//!         - Then, if I *am not* the next leader, move to the next view.
//!     * [On receiving a nudge](on_receive_nudge):
//!         - Check if it is a prepare or precommit qc.
//!         - [Call](BlockTree::qc_can_be_inserted) on the block tree to see if the qc can be set as the highest qc.
//!         - If yes, then store it as the highest qc and send a vote for it to the next leader.
//!         - Then, if I *am not* the next leader, move to the next view.
//!     * [On receiving a vote](on_receive_vote):
//!         - Collect the vote.
//!         - If enough matching votes have been collected to form a quorum certificate, store it as the highest qc.
//!         - Then, if I *am* the next leader, move to the next view.
//!     * [On receving a new view](on_receive_new_view):
//!         - Check if its quorum certificate is higher than the block tree's highest qc.
//!         - If yes, then store it as the highest qc.
//! 
//! A view eventually times out and transitions if a transition is not triggered 'manually' by one of the steps.
//! 
//! ## Sync Protocol

use std::sync::mpsc::{Receiver, TryRecvError};
use std::cmp::{min, max};
use std::time::Instant;
use std::thread::{self, JoinHandle};
use crate::app::{ProduceBlockRequest, ValidateBlockResponse, ValidateBlockRequest};
use crate::app::ProduceBlockResponse;
use crate::messages::{Keypair, ProgressMessage, Vote, NewView, Proposal, Nudge, SyncRequest};
use crate::pacemaker::Pacemaker;
use crate::types::*;
use crate::state::*;
use crate::networking::*;
use crate::app::App;

pub(crate) fn start_algorithm<S: KVGet, K: KVStore<S>, N: Network>(
    app: impl App<S>,
    me: Keypair,
    block_tree: BlockTree<S, K>,
    pacemaker: impl Pacemaker,
    pm_stub: ProgressMessageStub<N>,
    sync_stub: SyncClientStub<N>,
    shutdown_signal: Receiver<()>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        loop {
            match shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => panic!("Algorithm thread disconnected from main thread"),
            }

            let cur_view: ViewNumber = max(block_tree.highest_view_entered(), block_tree.highest_qc().view) + 1;
            block_tree.set_highest_entered_view(cur_view);

            let view_result = execute_view(&me, cur_view, &mut pacemaker, &mut block_tree, &mut pm_stub, &mut app);
            if let Err(ShouldSync) = view_result {
                sync(&mut block_tree, &mut sync_stub, &mut app, &mut pacemaker)
            }

            cur_view += 1;
        }
    })
}

// Returns whether there was progress in the view.
fn execute_view<S: KVGet, K: KVStore<S>, N: Network>(
    me: &Keypair,
    view: ViewNumber,
    pacemaker: &mut impl Pacemaker,
    block_tree: &mut BlockTree<S, K>,
    pm_stub: &mut ProgressMessageStub<N>,
    app: &mut impl App<S>,
) -> Result<(), ShouldSync> {
    let view_deadline = Instant::now() + pacemaker.view_timeout(view, block_tree.highest_qc().view);
    
    let cur_validators: ValidatorSet = block_tree.committed_validator_set();

    let i_am_cur_leader = me.public() == pacemaker.view_leader(view, cur_validators);

    let prev_proposal_or_nudge_and_vote = None;
    let mut votes = VoteCollector::new(view, &cur_validators);
    while Instant::now() < view_deadline {
        // 1. If I am the current leader, propose a block.
        if i_am_cur_leader {
            match prev_proposal_or_nudge_and_vote {
                None => {
                    let (proposal_or_nudge_and_vote, i_am_next_leader) = propose_or_nudge(&me, view, app, &mut block_tree, pacemaker, &mut pm_stub);
                    prev_proposal_or_nudge_and_vote = Some(proposal_or_nudge_and_vote);
                    if !i_am_next_leader {
                        return Ok(())
                    }
                },
                // Rebroadcast proposal or nudge and resend vote.
                Some((proposal_or_nudge, vote)) => {
                    pm_stub.broadcast(&proposal_or_nudge);
                    let next_leader = pacemaker.view_leader(view + 1, block_tree.committed_validator_set());
                    pm_stub.send(&next_leader, &vote);
                }
            }
        }

        // 2. If I am a voter or the next leader, receive and progress messages until view timeout.
        let recv_deadline = min(Instant::now() + pacemaker.proposal_rebroadcast_period(), view_deadline);
        loop { 
            match pm_stub.recv(app.id(), view, block_tree.highest_qc().view, recv_deadline) {
                Ok((origin, msg)) => match msg {
                    ProgressMessage::Proposal(proposal) => {
                        let (voted, i_am_next_leader) = on_receive_proposal(&proposal, &me, view, &mut block_tree, app, pacemaker, pm_stub);
                        if voted {
                            if !i_am_next_leader {
                                return Ok(())
                            }
                        }
                    },
                    ProgressMessage::Nudge(nudge) => {
                        let (voted, i_am_next_leader) = on_receive_nudge(&nudge, &me, view, &mut block_tree, app, pacemaker, pm_stub);
                        if voted {
                            if !i_am_next_leader {
                                return Ok(())
                            }
                        }
                    },
                    ProgressMessage::Vote(vote) => {
                        let (highest_qc_updated, i_am_next_leader) = on_receive_vote(&origin, vote, &mut votes, &mut block_tree, view, &me, pacemaker);
                        if highest_qc_updated && i_am_next_leader {
                            return Ok(())
                        }
                    },
                    ProgressMessage::NewView(new_view) => {
                        let highest_qc_updated = on_receive_new_view(new_view, &mut block_tree);
                    },
                },
                Err(ProgressMessageReceiveError::ReceivedQuorumFromFuture) => return Err(ShouldSync),
                Err(ProgressMessageReceiveError::Timeout) => continue,
            }
        }
    }

    // 3. Send a New View message.
    todo!()

    Ok(())
}

struct ShouldSync;

/// Create a proposal or a nudge, broadcast it to the network, and then send a vote for it to the next leader.
/// 
/// Returns ((the broadcasted proposal or nudge, the sent vote), whether i am the next leader).
fn propose_or_nudge<S: KVGet, K: KVStore<S>, N: Network>(
    me: &Keypair,
    cur_view: ViewNumber,
    app: &mut impl App<S>,
    block_tree: &mut BlockTree<S, K>,
    pacemaker: &mut impl Pacemaker,
    pm_stub: &mut ProgressMessageStub<N>,
) -> ((ProgressMessage, ProgressMessage), bool) {
    let highest_qc = block_tree.highest_qc();
    let (proposal_or_nudge, vote) = match highest_qc.phase {
        // Produce a proposal.
        Phase::Generic | Phase::Commit => {
            let parent_block = if highest_qc.is_genesis_qc() {
                None
            } else {
                Some(highest_qc.block)
            };

            let produce_block_request = ProduceBlockRequest::new(
                cur_view, 
                parent_block, 
                block_tree.snapshot()
            );

            let ProduceBlockResponse { 
                data, 
                data_hash,
                app_state_updates, 
                validator_set_updates 
            } = app.produce_block(produce_block_request);

            let height = if let Some(parent_block) = parent_block {
                block_tree.block_height(&highest_qc.block).unwrap() + 1
            } else {
                0
            };
            let block = Block::new(height, highest_qc, data_hash, data);
            
            block_tree.insert_block(&block, app_state_updates.as_ref(), validator_set_updates.as_ref());

            let proposal = me.proposal(app.id(), cur_view, &block);
            let vote_phase = if validator_set_updates.is_some() { Phase::Prepare } else { Phase::Generic };
            let vote = me.vote(app.id(), cur_view, block.hash, vote_phase);

            (proposal, vote)
        },

        // Produce a nudge.
        Phase::Prepare | Phase::Precommit => {
            let nudge = me.nudge(app.id(), cur_view, &highest_qc);
            let vote_phase = if highest_qc.phase == Phase::Prepare { Phase::Precommit } else { Phase::Commit };
            let vote  = me.vote(app.id(), cur_view, highest_qc.block, vote_phase);

            (nudge, vote)
        }
    };

    pm_stub.broadcast(&proposal_or_nudge);

    let next_leader = pacemaker.view_leader(cur_view + 1, block_tree.committed_validator_set());

    pm_stub.send(&next_leader, &vote);

    let i_am_next_leader = me.public() == next_leader;

    ((proposal_or_nudge, vote), i_am_next_leader)
}

// Returns (whether I voted, whether I am the next leader).
fn on_receive_proposal<S: KVGet, K: KVStore<S>, N: Network>(
    proposal: &Proposal,
    me: &Keypair,
    cur_view: ViewNumber,
    block_tree: &mut BlockTree<S, K>,
    app: &mut impl App<S>,
    pacemaker: &impl Pacemaker,
    pm_stub: &mut ProgressMessageStub<N>,
) -> (bool, bool) {
    if !proposal.block.is_correct(&block_tree.committed_validator_set()) 
        || !block_tree.block_can_be_inserted(&proposal.block) {
        let next_leader = pacemaker.view_leader(cur_view + 1, block_tree.committed_validator_set());
        let i_am_next_leader = me.public() == next_leader;
        return (false, i_am_next_leader)
    }

    let validate_block_request = ValidateBlockRequest::new(
        proposal.block,
        block_tree.snapshot(),
    );
    if let ValidateBlockResponse::Valid {
        app_state_updates,
        validator_set_updates,
    } = app.validate_block(validate_block_request) {
        block_tree.insert_block(&proposal.block, app_state_updates.as_ref(), validator_set_updates.as_ref()); 

        let i_am_validator = block_tree.committed_validator_set().contains(&me.public());
        let next_leader = pacemaker.view_leader(cur_view + 1, block_tree.committed_validator_set());
        if i_am_validator {
            let vote_phase = if validator_set_updates.is_some() { Phase::Prepare } else { Phase::Precommit };
            let vote = me.vote(app.id(), cur_view, proposal.block.hash, vote_phase);

            pm_stub.send(&next_leader, &vote);
        }
        let i_am_next_leader = me.public() == next_leader;

        (true, i_am_next_leader)
    } else {
        let next_leader = pacemaker.view_leader(cur_view + 1, block_tree.committed_validator_set());
        let i_am_next_leader = me.public() == next_leader;

        (false, i_am_next_leader)
    }
}

// Returns (whether I voted, whether I am the next leader).
fn on_receive_nudge<S: KVGet, K: KVStore<S>, N: Network>(
    nudge: &Nudge,
    me: &Keypair,
    cur_view: ViewNumber,
    block_tree: &mut BlockTree<S, K>,
    app: &mut impl App<S>,
    pacemaker: &mut impl Pacemaker,
    pm_stub: &mut ProgressMessageStub<N>,
) -> (bool, bool) {
    if !nudge.justify.is_correct(&block_tree.committed_validator_set())
        || !block_tree.qc_can_be_inserted(&nudge.justify) {
            let next_leader = pacemaker.view_leader(cur_view + 1, block_tree.committed_validator_set());
            let i_am_next_leader = me.public() == next_leader;
            return (false, i_am_next_leader)
        }

    block_tree.set_highest_qc(&nudge.justify);

    let vote_phase = match nudge.justify.phase {
        Phase::Prepare => Phase::Precommit,
        Phase::Precommit => Phase::Commit,
        _ => unreachable!(),
    };
    let vote = me.vote(app.id(), cur_view, nudge.justify.block, vote_phase);

    let next_leader = pacemaker.view_leader(cur_view + 1, block_tree.committed_validator_set());
    let i_am_next_leader = me.public() == next_leader;
    
    let i_am_validator = block_tree.committed_validator_set().contains(&me.public());
    if i_am_validator {
        pm_stub.send(&next_leader, &vote);
    }

    (true, i_am_next_leader)
}

// Returns (whether a new highest qc was collected, i am the next leader).
fn on_receive_vote<S: KVGet, K: KVStore<S>>(
    origin: &PublicKeyBytes,
    vote: Vote,
    votes: &mut VoteCollector,
    block_tree: &mut BlockTree<S, K>,
    cur_view: ViewNumber,
    me: &Keypair,
    pacemaker: &mut impl Pacemaker,
) -> (bool, bool) {
    let highest_qc_updated = if let Some(new_qc) = votes.collect(origin, vote) {
        if block_tree.qc_can_be_inserted(&new_qc) {
            block_tree.set_highest_qc(&new_qc);
            true
        } else {
            false
        }
    } else {
        false
    };

    let next_leader = pacemaker.view_leader(cur_view + 1, block_tree.committed_validator_set()); 
    let i_am_next_leader = me.public() == next_leader;

    (highest_qc_updated, i_am_next_leader)
}

// Returns whether the highest qc was updated.
fn on_receive_new_view<S: KVGet, K: KVStore<S>>(
    new_view: NewView,
    block_tree: &mut BlockTree<S, K>,
) -> bool {
    if new_view.highest_qc.is_correct(&block_tree.committed_validator_set()) {
        if block_tree.qc_can_be_inserted(&new_view.highest_qc) {
            block_tree.set_highest_qc(&new_view.highest_qc);
            return true
        }
    }
    false
}

fn sync<S: KVGet, K: KVStore<S>, N: Network>(
    block_tree: &mut BlockTree<S, K>, 
    sync_stub: &mut SyncClientStub<N>,
    app: &mut impl App<S>,
    pacemaker: &mut impl Pacemaker,
) {
    // Pick random validator.
    if let Some(peer) = block_tree.committed_validator_set().random() {
        sync_with(&peer, block_tree, sync_stub, app, pacemaker);
    }
}

fn sync_with<S: KVGet, K: KVStore<S>, N: Network>(
    peer: &PublicKeyBytes, 
    block_tree: &mut BlockTree<S, K>,
    sync_stub: &mut SyncClientStub<N>,
    app: &mut impl App<S>,
    pacemaker: &mut impl Pacemaker,
) {
    loop {
        let request = SyncRequest {
            highest_committed_block: block_tree.highest_committed_block(),
            limit: pacemaker.sync_request_limit(),
        };
        sync_stub.send_request(&peer, &request);

        let response = sync_stub.recv_response(&peer);
        for block in response.blocks {
            if !block.is_correct(&block_tree.committed_validator_set())
                || !block_tree.block_can_be_inserted(&block) {
                return
            }

            let validate_block_request: ValidateBlockRequest<S>;
            if let ValidateBlockResponse::Valid {
                app_state_updates,
                validator_set_updates
            } = app.validate_block(validate_block_request) {
                block_tree.insert_block(&block, app_state_updates.as_ref(), validator_set_updates.as_ref())
            } else {
                return
            }
        }

        if block_tree.qc_can_be_inserted(&response.highest_qc) {
            block_tree.set_highest_qc(&response.highest_qc)
        }

        if response.blocks.len() == 0 {
            return
        }
    }
}
