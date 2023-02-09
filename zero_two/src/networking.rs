//! HotStuff-rs' has pluggable peer-to-peer networking, with each peer identified by a PublicKey. Networking providers
//! interact with HotStuff-rs' threads through implementations of the [Network] trait.

use std::sync::mpsc::{self, Sender, Receiver, RecvTimeoutError, RecvError};
use std::thread;
use std::time::{Instant, Duration};
use crate::types::{PublicKey, ViewNumber};
use crate::messages::*;

pub trait Network: Clone + Send + Sync + 'static {
    /// Causes the network stub to try and connect with the identified peer without blocking. The network stub will continue
    /// to try and connect to the peer until it succeeds or if the peer is [Network::disconnect]ed.
    fn connect(&self, peer: PublicKey);

    /// Causes the network stub to disconnect with the identified peer, or, if not connected, to stop trying to connect to it.
    fn disconnect(&self, peer: PublicKey);

    /// Send a message to the specified peer without blocking.
    fn send(&self, peer: PublicKey, message: Message);

    /// Receive a message from any peer.
    ///
    /// # Important
    /// Messages returned from this function should be deserialized using the Message's implementation of TryFrom<Vec<u8>>.
    fn recv(&self) -> Option<(PublicKey, Message)>;
}

/// Spawn the poller thread, which polls the Network for messages and distributes them into the ProgressMessageFilter,
/// or SyncRequestReceiver, or the SyncResponseReceiver according to their variant.
pub(crate) fn start_polling<N: Network>(network: N, shutdown_signal: Receiver<()>) -> (
    thread::JoinHandle<()>,
    ProgressMessageFilter,
    SyncRequestReceiver,
    SyncResponseReceiver,
) {
    let (to_progress_msg_filter, progress_msg_from_poller) = mpsc::channel();
    let (to_sync_request_receiver, sync_request_from_poller) = mpsc::channel();
    let (to_sync_response_receiver, sync_response_from_poller) = mpsc::channel();

    let poller_thread = thread::spawn(move || {
        loop {
            if let Ok(()) = shutdown_signal.try_recv() {
                return
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
    let progress_message_filter = ProgressMessageFilter {
        recycler: to_progress_msg_filter.clone(),
        receiver: progress_msg_from_poller,
        cur_view: 0,
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

/// A stream of Progress Messages for the current view. Progress messages received from this stream are
/// guaranteed to have correct signatures.
/// 
/// Progress messages from the next view (current view + 1) are cached, while those from all other views 
/// are dropped.
pub(crate) struct ProgressMessageFilter {
    recycler: Sender<(PublicKey, ProgressMessage)>,
    receiver: Receiver<(PublicKey, ProgressMessage)>,
    cur_view: ViewNumber
}

impl ProgressMessageFilter {
    pub(crate) fn set_cur_view(&mut self, cur_view: ViewNumber) {
        self.cur_view = cur_view;
    }

    pub(crate) fn recv(&self, timeout: Duration) -> Option<(PublicKey, ProgressMessage)> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let (origin, msg) = match self.receiver.recv_timeout(deadline - Instant::now()) {
                Ok(o_m) => o_m,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => panic!("ProgressMessageFilter disconnected from poller"),
            };

            if msg.view() == self.cur_view {
                // Verify signature.
                return Some((origin, msg)) 
            } else if msg.view() == self.cur_view + 1 {
                self.recycler.send((origin, msg));
            }
        }

        None
    }
}

pub(crate) struct SyncRequestReceiver(Receiver<(PublicKey, SyncRequest)>);

impl SyncRequestReceiver {
    pub(crate) fn recv(&self) -> (PublicKey, SyncRequest) {
        match self.0.recv() {
            Ok(o_m) => o_m,
            Err(RecvError) => panic!("SyncRequestReceiver disconnected from poller"),
        }
    }
}

pub(crate) struct SyncResponseReceiver(Receiver<(PublicKey, SyncResponse)>);

impl SyncResponseReceiver {
    pub(crate) fn recv(&self, timeout: Duration) -> Option<(PublicKey, SyncResponse)> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.0.recv_timeout(deadline - Instant::now()) {
                Ok(o_m) => return Some(o_m),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => panic!("SyncResponseReceiver disconnected from poller"),
            }
        }

        None
    }
}
