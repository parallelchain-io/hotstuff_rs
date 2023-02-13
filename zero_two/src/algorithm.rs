use std::sync::mpsc::{Receiver, TryRecvError};
use crate::app::ProposeBlockRequest;
use crate::app::ProposeBlockResponse;
use crate::messages::ProgressMessage;
use crate::pacemaker::Pacemaker;
use crate::types::*;
use crate::state::*;
use crate::networking::*;
use crate::app::App;

/// Helps leaders incrementally form QuorumCertificates by combining votes for the same view, block, and phase.
pub struct VoteCollection {
    view: ViewNumber,
    block: CryptoHash,
    phase: Phase,
    signature_set: SignatureSet,
}

impl VoteCollection {

}

pub(crate) fn start_algorithm<'a, K: KVStore<'a>, N: Network>(
    app: impl App<K>,
    me: PublicKey,
    block_tree: BlockTree<'a, K>,
    pacemaker: impl Pacemaker,
    progress_message_sender: ProgressMessageSender<N>,
    progress_message_filter: ProgressMessageFilter,
    sync_request_sender: SyncRequestSender<N>,
    sync_request_receiver: SyncRequestReceiver,
    shutdown_signal: Receiver<()>,
) {
    loop {
        match shutdown_signal.try_recv() {
            Ok(()) => return,
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => panic!("Algorithm thread disconnected from main thread"),
        }

        let cur_view: ViewNumber = block_tree.cur_view();
        let cur_validators: ValidatorSet = block_tree.committed_vs();

        // This replica is a Validator.
        if cur_validators.contains(me.as_bytes()) {

            // If I am the leader, broadcast a proposal.
            if me.to_bytes() == pacemaker.view_leader(cur_view, cur_validators) {
            }


        } 
        
        // This replica is a Listener.
        else {


        }
    }
}

fn propose_block<'a, K: KVStore<'a>, N: Network>(
    block_tree: &mut BlockTree<'a, K>,
    cur_view: ViewNumber,
    app: &mut impl App<K::Snapshot>,
    pm_factory: &ProgressMessageFactory,
    pm_sender: &mut ProgressMessageSender<N>
) {
    let highest_qc: Option<QuorumCertificate>;
    if let Some(highest_qc) = highest_qc {
        if highest_qc.phase == Phase::Generic || highest_qc.phase == Phase::Commit {
            let request: ProposeBlockRequest<K::Snapshot>;

            let ProposeBlockResponse { 
                data, 
                app_state_updates, 
                validator_set_updates } = app.propose_block(request);

            let block: Block;
            
            block_tree.insert_block(block.clone(), app_state_updates, validator_set_updates);

            let proposal = if validator_set_updates.is_some() {
                pm_factory.new_proposal(app.id(), cur_view, block, Phase::Prepare)
            } else {
                pm_factory.new_proposal(app.id(), cur_view, block, Phase::Generic)
            };

            pm_sender.broadcast(proposal)
        } else {
            
        }
    } else {

    }
}

struct ProgressMessageFactory(Keypair);

impl ProgressMessageFactory {
    // phase must be either Generic or Prepare.
    fn new_proposal(&self, app_id: AppID, view: ViewNumber, block: Block, phase: Phase) -> ProgressMessage {
        todo!()
    }

    // justify.phase must be either Prepare or Precommit.
    fn nudge_proposal(&self, app_id: AppID, view: ViewNumber, block: CryptoHash, justify: QuorumCertificate) -> ProgressMessage {
        todo!()
    }

    fn vote(&self, app_id: AppID, view: ViewNumber, block: CryptoHash, phase: Phase) -> ProgressMessage {
        todo!()
    }

    fn new_view(&self, view: ViewNumber, highest_qc: QuorumCertificate) -> ProgressMessage {
        todo!()
    }
}
