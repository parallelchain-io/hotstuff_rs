use std::time::Instant;
use crate::node_tree::{self, WorldState};
use crate::msg_types;

pub trait App: Send + 'static {
    fn create_leaf(
        &mut self, 
        parent_node: &node_tree::NodeHandle,
        state: WorldState,
        deadline: Instant
    ) -> (msg_types::Command, WorldState);

    fn try_execute(
        &mut self,
        node: &node_tree::NodeHandle,
        state: WorldState,
        deadline: Instant
    ) -> Result<WorldState, TryExecuteError>;
}

pub enum TryExecuteError {
    RanOutOfTime,
    InvalidNode,
}
