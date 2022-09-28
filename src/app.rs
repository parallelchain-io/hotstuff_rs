use std::convert::identity;
use std::ops::Deref;
use std::time::Instant;
use crate::block_tree::BlockTreeWriter;
use crate::stored_types::{StorageMutations, Key, Value};
use crate::msg_types::{Data, DataHash, Block as MsgBlock, BlockHash};

/// Methods that a type needs to implement to serve as a HotStuff-rs network's deterministic state transition function.
/// 
/// Besides implementing the functions specified in the trait, implementors of App are additionally expected to be *deterministic*. i.e., every
/// function it implements as part of the App trait should evaluate to the same value every time it is called with the same arguments.
pub trait App: Send + 'static {
    /// Called by the Algorithm state machine when this Participant becomes the Leader and has to propose a new Block that extends the branch
    /// of the BlockTree headed by `parent_block`. A view of Storage after executing `parent_block` is provided in `storage`. 
    /// 
    /// This function call should return at the latest by `deadline`. Otherwise, this view in which the Participant is the Leader is likely
    /// to fail because of view timeout.
    /// 
    /// # Return value
    /// A triple consisting of:
    /// 1. A Data. This will occupy the `data` field of the Block proposed by this Participant in this view. 
    /// 2. A DataHash. This will occupy the `data_hash` field of the Block proposed by the Participant in this view.
    /// 3. A WriteSet resulting from executing the provided Block on the provided Storage. This will be atomically applied into the Storage
    /// when the Block containing the returned Data becomes committed. 
    fn propose_block(
        &mut self, 
        parent_block: &Block,
        storage_snapshot: SpeculativeStorageReader,
        deadline: Instant
    ) -> (Data, DataHash, StorageMutations);

    /// Called by the Algorithm state machine when this Participant is a Replica and has to decide whether or not to vote on a Block (`block`)
    /// which was proposed by the Leader.
    /// 
    /// This function call should return at the latest by `deadline`. Otherwise, this Participant's Vote in this View may not be received in
    /// time by the next Leader.
    /// 
    /// # Return value
    /// If the Block is valid, the WriteSet that results from executing the provided Block on the provided Storage. Else, the reason why the
    /// Block is deemed invalid.
    fn validate_block(
        &mut self,
        block: &Block,
        storage_snapshot: SpeculativeStorageReader,
        deadline: Instant
    ) -> Result<StorageMutations, ExecuteError>;
}

/// Enumerates the circumstances in which an App could reject a proposed Block, causing this Participant to skip this round without voting.
pub enum ExecuteError {
    /// The Deadline was exceeded while processing the proposed Block.
    RanOutOfTime,

    /// The contents of the Block, in the context of its proposed position in the Block Tree, is invalid in the view of App-level validation rules.
    InvalidBlock,
}

/// A convenience wrapper around msg_types::Block which, besides allowing App's methods to look into the Block's fields, exposes methods for
/// traversing the branch that this Block heads.
pub struct Block {
    inner: MsgBlock,
    block_tree: BlockTreeWriter,
}

impl Block {
    pub(crate) fn new(block: MsgBlock, block_tree: BlockTreeWriter) -> Block {
        Block {
            inner: block,
            block_tree,
        }
    }

    /// Gets the parent of this Block. This returns None if called on the Genesis Block.
    pub fn get_parent(&self) -> Option<Block> {
        if self.justify.view_number == 0 {
            return None
        }

        let parent = self.block_tree.get_block(&self.inner.justify.block_hash)
            .expect("Programming error: Non-Genesis block does not have a parent.");

        let parent = Block {
            inner: parent,
            block_tree: self.block_tree.clone(),
        };

        Some(parent)
    }
}

impl Deref for Block {
    type Target = MsgBlock;

    fn deref(&self) -> &Self::Target {
        &self.inner 
    }
}

