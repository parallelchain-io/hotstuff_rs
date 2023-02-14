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
//!     * Call the app to produce a new block, and broadcast it in a new proposal.
//!     * If the proposed block changed the committed validator set, recompute whether I am the next leader.  
//!     * Then, if I am not the next leader, move to the next view.
//! 2. Then, poll the network for progress messages until the view timeout:
//!     * [on_receive_proposal](On receiving a proposal):
//!         - Call on the block tree to [crate::state::BlockTree::validate](validate) the proposed block.
//!         - Call on the app to validate it.
//!         - If it passes both checks, insert the block into the block tree and send a vote for it to the next leader.
//!         - If the inserted block changed the commited validator set, recompute whether I am the next leader.
//!         - Then, if I am not the next leader, move to the next view.
//!     * [on_receive_nudge](On receiving a nudge):
//!         - Check if its quorum certificate is higher that the block tree's highest qc.
//!         - If yes, then store it as the highest qc and send a vote for it to the next leader.
//!         - Then, if I am not the next leader, move to the next view.
//!     * [on_receive_vote](On receiving a vote):
//!         - If I am the next leader: 
//!             - Collect the vote.
//!             - If enough matching votes have been collected to form a quorum certificate, store it as the highest qc,
//!               then, move to the next view.
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
use crate::messages::{ProgressMessageFactory, ProgressMessage, Vote, NewView, Proposal, Nudge};
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

            let progressed = execute_view(cur_view, pacemaker, &mut block_tree, &mut pm_stub, &mut app);
            if !progressed && block_tree.highest_qc().view + 2 {
                sync(&mut block_tree, &mut sync_stub);
            }

            cur_view += 1;
        }
    })
}

