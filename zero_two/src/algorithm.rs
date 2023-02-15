/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! This module defines the algorithm thread and the procedures used in it. The algorithm thread is the driving
//! force of a HotStuff-rs replica, it receives and sends [crate::messages::ProgressMessage](progress messages) using 
//! the network to cause the [crate::state](Block Tree) to grow in all replicas in a safe and live manner.
//! 
//! The algorithm thread is a forever loop of increasing view numbers. On initially entering this loop, the replica
//! compares the view of the [highest qc it knows](crate::state::BlockTree::highest_qc) with the 
//! [highest view it has previously entered](crate::state::BlockTree::highest_view_entered), selecting the higher
//! of the two, plus 1, as the view to execute.
//! 
//! A view proceeds in one of two modes, depending on whether the depending on whether the local replica is a
//! [crate::replica](validator or a listener). Replicas automatically change between the two modes as the validator
//! set evolves.
//! 
//! The execution of a view in validator mode [execute_view](proceeds) as follows. The execution in listener view is
//! exactly the validator mode flow minus all of the proposing and voting:
//! 1. If I am the current leader (repeat this every proposal re-broadcast duration):
//!     * If the highest qc is a generic or commit qc, call the app to produce a new block, broadcast it in a proposal,
//!       and send a vote to the next leader.
//!     * Otherwise, broadcast a nudge and send a vote to the next leader.
//!     * Then, if I *am not* the next leader, move to the next view.
//! 2. Then, poll the network for progress messages until the view timeout:
//!     * [on_receive_proposal](On receiving a proposal):
//!         - Call on the block tree to [crate::state::BlockTree::validate](validate) the proposed block.
//!         - Call on the app to validate it.
//!         - If it passes both checks, insert the block into the block tree and send a vote for it to the next leader.
//!         - Then, if I *am not* the next leader, move to the next view.
//!     * [on_receive_nudge](On receiving a nudge):
//!         - Check if its quorum certificate is higher that the block tree's highest qc.
//!         - If yes, then store it as the highest qc and send a vote for it to the next leader.
//!         - Then, if I *am not* the next leader, move to the next view.
//!     * [on_receive_vote](On receiving a vote):
//!         - Collect the vote.
//!         - If enough matching votes have been collected to form a quorum certificate, store it as the highest qc.
//!         - Then, if I *am* the next leader, move to the next view.
//!     * [on_receive_new_view](On receving a new view):
//!         - Check if its quorum certificate is higher than the block tree's highest qc.
//!         - If yes, then store it as the highest qc.
//! 
//! A view eventually times out and transitions if a transition is not triggered 'manually' by one of the steps. If a
//! view timed out without making progress (proposing a block, sending out a vote, or collecting or receving a new 
//! highest qc), then the algorithm thread tries to [sync](sync) with other replicas before moving on to the next view. 

use std::sync::mpsc::{Receiver, TryRecvError};
use std::cmp::{min, max};
use std::time::{Instant, Duration};
use std::thread::{self, JoinHandle};
use crate::app::{ProduceBlockRequest, ValidateBlockResponse, ValidateBlockRequest};
use crate::app::ProduceBlockResponse;
use crate::messages::{Keypair, ProgressMessage, Vote, NewView, Proposal, Nudge};
use crate::pacemaker::Pacemaker;
use crate::types::*;
use crate::state::*;
use crate::networking::*;
use crate::app::App;

pub(crate) fn start_algorithm<'a, K: KVStore<'a>, N: Network>(
    app: impl App<K::Snapshot>,
    me: Keypair,
    block_tree: BlockTree<'a, K>,
    pacemaker: impl Pacemaker,
    proposal_rebroadcast_period: Duration,
    pm_stub: ProgressMessageStub,
    sync_stub: SyncClientStub,
    shutdown_signal: Receiver<()>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        loop {
            match shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => panic!("Algorithm thread disconnected from main thread"),
            }

            let cur_view: ViewNumber = max(block_tree.highest_entered_view(), block_tree.highest_qc().view) + 1;
            block_tree.set_highest_entered_view(cur_view);

            let progressed = execute_view(&me, cur_view, pacemaker, proposal_rebroadcast_period, &mut block_tree, &mut pm_stub, &mut app);
            if !progressed && block_tree.highest_qc().view + 2 {
                sync(&mut block_tree, &mut sync_stub);
            }

            cur_view += 1;
        }
    })
}