/// A read-and-writable view of the World State after executing all the Blocks in some branch of the Block Tree. The writes (`set`s)
/// applied into this WorldStateHandle only become permanent when the Block containing the Command that it corresponds to becomes committed. 
/// 
/// This structure should NOT be used in App code outside the context of `create_leaf` and `execute`.
/// 
/// # 'Speculative' Storage vs 'non-speculative' Storage
/// Blocks that have less than three justifications (i.e., that do not head a 3-Chain) are not yet committed, and as such are not guaranteed
/// to remain part of the BlockTree. We call Blocks in the BlockTree that are not yet committed 'speculative' Blocks.
/// 
/// BlockTreeSnapshot's [get_from_storage](crate::block_tree::BlockTreeSnapshot::get_from_storage) method provides a non-speculative view of
/// Storage, one that does not reflect the mutations that come about from executing speculative Blocks.
/// 
/// On the other hand, SpeculativeStorageReader provides a view of Storage, that, as its name suggests, reflect the mutations that come
/// about from executing speculative Blocks, in particular, the writes of the Block-to-be-proposed's parent, grandparent, and great-grandparent.
pub struct SpeculativeStorageReader {
    parent_writes: StorageMutations,
    grandparent_writes: StorageMutations,
    great_grandparent_writes: StorageMutations,
    block_tree: BlockTreeWriter,
}

impl SpeculativeStorageReader {
    /// # Boundary scenarios
    /// 1. If parent_block_hash points to the Genesis Block, then there is no grandparent or great-grandparent and their writes are set to
    /// the empty WriteSet.
    /// 2. If parent_block_hash points to a child of the Genesis Block, then there is no great-grandparent and its writes are the set to the
    /// empty WriteSet. 
    pub(crate) fn open(block_tree: BlockTreeWriter, parent_block_hash: &BlockHash) -> SpeculativeStorageReader {
        let parent = block_tree.get_block(&parent_block_hash).unwrap();
        let parent_writes = block_tree.get_write_set(&parent_block_hash).map_or(StorageMutations::new(), identity);

        if parent.height == 0 {
            SpeculativeStorageReader {
                parent_writes,
                grandparent_writes: StorageMutations::new(),
                great_grandparent_writes: StorageMutations::new(),
                block_tree
            }
        } else if parent.height == 1 {
            let grandparent_writes = { 
                let grandparent_block_hash = block_tree.get_block(parent_block_hash).unwrap().justify.block_hash;
                block_tree.get_write_set(&grandparent_block_hash).map_or(StorageMutations::new(), identity)
            };

            SpeculativeStorageReader {
                parent_writes,
                grandparent_writes,
                great_grandparent_writes: StorageMutations::new(),
                block_tree
            }
        } else { // parent.height >= 2
            let grandparent_block_hash = block_tree.get_block(parent_block_hash).unwrap().justify.block_hash;
            let grandparent_writes = block_tree.get_write_set(&grandparent_block_hash).map_or(StorageMutations::new(), identity);
            let great_grandparent_block_hash = block_tree.get_block(&grandparent_block_hash).unwrap().justify.block_hash; 
            let great_grandparent_writes = block_tree.get_write_set(&great_grandparent_block_hash).map_or(StorageMutations::new(), identity);

        SpeculativeStorageReader {
            parent_writes,
            grandparent_writes,
            great_grandparent_writes,
            block_tree
        }
        }
    }

    /// Get a value in Storage.
    pub fn get(&self, key: &Key) -> Option<Value> {
        // Check if key was touched in parent_writes.
        if self.parent_writes.contains_delete(key) {
            return None
        } else if let Some(value) = self.parent_writes.get_insert(key) {
            return Some(value.clone())
        }

        // Check if key was touched in grandparent_writes.
        else if self.grandparent_writes.contains_delete(key) {
            return None
        } else if let Some(value) = self.grandparent_writes.get_insert(key) {
            return Some(value.clone())
        }

        // Check if key was touched in great_grandparent_writes.
        else if self.great_grandparent_writes.contains_delete(key) {
            return None
        } else if let Some(value) = self.great_grandparent_writes.get_insert(key) {
            return Some(value.clone())
        }

        // Get from storage.
        else {
            self.block_tree.get_from_storage(key)
        }
    }

}
