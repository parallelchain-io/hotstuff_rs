use std::convert::identity;
use std::ops::Deref;
use std::time::Instant;
use crate::block_tree::BlockTreeWriter;
use crate::stored_types::{WriteSet, Key, Value};
use crate::msg_types::{Data, DataHash, Block as MsgBlock, BlockHash};

/// Besides implementing the functions specified in the trait, implementors of App are additionally expected to be *deterministic*. i.e., every
/// function it implements as part of the App trait should evaluate to the same value every time it is called with the same arguments.
pub trait App: Send + 'static {
    /// Called by StateMachine when this Participant becomes the Leader and has to propose a new Block which extends a branch of the BlockTree.
    /// 
    /// # Parameters
    /// 1. `parent_block`: the Block which the new Block directly descends from.
    /// 2. `storage_snapshot`: a read-and-writable view of Storage after executing all Blocks in the branch headed by `parent_block`.
    /// 3. `deadline`: this function call should return at the latest by this instant in time. Otherwise, this view in which the Participant
    /// is the Leader is likely to fail because of view timeout.
    /// 
    /// # Return value
    /// A triple consisting of:
    /// 1. A Data. This will occupy the `data` field of the Block proposed by this Participant in this view. 
    /// 2. A DataHash. This will occupy the `data_hash` field of the Block proposed by the Participant in this view.
    /// 3. A WriteSet resulting from executing the provided Block on the provided Storage. This will be applied into the Storage when the Block
    /// containing the returned Data becomes committed. 
    fn propose_block(
        &mut self, 
        parent_block: &Block,
        storage_snapshot: SpeculativeStorageSnapshot,
        deadline: Instant
    ) -> (Data, DataHash, WriteSet);

    /// Called by StateMachine when this Participant is a Replica and has to decide whether or not to vote on a Block which was proposed by
    /// the Leader.
    /// 
    /// # Parameters
    /// 1. `block`: the Block which was proposed by the Leader of this view.
    /// 2. `storage_snapshot`: read the corresponding entry in the itemdoc for `propose_block`.
    /// 3. `deadline`: this function call should return at the latest by this instant in time. If not, this Participant might not be able to 
    /// Vote in time for its signature to be included in this round's QuorumCertificate. 
    /// 
    /// # Return value
    /// If the Block is valid, the WriteSet that results from executing the provided Block on the provided Storage. Else, the reason why the
    /// Block is deemed invalid.
    fn validate_block(
        &mut self,
        block: &Block,
        storage_snapshot: SpeculativeStorageSnapshot,
        deadline: Instant
    ) -> Result<WriteSet, ExecuteError>;
}

/// Circumstances in which an App could reject a proposed Block, causing this Participant to skip this round without voting.
pub enum ExecuteError {
    /// The Deadline was exceeded while processing the proposed Block.
    RanOutOfTime,

    /// The contents of the Block, in the context of its proposed position in the Block Tree, is invalid in the view of App-level validation rules.
    InvalidBlock,
}

/// A convenience wrapper around msg_types::Block which, besides allowing App's methods to look into the Block's fields, exposes methods for
/// traversing the branch that this Block heads.
pub struct Block<'a> {
    inner: MsgBlock,
    block_tree: &'a BlockTreeWriter,
}

impl<'a> Block<'a> {
    pub(crate) fn new(block: MsgBlock, block_tree: &BlockTreeWriter) -> Block {
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

impl<'a> Deref for Block<'a> {
    type Target = MsgBlock;

    fn deref(&self) -> &Self::Target {
        &self.inner 
    }
}

/// A read-and-writable view of the World State after executing all the Blocks in some branch of the Block Tree. The writes (`set`s)
/// applied into this WorldStateHandle only become permanent when the Block containing the Command that it corresponds to becomes committed. 
/// 
/// This structure should NOT be used in App code outside the context of `create_leaf` and `execute`.
pub struct SpeculativeStorageSnapshot<'a> {
    parent_writes: WriteSet,
    grandparent_writes: WriteSet,
    block_tree: &'a BlockTreeWriter,
}

impl<'a> SpeculativeStorageSnapshot<'a> {
    pub(crate) fn open(block_tree: &'a BlockTreeWriter, parent_block_hash: &BlockHash) -> SpeculativeStorageSnapshot<'a> {
        let parent_writes = block_tree.get_write_set(&parent_block_hash).map_or(WriteSet::new(), identity);
        let grandparent_writes = {
            let grandparent_block_hash = block_tree.get_block(parent_block_hash).unwrap().justify.block_hash;
            block_tree.get_write_set(&grandparent_block_hash).map_or(WriteSet::new(), identity)
        };

        SpeculativeStorageSnapshot {
            parent_writes,
            grandparent_writes,
            block_tree
        }
    }

    /// get a value in the World State.
    pub fn get(&self, key: &Key) -> Value {
        if let Some(value) = self.parent_writes.get(key) {
            value.clone()
        } else if let Some(value) = self.grandparent_writes.get(key) {
            value.clone()
        } else {
            self.block_tree.get_from_storage(key)
        }
    }

}