// Returns whether there was progress in the view.
fn execute_view<'a, K: KVStore<'a>, N: Network>(
    me: &Keypair,
    view: ViewNumber,
    pacemaker: impl Pacemaker,
    proposal_rebroadcast_period: Duration,
    block_tree: &mut BlockTree<'a, K>,
    pm_stub: &mut ProgressMessageStub,
    app: &mut impl App<K::Snapshot>,
) -> bool {
    let mut progressed = false;

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
                    let (proposal_or_nudge_and_vote, i_am_next_leader) = propose_or_nudge(&me, view, app, &mut block_tree, &pacemaker, &mut pm_stub);
                    prev_proposal_or_nudge_and_vote = Some(proposal_or_nudge_and_vote);
                    if !i_am_next_leader {
                        return true
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
        let recv_deadline = min(Instant::now() + proposal_rebroadcast_period, view_deadline);
        loop { 
            if let Some((origin, msg)) = pm_stub.recv(app.id(), view, recv_deadline) {
                match msg {
                    ProgressMessage::Proposal(proposal) => {
                        let (voted, i_am_next_leader) = on_receive_proposal(&proposal, &me, view, &mut block_tree, app, &pacemaker, pm_stub);
                        if voted {
                            progressed = true;
                            if !i_am_next_leader {
                                return true
                            }
                        }
                    },
                    ProgressMessage::Nudge(nudge) => {
                        let (voted, i_am_next_leader) = on_receive_nudge(&nudge, &me, view, &mut block_tree, app, &pacemaker, pm_stub);
                        if voted {
                            progressed = true;
                            if !i_am_next_leader {
                                return true
                            }
                        }
                    },
                    ProgressMessage::Vote(vote) => {
                        let (highest_qc_updated, i_am_next_leader) = on_receive_vote(&origin, vote, &mut votes, &mut block_tree, view, &me, &pacemaker);
                        progressed = progressed || highest_qc_updated;
                        if highest_qc_updated && i_am_next_leader {
                            return true
                        }
                    },
                    ProgressMessage::NewView(new_view) => {
                        let highest_qc_updated = on_receive_new_view(new_view, &mut block_tree);
                        progressed = progressed || highest_qc_updated;
                    },
                }
            }
        }
    }

    return progressed
}

/// Create a proposal or a nudge, broadcast it to the network, and then send a vote for it to the next leader.
/// 
/// Returns ((the broadcasted proposal or nudge, the sent vote), whether i am the next leader).
fn propose_or_nudge<'a, K: KVStore<'a>, N: Network>(
    me: &Keypair,
    cur_view: ViewNumber,
    app: &mut impl App<K::Snapshot>,
    block_tree: &mut BlockTree<'a, K>,
    pacemaker: &impl Pacemaker,
    pm_stub: &mut ProgressMessageStub,
) -> ((ProgressMessage, ProgressMessage), bool) {
    let highest_qc = block_tree.highest_qc();
    let (proposal_or_nudge, vote) = match highest_qc().phase {
        // Produce a proposal.
        Phase::Generic | Phase::Commit => {
            let produce_block_request: ProduceBlockRequest<K::Snapshot>;

            let ProduceBlockResponse { 
                data, 
                app_state_updates, 
                validator_set_updates 
            } = app.produce_block(produce_block_request);

            let block: Block;
            
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
fn on_receive_proposal<'a, K: KVStore<'a>, N: Network>(
    proposal: &Proposal,
    me: &Keypair,
    cur_view: ViewNumber,
    block_tree: &mut BlockTree<'a, K>,
    app: &mut impl App<K::Snapshot>,
    pacemaker: &impl Pacemaker,
    pm_stub: &mut ProgressMessageStub,
) -> (bool, bool) {
    if !proposal.block.is_correct(&block_tree.committed_validator_set()) 
        || !block_tree.block_can_be_inserted(&proposal.block) {
        let next_leader = pacemaker.view_leader(cur_view + 1, block_tree.committed_validator_set());
        let i_am_next_leader = me.public() == next_leader;
        return (false, i_am_next_leader)
    }

    let validate_block_request: ValidateBlockRequest<K::Snapshot>;
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
fn on_receive_nudge<'a, K: KVStore<'a>, N: Network>(
    nudge: &Nudge,
    me: &Keypair,
    cur_view: ViewNumber,
    block_tree: &mut BlockTree<'a, K>,
    app: &mut impl App<K::Snapshot>,
    pacemaker: &impl Pacemaker,
    pm_stub: &mut ProgressMessageStub,
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
fn on_receive_vote<'a, K: KVStore<'a>>(
    origin: &PublicKeyBytes,
    vote: Vote,
    votes: &mut VoteCollector,
    block_tree: &mut BlockTree<'a, K>,
    cur_view: ViewNumber,
    me: &Keypair,
    pacemaker: &impl Pacemaker,
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
fn on_receive_new_view<'a, K: KVStore<'a>>(
    new_view: NewView,
    validator_set: &ValidatorSet,
    block_tree: &mut BlockTree<'a, K>,
) -> bool {
    if new_view.highest_qc.is_correct(&block_tree.committed_validator_set()) {
        if block_tree.qc_can_be_inserted(&new_view.highest_qc) {
            block_tree.set_highest_qc(&new_view.highest_qc);
            return true
        }
    }
    false
}

fn sync<'a, K: KVStore<'a>>(
    block_tree: &mut BlockTree<'a, K>,
    sync_stub: &mut SyncClientStub,
) {
    todo!()
}
