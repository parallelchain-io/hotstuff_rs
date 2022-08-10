use crate::node_tree::{self, NodeTree};
use crate::basic_types;

pub trait Application {
    fn create_leaf(
        &mut self,
        // node_tree and parent_hash can be combined into a struct that implements a more convenient API.
        node_tree: &NodeTree, 
        parent_hash: &basic_types::NodeHash, 
        state: &mut node_tree::State
    ) -> (basic_types::Command, node_tree::State);

    fn validate(
        &mut self, 
        node: basic_types::Node, 
        node_tree: &NodeTree, 
        parent_hash: &basic_types::NodeHash,
        state: &mut node_tree::State
    ) -> bool;
}