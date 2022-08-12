use std::sync::Arc;
use std::collections::HashSet;
use rocksdb;
use crate::node_tree::{Key, Value, WriteSet};
use crate::msg_types::{self, NodeHash, SerDe};

#[derive(Clone)]
pub(in crate::node_tree) struct Database(Arc<rocksdb::DB>);

impl Database {
    pub fn open() -> Result<Database, rocksdb::Error> {
        const DB_PATH: &str = "./database";
        let db = Arc::new(rocksdb::DB::open_default(DB_PATH)?);

        Ok(Database(db))
    } 

    pub fn get_node(&self, hash: &NodeHash) -> Result<Option<msg_types::Node>, rocksdb::Error> {
        self.get_with_prefix(&special_keys::NODES_PREFIX, hash)
    }

    pub fn get_write_set(&self, of_node: &NodeHash) -> Result<Option<WriteSet>, rocksdb::Error> {
        self.get_with_prefix(&special_keys::WRITE_SETS_PREFIX, of_node)
    }

    pub fn get_children(&self, of_node: &NodeHash) -> Result<Option<HashSet<NodeHash>>, rocksdb::Error> {
        self.get_with_prefix(&special_keys::CHILDREN_PREFIX, of_node)
    }

    pub fn get_from_state(&self, key: &Key) -> Result<Option<Value>, rocksdb::Error> {
        self.get_with_prefix(&special_keys::STATE_PREFIX, key)
    }

    pub fn get_node_with_generic_qc(&self) -> Result<Option<msg_types::Node>, rocksdb::Error> {
        self.get_with_prefix(&special_keys::NODE_CONTAINING_GENERIC_QC, &[])
    }

    pub fn write(&self, write_batch: WriteBatch) -> Result<(), rocksdb::Error> {
        self.0.write(write_batch.0)
    }

    fn get_with_prefix<T: SerDe>(&self, prefix: &special_keys::Prefix, key: &[u8]) -> Result<Option<T>, rocksdb::Error> {
        let bs = self.0.get(special_keys::prefix(prefix, key))?;
        match bs {
            None => Ok(None),
            Some(bs) => Ok(Some(T::deserialize(bs).unwrap()))
        }
    }
}

#[derive(Default)]
pub(in crate::node_tree) struct WriteBatch(rocksdb::WriteBatch);

impl WriteBatch {
    pub fn set_node(&mut self, hash: &NodeHash, node: msg_types::Node) {
        todo!()
    }

    pub fn set_write_set(&mut self, of_node: &NodeHash) {
        todo!()
    }

    pub fn set_children(&mut self, of_node: &NodeHash) {
        todo!()
    }

    pub fn apply_writes_to_state(&mut self, writes: WriteBatch) {
        todo!()
    }
}

mod special_keys {
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