// Returns whether there was progress in the view.
fn execute_view<'a, K: KVStore<'a>, N: Network>(
    me: Keypair,
    view: ViewNumber,
    pacemaker: impl Pacemaker,
    proposal_rebroadcast_period: Duration,
    block_tree: &mut BlockTree<'a, K>,
    progress_messages: &mut ProgressMessageStub,
    app: &mut impl App<K::Snapshot>,
) -> bool {
    let mut progressed = false;

    let view_deadline = Instant::now() + pacemaker.view_timeout(view, block_tree.highest_qc().view);
    
    let cur_validators: ValidatorSet = block_tree.committed_vs();

    let i_am_validator = cur_validators.contains(me.public.as_bytes());
    let i_am_cur_leader = me.public.to_bytes() == pacemaker.view_leader(view, cur_validators);
    let mut i_am_next_leader = i_am_next_leader(&me, view, &cur_validators, &pacemaker);

    let mut prev_proposal_or_nudge = None;
    let mut votes = VoteCollector;
    while Instant::now() < view_deadline {
        // 1. If I am the current leader, propose a block.
        if i_am_cur_leader {
            let (proposal_or_nudge, vs_changed) = propose_or_nudge(&prev_proposal_or_nudge, &me, &mut block_tree, view, &mut app, &mut progress_messages);
            prev_proposal_or_nudge = Some(proposal_or_nudge);
            if vs_changed {
                i_am_next_leader = i_am_next_leader(&me, view, &block_tree.committed_vs(), &pacemaker);
            }
            if !i_am_next_leader {
                return true
            }
        }

        // 2. If I am a voter or the next leader, receive and progress messages until view timeout.
        let recv_deadline = min(proposal_rebroadcast_period, view_deadline);
        loop { 
            if let Some((origin, msg)) = progress_messages.recv(view, proposal_rebroadcast_period) {
                match msg {
                    ProgressMessage::Proposal(proposal) => {
                        let (voted, i_am_next_leader) = on_receive_proposal(proposal, &mut block_tree, view, &mut app, &progress_messages);
                        if voted {
                            progressed = true;
                            if !i_am_next_leader {
                                return true
                            }
                        }
                    },
                    ProgressMessage::Nudge(nudge) => {
                        let (voted, i_am_next_leader) = on_receive_nudge(nudge, &mut block_tree, view, &mut app, &progress_messages);
                        if voted {
                            progressed = true;
                            if !i_am_next_leader {
                                return true
                            }
                        }
                    },
                    ProgressMessage::Vote(vote) => {
                        let highest_qc_updated = on_receive_vote(vote, &mut votes, &mut block_tree);
                        if highest_qc_updated {
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

// Returns (the broadcasted proposal or nudge, whether the validator set changed).
fn propose_or_nudge<'a, K: KVStore<'a>, N: Network>(
    prev_proposal_or_nudge: Option<&ProgressMessage>,
    me: &Keypair,
    block_tree: &mut BlockTree<'a, K>,
    cur_view: ViewNumber,
    app: &mut impl App<K::Snapshot>,
    pm_stub: &mut ProgressMessageStub,
) -> (ProgressMessage, bool) {
    if let Some(proposal_or_nudge) = prev_proposal_or_nudge {
        pm_stub.broadcast(proposal_or_nudge);
    }

    else {
        let pm_factory = ProgressMessageFactory(me);
        let highest_qc: QuorumCertificate;
        
        match highest_qc.phase {
            // We should propose.
            Phase::Generic | Phase::Commit => {
                let request: ProduceBlockRequest<K::Snapshot>;

                let ProduceBlockResponse { 
                    data, 
                    app_state_updates, 
                    validator_set_updates 
                } = app.produce_block(request);

                let block: Block;
                
                block_tree.insert_block(block.clone(), app_state_updates, validator_set_updates);

                let proposal = if validator_set_updates.is_some() {
                    pm_factory.proposal(app.id(), cur_view, block, Phase::Prepare)
                } else {
                    pm_factory.proposal(app.id(), cur_view, block, Phase::Generic)
                };

                pm_stub.broadcast(&proposal);

                let vs_changed = block.justify.phase == Phase::Commit;

                (proposal, vs_changed)
            },

            // Otherwise, we should broadcast a nudge.
            Phase::Prepare | Phase::Precommit => {
                let nudge = pm_factory.nudge(app.id(), cur_view, highest_qc.block, highest_qc);
                pm_stub.broadcast(&nudge);

                (nudge, false)
            }
        }
    }
}


// Returns (whether I voted, whether I am the next leader).
fn on_receive_proposal<'a, K: KVStore<'a>, N: Network>(
    propose: Proposal,
    block_tree: &mut BlockTree<'a, K>,
    cur_view: ViewNumber,
    app: &mut impl App<K::Snapshot>,
    pm_factory: &ProgressMessageFactory,
    pm_stub: &mut ProgressMessageStub,
) -> (bool, bool) {
    todo!()
}

// Returns (whether I voted, whether I am the next leader).
fn on_receive_nudge<'a, K: KVStore<'a>, N: Network>(
    nudge: Nudge,
    block_tree: &mut BlockTree<'a, K>,
    cur_view: ViewNumber,
    app: &mut impl App<K::Snapshot>,
    pm_factory: &ProgressMessageFactory,
    pm_stub: &mut ProgressMessageStub,
) -> (bool, bool) {
    todo!()
}

// Returns whether a new quorum certificate was collected and highest qc updated.
fn on_receive_vote<'a, K: KVStore<'a>>(
    vote: Vote,
    vote_collector: &mut VoteCollector,
    block_tree: &mut BlockTree<'a, K>,
) -> bool {
    todo!()
}

fn on_receive_new_view<'a, K: KVStore<'a>>(
    new_view: NewView,
    block_tree: &mut BlockTree<'a, K>,
) {
    todo!()
}

fn sync<'a, K: KVStore<'a>>(
    block_tree: &mut BlockTree<'a, K>,
    sync_stub: &mut SyncClientStub,
) {
    todo!()
}

fn i_am_next_leader(
    me: &Keypair,
    view: ViewNumber,
    validators: &ValidatorSet,
    pacemaker: &impl Pacemaker,
) -> bool {
    me.public.to_bytes() == pacemaker.view_leader(view + 1, validators)
}
