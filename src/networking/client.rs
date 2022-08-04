use crate::types;

pub struct Client;

impl Client {
    fn open(participants: types::ParticipantSet) -> Client { todo!() }
    // For version 2: with dynamic ParticipantSets.
    // fn update(&mut self, participants: types::ParticipantSet) { todo!() };
}

// Consensus TCP API.
impl Client {
    pub fn send(&self, to_participant: types::PublicKey, msg: types::ConsensusMsg) { todo!() } 
    pub fn broadcast(&self, msg: types::ConsensusMsg) { todo!() } // This is non-blocking since we don't want to wait for an acknowledgement from a slow Participant.
    pub fn read(&self) -> types::ConsensusMsg { todo!() } // This is blocking and discards any un-deserializable messages.
}

// Node Tree API.
impl Client {
    pub fn get_nodes(&self, starting_from: types::NodeHash) -> Option<Vec<types::Node>> { todo!() }
}
