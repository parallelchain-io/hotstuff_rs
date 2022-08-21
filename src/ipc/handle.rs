use std::io;
use std::ops::Deref;
use std::time::Duration;
use crate::msg_types::{ConsensusMsg, PublicAddress};
use crate::ipc::ManagedConnSet;

pub struct Handle(ManagedConnSet);

// `send_to` and `broadcast` are non-blocking. 
// `recv` and `recv_from_any` are blocking with a timeout (typically set to some function of TNT).
// All functions transparently handle errored streams by calling `ManagedConnSet::reconnect` on them. 
impl Handle {
    // Returns false if there is no Stream corresponding to public_address in ConnSet.
    pub fn send_to(&self, participant: PublicAddress, msg: ConsensusMsg) -> io::Error {
        todo!()
    }

    pub fn broadcast(&mut self, msg: ConsensusMsg) {
        let mut errored_streams = Vec::new();
        for (_, stream) in &self.0.iter() {
            if let Err(_) = stream.write(&msg) {
                errored_streams.push(stream.peer_addr());
            }
        }

        self.0.reconnect(errored_streams);
    }

    // # Possible ErrorKinds
    // 1. ErrorKind::TimedOut.
    // 2. ErrorKind::NotConnected.
    pub fn recv_from(&self, participant: PublicAddress, timeout: Duration) -> io::Result<ConsensusMsg> {
        todo!()
    }

    // # Possible ErrorKinds
    // 1. ErrorKind::TimedOut.
    // 2. ErrorKind::NotConnected.
    pub fn recv_from_any(&self, timeout: Duration) -> io::Result<ConsensusMsg> {
        todo!()
        // Repeatedly call `ManagedConnSet::get_random` and reading on the returned Stream with a ZERO timeout until a ConsensusMsg is got.
    }
}
