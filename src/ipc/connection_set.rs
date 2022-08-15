use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::net::TcpStream;
use crate::msg_types::PublicAddress;

#[derive(Clone)]
pub struct ConnectionSet(Arc<RwLock<HashMap<PublicAddress, Arc<TcpStream>>>>);

impl ConnectionSet {
    pub fn new() -> ConnectionSet {
        todo!()
    }

    pub fn get(to_participant: &PublicAddress) -> Option<Arc<TcpStream>> {
        todo!()
    }

    pub fn set(participant: &PublicAddress, connection: TcpStream) {
        todo!()
    }

    pub fn delete(participant: &PublicAddress) -> bool {
        todo!()
    }
}