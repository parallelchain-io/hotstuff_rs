use std::time::{Instant, Duration};
use hotstuff_rs_types::messages::ConsensusMsg;
use hotstuff_rs_types::identity::{PublicKeyBytes, ParticipantSet, PublicKey};
use crate::config::NetworkingConfiguration;
use crate::ipc::{ConnectionSet, StreamReadError};

use super::stream::StreamCorruptedError;

/// Handle exposes methods for sending ConsensusMsgs to, and receiving ConsensusMsgs from other Participants. All of Handle's methods
/// transparently handle errored streams by calling `ConnectionSet::reconnect` on them. 
pub struct Handle {
    connections: ConnectionSet,
}

impl Handle {
    pub fn new(static_participant_set: ParticipantSet, my_public_key: PublicKey, ipc_config: NetworkingConfiguration) -> Handle {
        Handle {
            connections: ConnectionSet::new(static_participant_set, my_public_key.to_bytes(), ipc_config.clone()),
        }
    }

    #[allow(dead_code)]
    pub fn update_participant_set(&mut self, new_participant_set: ParticipantSet) {
        self.connections.replace_set(new_participant_set);
    } 
}

impl AbstractHandle for Handle {
    fn send_to(&mut self, public_addr: &PublicKeyBytes, msg: &ConsensusMsg) -> Result<(), NotConnectedError> {
        if let Some(stream) = self.connections.get(&public_addr) {
            if let Err(StreamCorruptedError) = stream.write(&msg) {
                self.connections.reconnect((*public_addr, stream.peer_addr().expect("Programming error: Loopback Stream is corrupted.").ip()));
                Err(NotConnectedError)
            } else {
                Ok(())
            }
        } else {
            Err(NotConnectedError)
        }
    }

    fn broadcast(&mut self, msg: &ConsensusMsg) {
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

    fn recv_from(&mut self, public_addr: &PublicKeyBytes, timeout: Duration) -> Result<ConsensusMsg, RecvFromError> {
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

    fn recv_from_any(&mut self, timeout: Duration) -> Result<(PublicKeyBytes, ConsensusMsg), RecvFromError> {
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
}

pub struct NotConnectedError;

pub enum RecvFromError {
    Timeout,
    NotConnected,
}

pub(crate) trait AbstractHandle {
    /// Asynchronously send a ConsensusMsg to a specific Participant, identified by their PublicAddr. If the Handle
    /// does not have a connection to the specified Participant, this method returns quietly. 
    fn send_to(&mut self, public_addr: &PublicKeyBytes, msg: &ConsensusMsg) -> Result<(), NotConnectedError>;
    
    /// Asynchronously send a ConsensusMsg to all Participants connected to this Handle.
    fn broadcast(&mut self, msg: &ConsensusMsg);

    /// Attempts to receive a ConsensusMsg from a particular Participant for at most timeout Duration. 
    fn recv_from(&mut self, public_addr: &PublicKeyBytes, timeout: Duration) -> Result<ConsensusMsg, RecvFromError>;

    /// Attempts to receive ConsensusMsg from *any* Participant for at most timeout Duration.
    fn recv_from_any(&mut self, timeout: Duration) -> Result<(PublicKeyBytes, ConsensusMsg), RecvFromError>;
}
