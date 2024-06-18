use std::{
    collections::HashMap,
    sync::{
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc, Mutex,
    },
};

use ed25519_dalek::VerifyingKey;
use hotstuff_rs::{
    messages::Message,
    networking::Network,
    types::validators::{ValidatorSet, ValidatorSetUpdates},
};

/// A mock network stub which passes messages from and to threads using channels.  
#[derive(Clone)]
pub(crate) struct NetworkStub {
    my_verifying_key: VerifyingKey,
    all_peers: HashMap<VerifyingKey, Sender<(VerifyingKey, Message)>>,
    inbox: Arc<Mutex<Receiver<(VerifyingKey, Message)>>>,
}

impl Network for NetworkStub {
    fn init_validator_set(&mut self, _: ValidatorSet) {}

    fn update_validator_set(&mut self, _: ValidatorSetUpdates) {}

    fn send(&mut self, peer: VerifyingKey, message: Message) {
        if let Some(peer) = self.all_peers.get(&peer) {
            let _ = peer.send((self.my_verifying_key, message));
        }
    }

    fn broadcast(&mut self, message: Message) {
        for (_, peer) in &self.all_peers {
            let _ = peer.send((self.my_verifying_key, message.clone()));
        }
    }

    fn recv(&mut self) -> Option<(VerifyingKey, Message)> {
        match self.inbox.lock().unwrap().try_recv() {
            Ok(o_m) => Some(o_m),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!(),
        }
    }
}

pub(crate) fn mock_network(peers: impl Iterator<Item = VerifyingKey>) -> Vec<NetworkStub> {
    let mut all_peers = HashMap::new();
    let peer_and_inboxes: Vec<(VerifyingKey, Receiver<(VerifyingKey, Message)>)> = peers
        .map(|peer| {
            let (sender, receiver) = mpsc::channel();
            all_peers.insert(peer, sender);

            (peer, receiver)
        })
        .collect();

    peer_and_inboxes
        .into_iter()
        .map(|(my_verifying_key, inbox)| NetworkStub {
            my_verifying_key,
            all_peers: all_peers.clone(),
            inbox: Arc::new(Mutex::new(inbox)),
        })
        .collect()
}
