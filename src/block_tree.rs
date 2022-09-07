use std::sync::Arc;
use std::convert::identity;
use borsh::{BorshDeserialize, BorshSerialize};
use rocksdb::{DB, WriteBatch, Snapshot};
use crate::config::BlockTreeConfig;
use crate::stored_types::{WriteSet, ChildrenList, Key, Value};
use crate::msg_types::{Block, BlockHash, ViewNumber, BlockHeight, Datum, Data, QuorumCertificate};

pub fn open(block_tree_config: &BlockTreeConfig) -> (BlockTreeWriter, BlockTreeSnapshotFactory) {
    let db = DB::open_default(block_tree_config.db_path.clone())
        .expect("Configuration error: fail to open DB.");
    let db = Arc::new(db);
    
    let block_tree_writer = BlockTreeWriter {
        db: Arc::clone(&db),
    };

    let block_tree_snapshot_factory = BlockTreeSnapshotFactory {
        db: Arc::clone(&db),
    };

    (block_tree_writer, block_tree_snapshot_factory)
}

fn initialize(block_tree_writer: &mut BlockTreeWriter) {
    todo!()
}

/// Used exclusively by the single thread of the Engine module.
pub struct BlockTreeWriter {
    db: Arc<DB>,
}

impl BlockTreeWriter {
    /// # Safety
    /// This function assumes that `block`:
    /// 1. Has not been inserted before,
    /// 2. Satisfies the SafeBlock predicate, and
    /// 3. That its parent, grandparent, great-grandparent, and great-great-grandparent are in the BlockTree.
    /// 
    /// To satisfy each assumption, this codebase maintains the following invariants:
    /// 1. Consensus proceeds in strictly increasing view numbers, so no two Blocks can have the same view number.
    /// 2. `crate::StateMachine` always checks the SafeBlock predicate before calling this function.
    /// 3. `initialize` does not only add a Genesis Block to the empty BlockTree, it also pads it with three additional
    ///    descendant Blocks. These four blocks become the parent, grandparent, great-grandparent, and great-great-grandparent
    ///    of the first Block agreed upon by consensus.
    pub fn insert_block(&self, block: &Block, write_set: &WriteSet) {
        let mut wb = WriteBatch::default();

        let parent_block = self.get_block(&block.justify.block_hash).unwrap();
        let grandparent_block = self.get_block(&parent_block.justify.block_hash).unwrap();
        let great_grandparent_block = self.get_block(&grandparent_block.justify.block_hash).unwrap(); 
        let great_great_grandparent_block = self.get_block(&great_grandparent_block.justify.block_hash).unwrap();

        // 1. Insert block to parent's ChildrenList.
        let parent_children = self.get_children_list(&parent_block.hash).map_or(ChildrenList::new(), identity);
        Self::set_children_list(&mut wb, &parent_block.hash, &parent_children);

        // 2. Insert block to the NODES keyspace. 
        Self::set_block(&mut wb, &block);

        // 3. Insert block's write_set to the WriteSet keyspace.
        Self::set_write_set(&mut wb, &block.hash, write_set);

        // 4. Check if great_grandparent.height is greater than HIGHEST_COMMITTED_NODE.height. If so, apply great_grandparent's writes to World State.
        // TODO: argue that no more than one Block can become committed because of a single insertion.
        let highest_committed_block = self.get_highest_committed_block();
        if great_grandparent_block.height > highest_committed_block.height {
            let great_grandparent_writes = self.get_write_set(&great_grandparent_block.hash).map_or(WriteSet::new(), identity);
            Self::apply_write_set(&mut wb, &great_grandparent_writes);
            Self::delete_write_set(&mut wb, &great_grandparent_block.hash);

            // 4.1. Update HIGHEST_COMMITTED_NODE.
            Self::set_highest_committed_block(&mut wb, &great_grandparent_block.hash);
        } 

        // 5. Abandon siblings of great_grandparent_block.
        let great_great_grandparent_children = self.get_children_list(&great_great_grandparent_block.hash).unwrap(); 
        let siblings = great_great_grandparent_children
            .iter()
            .filter(|child_hash| **child_hash != great_great_grandparent_block.hash);
        for sibling_hash in siblings {
            self.delete_branch(&mut wb, &sibling_hash)
        }
                
        // 6. Update Special Keys (HIGHEST_COMMITTED_NODE was updated in Step 4.1).

        // 6.1. LOCKED_VIEW. As `block` is assumed to satisfy the SafeBlock predicate, the QC it carries is guaranteed to have a view number greater than or equal
        // to locked_view.
        Self::set_locked_view(&mut wb, parent_block.justify.view_number);

        // 6.2. TOP_NODE.
        Self::set_top_block(&mut wb, &block.hash);

        // 7. Write changes to persistent storage.
        self.db.write(wb)
            .expect("Programming or Configuration error: fail to insert Block into persistent storage.");
    }
}

