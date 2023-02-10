use std::sync::mpsc::TryRecvError;
use crate::persistence::*;
use crate::networking::*;

/// Helps leaders incrementally form QuorumCertificates by combining votes for the same view, block, and phase.
pub struct VoteCollection {
    view: ViewNumber,
    block: CryptoHash,
    phase: Phase,
    signature_set: SignatureSet,
}

impl VoteCollection {

}

pub(crate) fn start_algorithm<K: KVGet + KVWrite + KVCamera>(
    block_tree: BlockTree<K>,
    progress_message_sender: ProgressMessageSender,
    progress_message_filter: ProgressMessageFilter,
    sync_request_sender: SyncRequestSender,
    sync_request_receiver: SyncRequestReceiver,
    shutdown_signal: Receiver<()>,
) {
    loop {
        match shutdown_signal.try_recv() {
            Ok(()) => return
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => panic!("Algorithm thread disconnected from main thread"),
        }

        




    }
}