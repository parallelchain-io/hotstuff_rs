use std::thread;
use std::collections::{HashMap, HashSet};
use crate::types;

type Proposal = types::ConsensusMsg;
type Vote = types::ConsensusMsg;

struct MsgQueue {
    proposals: HashMap<types::ViewNumber, HashSet<Proposal>>,
    votes: HashMap<types::ViewNumber, HashSet<Vote>>,
    cleanup_thread: thread::JoinHandle<()>,
}

impl MsgQueue {
    pub fn new() -> Self { todo!() }
    pub fn insert_proposal(&mut self, view_number: types::ViewNumber, proposal: Proposal) { todo!() }
    pub fn take_proposals(&mut self, view_number: types::ViewNumber) -> Option<Vec<Proposal>> { todo!() }
    pub fn insert_vote(&mut self, view_number: types::ViewNumber, vote: Vote) { todo!() }
    pub fn take_votes(&mut self, view_number: types::ViewNumber) -> Option<Vec<Vote>> { todo!() }

    fn delete_if_less_than(&mut self, view_number: types::ViewNumber) { todo!() }
}