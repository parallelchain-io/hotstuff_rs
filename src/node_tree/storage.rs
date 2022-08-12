use std::sync::Arc;
use std::collections::HashSet;
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
pub(in crate::node_tree) struct Database(Arc<rocksdb::DB>);

impl Database {
    pub fn open() -> Result<Database, rocksdb::Error> {
        const DB_PATH: &str = "./database";
        let db = Arc::new(rocksdb::DB::open_default(DB_PATH)?);

        Ok(Database(db))
    } 

    pub fn get_node(&self, hash: &NodeHash) -> Result<Option<msg_types::Node>, rocksdb::Error> {
        self.get_with_prefix(&keyspaces::NODES_PREFIX, hash)
    }

    pub fn get_write_set(&self, of_node: &NodeHash) -> Result<WriteSet, rocksdb::Error> {
        match self.get_with_prefix(&keyspaces::WRITE_SETS_PREFIX, of_node)? {
            Some(write_set) => Ok(write_set),
            None => Ok(WriteSet::new())
        }
    }

    pub fn get_children(&self, of_node: &NodeHash) -> Result<Children, rocksdb::Error> {
        match self.get_with_prefix(&keyspaces::CHILDREN_PREFIX, of_node)? {
            Some(children) => Ok(children),
            None => Ok(Children::new())
        }
    }

    pub fn get_node_with_generic_qc(&self) -> Result<msg_types::Node, rocksdb::Error> {
        Ok(self.get_with_prefix(&keyspaces::NODE_CONTAINING_GENERIC_QC, &[])?.unwrap())
    }

    pub fn get_from_state(&self, key: &Key) -> Result<Option<Value>, rocksdb::Error> {
        self.get_with_prefix(&keyspaces::STATE_PREFIX, key)
    }

    pub fn write(&self, write_batch: WriteBatch) -> Result<(), rocksdb::Error> {
        self.0.write(write_batch.0)
    }

    fn get_with_prefix<T: SerDe>(&self, prefix: &keyspaces::Prefix, key: &[u8]) -> Result<Option<T>, rocksdb::Error> {
        let bs = self.0.get(keyspaces::prefix(prefix, key))?;
        match bs {
            None => Ok(None),
            Some(bs) => Ok(Some(T::deserialize(bs).unwrap()))
        }
    }
}

#[derive(Default)]
pub(in crate::node_tree) struct WriteBatch(rocksdb::WriteBatch);

impl WriteBatch {
    pub fn new() -> WriteBatch {
        WriteBatch(rocksdb::WriteBatch::default())
    }

    pub fn set_node(&mut self, hash: &NodeHash, node: &msg_types::Node) {
        todo!()
    }

    pub fn set_write_set(&mut self, of_node: &NodeHash, write_set: &WriteSet) {
        todo!()
    }

    pub fn set_children(&mut self, of_node: &NodeHash, children: &Children) {
        todo!()
    }

    pub fn apply_writes_to_state(&mut self, writes: &WriteSet) {
        todo!()
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