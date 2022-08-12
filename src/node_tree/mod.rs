mod node_tree;
pub(crate) use node_tree::*;

/// database defines the persistent storage model of the NodeTree.
mod storage;

/// stored_types defines types that NodeTree persists in its Database, including functions for encoding them into bytes.
mod stored_types;
pub use stored_types::{Key, Value, WriteSet};
