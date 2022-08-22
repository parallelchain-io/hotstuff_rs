//! ## "None values are treated as equivalent to Empty values."
//! Definitions in node_tree::Storage follow this rule to simplify the implementation of callers. This feels somewhat
//! unidiomatic, but frees callers from having to match on whether the return value they receive from a getter is a `Some`
//! variant or a `None`. The only exception to this rule is Node, since Node does not have an obvious or cheap-to-create 
//! Empty value.

use std::sync::Arc;
use rocksdb;
use crate::node_tree::stored_types::{Key, Value, WriteSet, Children};
use crate::msg_types::{self, NodeHash, SerDe};

/// Database handles all of HotStuff-rs' interactions with persistent storage. By itself, it only exposes getters. To write into
/// Database, construct a `database::WriteBatch`, collect writes, and commit them atomically into Database using `write`.
/// 
/// This functionality is implemented separately from `NodeTree` for two reasons:
/// 1. NodeTree is a protocol-defined construct, while the specifics of interaction with persistent storage is an implementation
///    detail. If and when we decide to shake up the storage stack (for performance, stability, etc.), we'd like to be able to do
///    so without modifying protocol-related logic.
/// 2. `Database` is used in another place besides `NodeTree`, namely `node_tree::Node`. Had we not separated storage logic into
///    this struct, `Node` would need to own a reference to `NodeTree`, a weird inversion of the intuitive hierarchy between the
///    two types. 
#[derive(Clone)]
pub(crate) struct Database(Arc<rocksdb::DB>);

impl Database {
    pub fn open() -> Result<Database, rocksdb::Error> {
        const DB_PATH: &str = "./database";
        let db = Arc::new(rocksdb::DB::open_default(DB_PATH)?);

        Ok(Database(db))
    } 

    pub fn get_node(&self, hash: &NodeHash) -> Result<Option<msg_types::Node>, rocksdb::Error> {
        let keyspaced_key = keyspaces::prefix(&keyspaces::NODES_PREFIX, hash);
        match self.0.get(&keyspaced_key)? {
            Some(bs) => Ok(Some(msg_types::Node::deserialize(&bs).unwrap())),
            None => Ok(None)
        }
    }

    pub fn get_write_set(&self, of_node: &NodeHash) -> Result<WriteSet, rocksdb::Error> {
        let keyspaced_key = keyspaces::prefix(&keyspaces::WRITE_SETS_PREFIX, of_node);
        match self.0.get(&keyspaced_key)? {
            Some(bs) => Ok(WriteSet::deserialize(&bs).unwrap()),
            None => Ok(WriteSet::new())
        }
    }

    pub fn get_children(&self, of_node: &NodeHash) -> Result<Children, rocksdb::Error> {
        let keyspaced_key = keyspaces::prefix(&keyspaces::CHILDREN_PREFIX, of_node);
        match self.0.get(&keyspaced_key)? {
            Some(bs) => Ok(Children::deserialize(&bs).unwrap()),
            None => Ok(Children::new())
        }
    }

    pub fn get_node_with_generic_qc(&self) -> Result<msg_types::Node, rocksdb::Error> {
        // SAFETY: NODE_CONTAINING_GENERIC_QC is initialized to the Genesis Node during Participant initialization,
        // so this can never be None.
        let bs = self.0.get(&keyspaces::NODE_CONTAINING_GENERIC_QC)?.unwrap();
        Ok(msg_types::Node::deserialize(&bs).unwrap())
    }

    pub fn get_from_state(&self, key: &Key) -> Result<Value, rocksdb::Error> {
        let keyspaced_key = keyspaces::prefix(&keyspaces::STATE_PREFIX, key);
        match self.0.get(&keyspaced_key)? {
            Some(value) => Ok(value),
            None => Ok(Value::new()), 
        }
    }

    pub fn write(&self, write_batch: WriteBatch) -> Result<(), rocksdb::Error> {
        self.0.write(write_batch.0)
    }
}

#[derive(Default)]
pub(crate) struct WriteBatch(rocksdb::WriteBatch);

impl WriteBatch {
    pub fn new() -> WriteBatch {
        WriteBatch(rocksdb::WriteBatch::default())
    }

    pub fn set_node(&mut self, hash: &NodeHash, node: Option<&msg_types::Node>) {
        let keyspaced_key = keyspaces::prefix(&keyspaces::NODES_PREFIX, hash);
        match node {
            Some(node) => self.0.put(keyspaced_key, node.serialize()),
            None => self.0.delete(keyspaced_key)
        } 
    }

    pub fn set_write_set(&mut self, of_node: &NodeHash, write_set: Option<&WriteSet>) {
        let keyspaced_key = keyspaces::prefix(&keyspaces::WRITE_SETS_PREFIX, of_node);
        match write_set {
            Some(write_set) => self.0.put(keyspaced_key, write_set.serialize()),
            None => self.0.delete(keyspaced_key),
        }
    }

    pub fn set_children(&mut self, of_node: &NodeHash, children: Option<&Children>) {
        let keyspaced_key = keyspaces::prefix(&keyspaces::CHILDREN_PREFIX, of_node);
        match children {
            Some(children) => self.0.put(keyspaced_key, children.serialize()),
            None => self.0.delete(keyspaced_key),
        }
    }

    pub fn apply_writes_to_state(&mut self, writes: &WriteSet) {
        for (key, value) in writes {
            let keyspaced_key = keyspaces::prefix(&keyspaces::STATE_PREFIX, key);
            if value.len() > 0 {
                self.0.put(keyspaced_key, value)
            } else {
                self.0.delete(keyspaced_key)
            }
        }
    }
}

mod keyspaces {
    pub(super) type Prefix = [u8; 1];

    pub const NODES_PREFIX: Prefix               = [00];
    pub const WRITE_SETS_PREFIX: Prefix          = [01];
    pub const CHILDREN_PREFIX: Prefix            = [02];
    pub const STATE_PREFIX: Prefix               = [03];

    pub const NODE_CONTAINING_GENERIC_QC: Prefix = [10];

    pub fn prefix(prefix: &Prefix, key: &[u8]) -> Vec<u8> {
        let mut prefixed_key = Vec::with_capacity(1 + key.len());
        prefixed_key.extend_from_slice(prefix);
        prefixed_key.extend_from_slice(key);

        prefixed_key
    }
}