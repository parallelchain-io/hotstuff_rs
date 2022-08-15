use std::net::IpAddr;
use std::thread;
use std::sync::mpsc;
use crate::msg_types::{PublicAddress, ConsensusMsg};
use crate::ipc::{self, ConnectionSet};

pub struct Manager {
    establisher: thread::JoinHandle<()>,
    sender: thread::JoinHandle<()>,
}

impl Manager {
    fn new() -> (Manager, ipc::Handle) {
        let connections = ConnectionSet::new();
        let (to_establisher, establisher_from_handles) = mpsc::channel();
        let (to_sender, sender_from_handles ) = mpsc::channel();
        let manager = Manager {
            establisher: Self::establisher(connections.clone(), establisher_from_handles),
            sender: Self::sender(connections.clone(), sender_from_handles),
        };
        let handle = ipc::Handle::new(connections, to_establisher, to_sender);

        (manager, handle)
    }

    fn establisher(connections: ConnectionSet, requests: mpsc::Receiver<EstablishRequest>) -> thread::JoinHandle<()> {
        todo!()
    } 

    fn sender(connections: ConnectionSet, request: mpsc::Receiver<SendRequest>) -> thread::JoinHandle<()> {
        todo!()
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        todo!()
    }
}

pub struct EstablishRequest(Vec<(PublicAddress, IpAddr)>);

pub enum SendRequest {
    SendTo(PublicAddress, ConsensusMsg),
    Broadcast(ConsensusMsg),
}