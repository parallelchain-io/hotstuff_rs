//! HotStuff-rs' has pluggable peer-to-peer networking, with each peer identified by a PublicKey. Networking providers
//! interact with HotStuff-rs' threads through implementations of the [Network] trait.

use std::sync::mpsc::{self, Sender, Receiver, RecvTimeoutError, TryRecvError, RecvError};
use std::thread::{self, JoinHandle};
use std::time::{Instant, Duration};
use crate::types::{PublicKeyBytes, ViewNumber, ValidatorSet, AppID};
use crate::messages::*;

pub trait Network: Clone + Send + 'static {
    fn update_validator_set(&mut self, validator_set: ValidatorSet);

    fn broadcast(&mut self, message: Message);

    /// Send a message to the specified peer without blocking.
    fn send(&mut self, peer: PublicKeyBytes, message: Message);

    /// Receive a message from any peer.
    ///
    /// # Important
    /// Messages returned from this function should be deserialized using the Message's implementation of TryFrom<Vec<u8>>,
    /// which checks some simple invariants.
    fn recv(&mut self) -> Option<(PublicKeyBytes, Message)>;
}

/// Spawn the poller thread, which polls the Network for messages and distributes them into the ProgressMessageFilter,
/// or SyncRequestReceiver, or the SyncResponseReceiver according to their variant.
pub(crate) fn start_polling<N: Network>(mut network: N, shutdown_signal: Receiver<()>) -> (
    JoinHandle<()>,
    ProgressMessageFilter,
    Receiver<SyncRequest>,
    Receiver<SyncResponse>,
) {
    let (to_progress_msg_filter, progress_msg_from_poller) = mpsc::channel();
    let (to_sync_request_receiver, sync_request_from_poller) = mpsc::channel();
    let (to_sync_response_receiver, sync_response_from_poller) = mpsc::channel();

    let poller_thread = thread::spawn(move || {
        loop {
            match shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => panic!("Poller thread disconnected from main thread"),
            }

            if let Some((origin, msg)) = network.recv() {
                match msg {
                    Message::ProgressMessage(p_msg) => to_progress_msg_filter.send((origin, p_msg)).unwrap(),
                    Message::SyncMessage(s_msg) => match s_msg {
                        SyncMessage::SyncRequest(s_req) => to_sync_request_receiver.send((origin, s_req)).unwrap(),
                        SyncMessage::SyncResponse(s_res) => to_sync_response_receiver.send((origin, s_res)).unwrap(),
                    }
                }
            }

            thread::yield_now();
        }
    });
    todo!();
    let progress_message_filter = ProgressMessageFilter {
        recycler: to_progress_msg_filter.clone(),
        receiver: progress_msg_from_poller,
    };
    let sync_request_receiver = SyncRequestReceiver(sync_request_from_poller);
    let sync_response_receiver = SyncResponseReceiver(sync_response_from_poller);

    (
        poller_thread,
        progress_message_filter,
        sync_request_receiver,
        sync_response_receiver,
    )
}

pub(crate) struct ProgressMessageStub;

impl ProgressMessageStub {
    pub(crate) fn recv(&self, cur_view: ViewNumber, deadline: Instant) -> Option<(PublicKeyBytes, ProgressMessage)> {
        todo!()
    }

    pub(crate) fn send(&self, peer: PublicKeyBytes, msg: ProgressMessage) {
        todo!()
    }

    pub(crate) fn broadcast(&self, msg: ProgressMessage) {
        todo!()
    }
}

/// A stream of Progress Messages for the current view and specified app. Progress messages received from
/// this stream are guaranteed to have correct signatures.
/// 
/// Progress messages from the next view (current view + 1) are cached, while those from all other views 
/// are dropped.
struct ProgressMessageFilter {
    app_id: AppID,
    recycler: Sender<(PublicKeyBytes, ProgressMessage)>,
    receiver: Receiver<(PublicKeyBytes, ProgressMessage)>,
}

impl ProgressMessageFilter {
    fn recv(&self, cur_view: ViewNumber, timeout: Duration) -> Option<(PublicKeyBytes, ProgressMessage)> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let (origin, msg) = match self.receiver.recv_timeout(deadline - Instant::now()) {
                Ok(o_m) => o_m,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => panic!("ProgressMessageFilter disconnected from poller"),
            };

            if msg.app_id() == self.app_id {
                if msg.view() == cur_view {
                    return Some((origin, msg)) 
                } else if msg.view() == cur_view + 1 {
                    self.recycler.send((origin, msg));
                }   
            }
        }

        None
    }
}

pub(crate) struct SyncClientStub;

impl SyncClientStub {

}

pub(crate) struct SyncServerStub;

impl SyncServerStub {
    pub(crate) fn recv(&self) -> (PublicKeyBytes, SyncRequest) {
        todo!()
    }

    pub(crate) fn send(&self, peer: &PublicKeyBytes, msg: SyncResponse) {
        todo!()
    }
}
