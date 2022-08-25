use std::time::Instant;
use crate::node_tree::{self, State};
use crate::msg_types;

pub trait App: Send + 'static {
    fn create_leaf(&mut self, parent_node: node_tree::Node, deadline: Instant) -> (msg_types::Command, State);

    fn try_execute(&mut self, node: node_tree::Node, deadline: Instant) -> Result<State, TryExecuteError>;
}

pub enum TryExecuteError {
    RanOutOfTime,
    InvalidNode,
}
