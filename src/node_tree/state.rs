use std::rc::Rc;
use crate::node_tree::Database;
use crate::node_tree::types::{Key, Value, WriteSet};

pub struct State;

impl State {
    pub(crate) fn new(parent_writes: Rc<WriteSet>, grandparent_writes: Rc<WriteSet>, db: &Database) -> State {
        todo!()
    }

    pub fn set(&mut self, key: Key, value: Value) {
        todo!()
    }

    pub fn get(&self, key: Key) -> Value {
        todo!()
    }
}
