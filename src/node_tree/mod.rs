pub(crate) mod api;

/// node_tree defines NodeTree, a directed acyclic graph of Nodes that grows over time through consensus. 
mod node_tree;
pub(crate) use node_tree::*;

/// state defines State, which abstracts a persistent set of Key-Value mappings that Apps mutate in response to Commands.
mod world_state;
pub(crate) use world_state::WorldState;

/// storage defines two structs: Database and WriteBatch, which handles all interactions with persistent storage for HotStuff-rs and Apps. Apps
/// do not use the structs defined in storage directly, but instead use abstractions like NodeTree, State, and Command. 
pub(crate) mod storage;
pub(crate) use storage::{Database, WriteBatch};

/// stored_types defines types that NodeTree persists in its Database *and* are not sent over the wire (types sent over the wire are defined
/// in crate::msg_types).
pub(crate) mod stored_types;
