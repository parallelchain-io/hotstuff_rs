use crate::node_tree::{self, State};
use crate::msg_types;

pub trait App {
    fn create_leaf(&mut self, parent_node: node_tree::Node) -> (msg_types::Command, State);

    fn try_execute(&mut self, node: node_tree::Node) -> Result<State, InvalidNodeError>;
}

pub struct InvalidNodeError; 
