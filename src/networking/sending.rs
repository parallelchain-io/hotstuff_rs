//! Functions and types for sending messages to the P2P network.

use ed25519_dalek::VerifyingKey;

use super::{messages::Message, network::Network};

/// Handle for sending and broadcasting messages to the [`Network`].
///
/// It can be used to send or broadcast instances of any type that implement the [`Into<Message>`]
/// trait.
#[derive(Clone)]
pub(crate) struct SenderHandle<N: Network> {
    network: N,
}

impl<N: Network> SenderHandle<N> {
    pub(crate) fn new(network: N) -> Self {
        Self { network }
    }

    pub(crate) fn send<S: Into<Message>>(&mut self, peer: VerifyingKey, msg: S) {
        self.network.send(peer, msg.into())
    }

    pub(crate) fn broadcast<S: Into<Message>>(&mut self, msg: S) {
        self.network.broadcast(msg.into())
    }
}
