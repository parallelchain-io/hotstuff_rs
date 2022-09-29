use std::time::{Instant, Duration};
use crate::config::NetworkingConfiguration;
use crate::msg_types::ConsensusMsg;
use crate::identity::{PublicKeyBytes, ParticipantSet, PublicKey};
use crate::ipc::{ConnectionSet, StreamReadError};

use super::stream::StreamCorruptedError;

/// Handle exposes methods for sending ConsensusMsgs to, and receiving ConsensusMsgs from other Participants. All of Handle's methods
/// transparently handle errored streams by calling `ConnectionSet::reconnect` on them. 
pub struct Handle {
    connections: ConnectionSet,
    ipc_config: NetworkingConfiguration,
}

impl Handle {
    pub fn new(static_participant_set: ParticipantSet, my_public_key: PublicKey, ipc_config: NetworkingConfiguration) -> Handle {
        Handle {
            connections: ConnectionSet::new(static_participant_set, my_public_key.to_bytes(), ipc_config.clone()),
            ipc_config,
        }
    }

    /// Asynchronously send a ConsensusMsg to a specific Participant, identified by their PublicAddr. Returns false if there is
    /// no Stream corresponding to public_addr in the ConnectionSet, or if the Stream is corrupted, in which case Handle will
    /// transparently arrange for Stream to be dropped and re-established.
    pub fn send_to(&self, public_addr: &PublicKeyBytes, msg: &ConsensusMsg) -> bool {
        match self.connections.get(&public_addr) {
            Some(stream) => {
                match stream.write(&msg) {
                    Ok(_) => true,
                    Err(StreamCorruptedError) => {
                        self.connections.reconnect((*public_addr, stream.peer_addr().expect("Programming error: Loopback Stream is corrupted.").ip()));
                        false
                    },
                }
            },
            None => false,
        }
    }

    /// Asynchronously send a ConsensusMsg to all Participants in the ConnectionSet.
    pub fn broadcast(&self, msg: &ConsensusMsg) {
        let mut errored_conns = vec![];
        for (public_addr, stream) in &self.connections.iter() {
            if let Err(StreamCorruptedError) = stream.write(msg) {
                errored_conns.push((*public_addr, stream.peer_addr().expect("Programming error: Loopback Stream is corrupted.").ip()));
            }
        }

        for errored_conn in errored_conns {
            self.connections.reconnect(errored_conn);
        }
    }

    /// Attempts to receive a ConsensusMsg from a particular Participant for at most timeout Duration.
    pub fn recv_from(&self, public_addr: &PublicKeyBytes, timeout: Duration) -> Result<ConsensusMsg, RecvFromError> {
        match self.connections.get(public_addr) {
            Some(stream) => {
                stream.read(timeout).map_err(|e| match e {
                    StreamReadError::Corrupted => {
                        self.connections.reconnect((*public_addr, stream.peer_addr().expect("Programming error: Loopback Stream is corrupted.").ip()));
                       RecvFromError::NotConnected
                    },
                    StreamReadError::Timeout => RecvFromError::Timeout,
                    StreamReadError::LoopbackEmpty => RecvFromError::Timeout,
                })
            },
            None => Err(RecvFromError::NotConnected)
        }
    }

    /// Attempts to receive ConsensusMsg from *any* Participant for at most timeout Duration.
    pub fn recv_from_any(&self, timeout: Duration) -> Result<(PublicKeyBytes, ConsensusMsg), RecvFromError> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            match self.connections.get_random() {
                Some((public_addr, stream)) => match stream.read(Duration::ZERO) {
                    Ok(msg) => return Ok((public_addr, msg)),
                    Err(e) => match e {
                        StreamReadError::Corrupted => {
                            self.connections.reconnect((public_addr, stream.peer_addr().expect("Programming error: Loopback Stream is corrupted.").ip()));
                            continue
                        },
                        StreamReadError::Timeout => continue,
                        StreamReadError::LoopbackEmpty => continue
                    }
                }
                None => continue,
            }
        }

        Err(RecvFromError::Timeout)
    }

    pub fn update_participant_set(&mut self, new_participant_set: ParticipantSet) {
        self.connections.replace_set(new_participant_set);
    } 
}

pub enum RecvFromError {
    Timeout,
    NotConnected,
}
