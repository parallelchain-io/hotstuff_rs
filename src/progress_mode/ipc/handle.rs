use std::time::{Instant, Duration};
use crate::msg_types::{ConsensusMsg, PublicAddr};
use crate::progress_mode::ipc::ConnectionSet;

use super::stream::StreamReadError;

pub struct Handle {
    connections: ConnectionSet
}

// `send_to` and `broadcast` are non-blocking. 
// `recv` and `recv_from_any` are blocking with a timeout (typically set to some function of TNT).
// All functions transparently handle errored streams by calling `ManagedConnSet::reconnect` on them. 
impl Handle {
    // Returns false if there is no Stream corresponding to public_address in ConnSet, or if the Stream is corrupted, in which case
    // Handle will transparently arrange for Stream to be dropped and re-established.
    pub fn send_to(&self, participant: PublicAddr, msg: &ConsensusMsg) -> bool {
        match self.connections.get(&participant) {
            Some(stream) => {
                match stream.write(&msg) {
                    Ok(_) => true,
                    Err(_) => {
                        let _ = self.connections.reconnect((participant, stream.peer_addr().ip()));
                        false
                    },
                }
            },
            None => false,
        }
    }

    pub fn broadcast(&self, msg: &ConsensusMsg) {
        let mut errored_conns = vec![];
        for (public_addr, stream) in &self.connections.iter() {
            if stream.write(msg).is_err() {
                errored_conns.push((*public_addr, stream.peer_addr().ip()));
            }
        }

        for errored_conn in errored_conns {
            self.connections.reconnect(errored_conn);
        }
    }

    pub fn recv_from(&self, public_addr: &PublicAddr, timeout: Duration) -> Result<ConsensusMsg, RecvFromError> {
        match self.connections.get(public_addr) {
            Some(stream) => {
                stream.read(timeout).map_err(|e| match e {
                    StreamReadError::Corrupted => {
                        self.connections.reconnect((*public_addr, stream.peer_addr().ip()));
                       RecvFromError::NotConnected
                    },
                    StreamReadError::Timeout => RecvFromError::Timeout,
                })
            },
            None => Err(RecvFromError::NotConnected)
        }
    }

    pub fn recv_from_any(&self, timeout: Duration) -> Result<ConsensusMsg, RecvFromError> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            match self.connections.get_random() {
                Some((public_addr, stream)) => match stream.read(Duration::ZERO) {
                    Ok(msg) => return Ok(msg),
                    Err(e) => match e {
                        StreamReadError::Corrupted => {
                            self.connections.reconnect((public_addr, stream.peer_addr().ip()));
                            continue
                        },
                        StreamReadError::Timeout => continue
                    }
                }
                None => continue,
            }
        }

        Err(RecvFromError::Timeout)
    }
}

pub enum RecvFromError {
    Timeout,
    NotConnected,
}
