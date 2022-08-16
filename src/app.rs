use crate::node_tree::{self, State};
use crate::msg_types;

pub trait App {
    fn create_leaf(
        &mut self,
        parent_node: node_tree::Node,
        state: State,
    ) -> (msg_types::Command, State);

    fn validate(
        &mut self, 
        node: node_tree::Node, 
        state: State
    ) -> bool;
}
