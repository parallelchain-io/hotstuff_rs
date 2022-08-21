use std::iter::IntoIterator;
use std::collections::{hash_map, HashMap};
use crate::ipc;
use crate::msg_types::PublicAddress;

pub struct ConnSet {
    connections: HashMap<PublicAddress, ipc::Stream>,
    entries: Vec<PublicAddress>, // Used to implement `random`.
}

impl ConnSet {
    pub fn new() -> ConnSet { todo!() }
    pub fn insert(&mut self, public_addr: PublicAddress, stream: ipc::Stream) { todo!() }
    pub fn remove(&mut self, public_addr: PublicAddress) -> bool { todo!() }
    pub fn get(&self, public_addr: PublicAddress) -> Option<&ipc::Stream> { todo!() }
    pub fn get_random(&self) -> Option<&ipc::Stream> { todo!() } // Returns None is ConnSet is empty.
}

impl<'a> IntoIterator for &'a ConnSet {
    type Item = (&'a PublicAddress, &'a ipc::Stream);
    type IntoIter = hash_map::Iter<'a, PublicAddress, ipc::Stream>;

    fn into_iter(self) -> Self::IntoIter {
        self.connections.iter()    
    }
}