// Getters and setters for Special Keys.
impl BlockTreeWriter {
    fn set_locked_view(wb: &mut WriteBatch, view_num: ViewNumber) {
        wb.put(special_keys::LOCKED_VIEW, view_num.to_le_bytes());
    }

    pub fn get_locked_view(&self) -> u64 {
        u64::from_le_bytes(self.db.get(special_keys::LOCKED_VIEW).unwrap().unwrap().try_into().unwrap())
    }

    fn set_top_block(wb: &mut WriteBatch, block_hash: &BlockHash) {
        wb.put(special_keys::HASH_OF_TOP_NODE, block_hash)
    }

    pub fn get_top_block(&self) -> Block {
        let top_block_hash = BlockHash::try_from(self.db.get(special_keys::HASH_OF_TOP_NODE).unwrap().unwrap()).unwrap();
        self.get_block(&top_block_hash).unwrap()
    }
    
    fn set_highest_committed_block(wb: &mut WriteBatch, block_hash: &BlockHash) {
        wb.put(special_keys::HASH_OF_HIGHEST_COMMITTED_NODE, block_hash)
    }

    pub fn get_highest_committed_block(&self) -> Block {
        let highest_committed_block_hash = BlockHash::try_from(self.db.get(special_keys::HASH_OF_HIGHEST_COMMITTED_NODE).unwrap().unwrap()).unwrap();
        self.get_block(&highest_committed_block_hash).unwrap()
    } 
}

// Getters and setters for Blocks and chains of Blocks.
impl BlockTreeWriter {
    fn set_block(wb: &mut WriteBatch, block: &Block) {
        wb.put(prefix(special_prefixes::NODES, &block.hash), block.try_to_vec().unwrap());
    }

    pub fn get_block(&self, block_hash: &BlockHash) -> Option<Block> {
        Some(Block::deserialize(&mut &self.db.get(prefix(special_prefixes::NODES, block_hash)).unwrap()?[..]).unwrap())
    }

    fn delete_branch(&self, wb: &mut WriteBatch, tail_block_hash: &BlockHash) {
        // 1. Delete children.
        if let Some(children) = self.get_children_list(tail_block_hash) {
            for child_hash in children.iter() {
                self.delete_branch(wb, &child_hash);
            }
        }

        // 2. Delete tail.
        Self::delete_block(wb, tail_block_hash);
        Self::delete_write_set(wb, tail_block_hash);
        Self::delete_children_list(wb, tail_block_hash);
    }

    fn delete_block(wb: &mut WriteBatch, block_hash: &BlockHash) {
        wb.delete(prefix(special_prefixes::NODES, block_hash))
    }
}

// Getters and Setters for WriteSets.
impl BlockTreeWriter {
    pub fn set_write_set(wb: &mut WriteBatch, block_hash: &BlockHash, write_set: &WriteSet) {
        wb.put(prefix(special_prefixes::WRITE_SETS, block_hash), write_set.try_to_vec().unwrap());
    }

    pub fn get_write_set(&self, block_hash: &BlockHash) -> Option<WriteSet> {
        Some(WriteSet::deserialize(&mut &self.db.get(&prefix(special_prefixes::WRITE_SETS, block_hash)).unwrap()?[..]).unwrap())
    }

    pub fn apply_write_set(wb: &mut WriteBatch, write_set: &WriteSet) {
        for (key, value) in write_set.iter() {
            wb.put(prefix(special_prefixes::WORLD_STATE, key), value)
        }
    }

    pub fn get_from_storage(&self, key: &Key) -> Value {
        self.db.get(prefix(special_prefixes::WORLD_STATE, key)).unwrap().map_or(Value::new(), identity)
    }

    pub fn delete_write_set(wb: &mut WriteBatch, block_hash: &BlockHash) {
        wb.delete(prefix(special_prefixes::WRITE_SETS, block_hash))
    } 
}

// Getters and Setters for ChildrenLists.
impl BlockTreeWriter {
    pub fn set_children_list(wb: &mut WriteBatch, block_hash: &BlockHash, children_list: &ChildrenList) {
        wb.put(prefix(special_prefixes::CHILDREN_LISTS, block_hash), children_list.try_to_vec().unwrap())
    }

    pub fn get_children_list(&self, block_hash: &BlockHash) -> Option<ChildrenList> {
        Some(ChildrenList::deserialize(&mut &self.db.get(&prefix(special_prefixes::CHILDREN_LISTS, block_hash)).unwrap()?[..]).unwrap())
    }

