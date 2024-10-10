//! A "mock" (totally local) network for passing around HotStuff-rs messages.

use std::{
    collections::HashMap,
    sync::{
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc, Mutex,
    },
};

use ed25519_dalek::VerifyingKey;
use hotstuff_rs::{
    networking::{messages::Message, network::Network},
    types::{update_sets::ValidatorSetUpdates, validator_set::ValidatorSet},
};

/// A network stub that passes messages to and from nodes using channels.
///
/// ## Limitations
///
/// `NetworkStub`'s implementation of the [`Network`] trait's `init_validator_set` and
/// `update_validator_set` methods are no-ops. As a consequence, the set of peers reachable from a given
/// `NetworkStub` is fixed on construction by [`mock_network`].
///
/// Therefore, tests that dynamically change the validator set must "plan ahead" and create mock network
/// with "extra" `VerifyingKey`s, beyond the ones for the replicas that are started initially.
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

/// Create a vector of `NetworkStub`s, connecting the provided set of `peers`.
///
/// `NetworkStub`s feature in the returned vector in the same order as the provided `peers`, i.e.,
/// the i-th network stub is the network stub for the i-th peer.
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
