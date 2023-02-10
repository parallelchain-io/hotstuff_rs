use std::collections::{HashSet, HashMap};
use std::sync::mpsc::{self, Sender, Receiver};
use crate::types::*;
use crate::messages::*;

/// A mocked network stub which uses channels to send messages between peers. It simulates a network scenario where
/// there is a fixed set of peers somewhere in the universe ('all_peers'), but upon construction, the stub is connected
/// to none of them ('connected_peers' is empty).  
struct MockNetworkStub {
    all_peers: HashMap<PublicKey, Sender<Message>>,
    connected_peers: HashSet<PublicKey>,
    inbox: Receiver<Message>,
}


fn mock_network(peers: Vec<PublicKey>) -> Vec<(PublicKey, MockNetworkStub)> {
    let ((inboxes, outboxes)) 
    for peer in peers 
}

/// 
struct MockApp {

}