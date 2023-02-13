use std::sync::mpsc::{Receiver, TryRecvError};
use std::cmp::max;
use std::time::Instant;
use std::thread::{self, JoinHandle};
use crate::app::{ProposeBlockRequest, ValidateBlockResponse, ValidateBlockRequest};
use crate::app::ProposeBlockResponse;
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
        let pm_factory = ProgressMessageFactory(me);

        'next_view: loop {
            match shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => panic!("Algorithm thread disconnected from main thread"),
            }

            let cur_view: ViewNumber = max(block_tree.highest_voted_view(), block_tree.highest_qc().view) + 1;
            let view_deadline = Instant::now() + pacemaker.view_timeout(cur_view, block_tree.highest_qc().view);
            let cur_validators: ValidatorSet = block_tree.committed_vs();

            // I am a validator.
            if cur_validators.contains(me.public.as_bytes()) {
                let i_am_cur_leader = me.public.to_bytes() == pacemaker.view_leader(cur_view, cur_validators);
                let i_am_next_leader = me.public.to_bytes() == pacemaker.view_leader(cur_view + 1, cur_validators);

                // If I am the current leader, propose a block.
                if i_am_cur_leader {
                    propose(&mut block_tree, cur_view, &mut app, &pm_factory, &mut pm_stub);
                    if !i_am_next_leader {
                        continue 'next_view
                    }
                }

                // If I am a voter or the next leader, receive and progress messages until view timeout.
                let mut votes = VoteCollector;
                loop { 
                    if let Some((origin, msg)) = pm_stub.recv(cur_view, view_deadline) {
                        match msg {
                            ProgressMessage::Proposal(proposal) => {
                                let voted = on_receive_proposal(proposal, &mut block_tree, cur_view, &mut app, &pm_factory, &mut pm_stub);
                                if voted && !i_am_next_leader {
                                    continue 'next_view
                                }
                            },
                            ProgressMessage::Nudge(nudge) => {
                                let voted = on_receive_nudge(nudge, &mut block_tree, cur_view, &mut app, &pm_factory, &mut pm_stub);
                                if voted && !i_am_cur_leader {
                                    continue 'next_view
                                }
                            },
                            ProgressMessage::Vote(vote) => {
                                let highest_qc_updated = on_receive_vote(vote, &mut votes, &mut block_tree);
                                if highest_qc_updated {
                                    continue 'next_view
                                }
                            },
                            ProgressMessage::NewView(new_view) => on_receive_new_view(new_view, &mut block_tree),
                        }
                    }

                    if Instant::now() > view_deadline {
                        // If there wasn't progress in this view, and it has been more than 2 views since we last
                        // progressed, then sync before moving to the next view.
                        if cur_view > block_tree.highest_qc().view + 2 {
                            sync(&mut block_tree, &sync_stub);
                        }

                        continue 'next_view
                    }
                } 
            }
            
            // I am a listener.
            else {


            }
        }
    })
}

fn propose<'a, K: KVStore<'a>, N: Network>(
    block_tree: &mut BlockTree<'a, K>,
    cur_view: ViewNumber,
    app: &mut impl App<K::Snapshot>,
    pm_factory: &ProgressMessageFactory,
    pm_stub: &mut ProgressMessageStub,
) {
    let highest_qc: Option<QuorumCertificate>;
    if let Some(highest_qc) = highest_qc {
        if highest_qc.phase == Phase::Generic || highest_qc.phase == Phase::Commit {
            let request: ProposeBlockRequest<K::Snapshot>;

            let ProposeBlockResponse { 
                data, 
                app_state_updates, 
                validator_set_updates 
            } = app.propose_block(request);

            let block: Block;
            
            block_tree.insert_block(block.clone(), app_state_updates, validator_set_updates);

            let proposal = if validator_set_updates.is_some() {
                pm_factory.new_proposal(app.id(), cur_view, block, Phase::Prepare)
            } else {
                pm_factory.new_proposal(app.id(), cur_view, block, Phase::Generic)
            };

            pm_stub.broadcast(proposal)
        } else {
            
        }
    } else {

    }
}

// Returns whether the proposal was voted for.
fn on_receive_proposal<'a, K: KVStore<'a>, N: Network>(
    propose: Proposal,
    block_tree: &mut BlockTree<'a, K>,
    cur_view: ViewNumber,
    app: &mut impl App<K::Snapshot>,
    pm_factory: &ProgressMessageFactory,
    pm_stub: &mut ProgressMessageStub,
) -> bool {
    todo!()
}

// Returns whether the nudge was voted for.
fn on_receive_nudge<'a, K: KVStore<'a>, N: Network>(
    nudge: Nudge,
    block_tree: &mut BlockTree<'a, K>,
    cur_view: ViewNumber,
    app: &mut impl App<K::Snapshot>,
    pm_factory: &ProgressMessageFactory,
    pm_stub: &mut ProgressMessageStub,
) -> bool {
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
    sync_stub: &SyncClientStub,
) {
    todo!()
}
