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
                        let _ = self.connections.reconnect(&participant);
                        false
                    },
                }
            },
            None => false,
        }
    }

    pub fn broadcast(&self, msg: &ConsensusMsg) {
        let mut public_addrs_of_errored_connections = vec![];
        for (public_addr, stream) in &self.connections.iter() {
            if stream.write(msg).is_err() {
                public_addrs_of_errored_connections.push(*public_addr);
            }
        }

        for public_addr_of_errored_connection in public_addrs_of_errored_connections {
            self.connections.reconnect(&public_addr_of_errored_connection);
        }
    }

    pub fn recv_from(&self, participant: &PublicAddr, timeout: Duration) -> Result<ConsensusMsg, RecvFromError> {
        match self.connections.get(participant) {
            Some(stream) => {
                stream.read(timeout).map_err(|e| match e {
                    StreamReadError::Corrupted => {
                        self.connections.reconnect(participant);
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
                            self.connections.reconnect(&public_addr);
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