    pub fn delete_children_list(wb: &mut WriteBatch, block_hash: &BlockHash) {
        wb.delete(prefix(special_prefixes::CHILDREN_LISTS, block_hash))
    }
}


/// Shared between the multiple threads of the Block Tree REST API.
#[derive(Clone)]
pub struct BlockTreeSnapshotFactory {
    db: Arc<DB>,
}

impl BlockTreeSnapshotFactory {
    pub fn snapshot(&self) -> BlockTreeSnapshot {
        BlockTreeSnapshot {
            db_snapshot: self.db.snapshot()
        }
    }
}

/// Reads BlockTree as if it holds a read-lock on the Database.
pub struct BlockTreeSnapshot<'a> {
    db_snapshot: Snapshot<'a>,
}

impl<'a> BlockTreeSnapshot<'a> {
    pub fn get_block_by_hash(&self, block_hash: &BlockHash) -> Option<Block> {
        Some(Block::deserialize(&mut &self.db_snapshot.get(prefix(special_prefixes::NODES, block_hash)).unwrap()?[..]).unwrap())
    }

    pub fn get_block_by_height(height: &BlockHeight) -> Option<Block> {
        todo!()
    }

    pub fn get_block_height(hash: &BlockHash) -> Option<BlockHeight> {
        todo!()
    }

    pub fn get_block_justify_by_hash(hash: &BlockHash) -> Option<QuorumCertificate> {
        todo!()
    }

    pub fn get_block_justify_by_height(height: &BlockHeight) -> Option<QuorumCertificate> {
        todo!()
    }

    pub fn get_block_command_list_by_hash(hash: &BlockHash) -> Option<Data> {
        todo!()
    }

    pub fn get_block_command_list_by_height(height: &BlockHeight) -> Option<Data> {
        todo!()
    }

    pub fn get_block_command_by_hash(hash: &BlockHash, index: usize) -> Option<Datum> {
        todo!()
    }

    pub fn get_block_command_by_height(height: &BlockHeight) -> Option<Datum> {
        todo!()
    }

    pub fn get_child(&self, parent_block_hash: &BlockHash) -> Result<Block, ChildrenNotYetCommittedError> {
        let highest_committed_block_hash = BlockHash::try_from(self.db_snapshot.get(special_keys::HASH_OF_HIGHEST_COMMITTED_NODE).unwrap().unwrap()).unwrap();
        let highest_committed_block = self.get_block_by_hash(&highest_committed_block_hash).unwrap();
        let parent_block = self.get_block_by_hash(&parent_block_hash).unwrap();
        if parent_block.height >= highest_committed_block.height {
            return Err(ChildrenNotYetCommittedError)
        }

        let parent_children = ChildrenList::deserialize(&mut &self.db_snapshot.get(&prefix(special_prefixes::CHILDREN_LISTS, parent_block_hash)).unwrap().unwrap()[..]).unwrap();
        // Safety: parent_children.len() must be 1, since parent is an ancestor of a committed Block. 
        let child_hash = parent_children.iter().next().unwrap();
        Ok(self.get_block_by_hash(child_hash).unwrap())
    }

    pub fn get_top_block(&self) -> Block {
        let top_block_hash = BlockHash::try_from(self.db_snapshot.get(special_keys::HASH_OF_TOP_NODE).unwrap().unwrap()).unwrap();
        self.get_block_by_hash(&top_block_hash).unwrap()
    }

    pub fn get_highest_committed_block(&self) -> Block {
        let highest_committed_block_hash = BlockHash::try_from(self.db_snapshot.get(special_keys::HASH_OF_HIGHEST_COMMITTED_NODE).unwrap().unwrap()).unwrap();
        self.get_block_by_hash(&highest_committed_block_hash).unwrap()
    }
}

pub struct ChildrenNotYetCommittedError;

mod special_prefixes {
    pub(super) const NODES: [u8; 1] = [00];
    pub(super) const WRITE_SETS: [u8; 1] = [01];
    pub(super) const CHILDREN_LISTS: [u8; 1] = [02];
    pub(super) const WORLD_STATE: [u8; 1] = [03]; 
}

fn prefix(prefix: [u8; 1], additional_key: &[u8]) -> Vec<u8> {
    let mut prefixed_key = Vec::with_capacity(prefix.len() + additional_key.len());
    prefixed_key.extend_from_slice(&prefix);
    prefixed_key.extend_from_slice(additional_key);
    prefixed_key
}

mod special_keys {
    pub(super) const LOCKED_VIEW: [u8; 1] = [10];
    pub(super) const HASH_OF_TOP_NODE: [u8; 1] = [11];
    pub(super) const HASH_OF_HIGHEST_COMMITTED_NODE: [u8; 1] = [12];
}
