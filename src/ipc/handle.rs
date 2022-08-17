use std::sync::{Arc, RwLock, mpsc};
use crate::msg_types::{ConsensusMsg, PublicAddress};
use crate::ipc::{ConnectionSet, manager};

#[derive(Clone)]
pub struct Handle {
    connections: Arc<ConnectionSet>,
    to_establisher: mpsc::Sender<manager::EstablishRequest>,
    to_sender: mpsc::Sender<manager::SendRequest>,
}

impl Handle {
    pub fn new(
        connections: Arc<RwLock<ConnectionSet>>,
        to_establisher: mpsc::Sender<manager::EstablishRequest>,
        to_sender: mpsc::Sender<manager::SendRequest>
    ) -> Handle {
        todo!()
    }

    pub fn send_to(msg: ConsensusMsg, participant: PublicAddress) {
        todo!()
    }

    pub fn broadcast(msg: ConsensusMsg) {
        todo!()
    }

    pub fn recv_from(participant: PublicAddress) -> ConsensusMsg {
        todo!()
    }

    fn recv_from_any() -> ConsensusMsg {
        todo!()
    }
}

