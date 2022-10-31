use std::sync::Arc;
use std::convert::{identity, TryFrom};
use std::cmp::min;
use log;
use borsh::{BorshDeserialize, BorshSerialize};
use rocksdb::{DB, WriteBatch, Snapshot};
use hotstuff_rs_types::base64url::Base64URL;
use hotstuff_rs_types::stored::{StorageMutations, ChildrenList, Key, Value, DataLen};
use hotstuff_rs_types::messages::{Block, BlockHash, ViewNumber, BlockHeight, Datum, Data, QuorumCertificate, AppID, DataHash};
use crate::config::BlockTreeStorageConfig;

/// Create an instance of BlockTreeWriter and BlockTreeSnapshotFactory. This function should only be called once in the process's lifetime.
pub(crate) fn open(block_tree_config: &BlockTreeStorageConfig) -> (BlockTreeWriter, BlockTreeSnapshotFactory) {
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

/// The exclusive writer into the BlockTree in a HotStuff-rs process. Only Algorithm (also a singleton) should call the writing methods of
/// this struct. It also exposes specific methods for reading from the BlockTree which are tailored to Algorithm's needs.
#[derive(Clone)]
pub(crate) struct BlockTreeWriter {
    db: Arc<DB>,
}

impl BlockTreeWriter {
    /// *Atomically* insert a Block into the BlockTree, updating 'special' keys, and applying the writes of Nodes that become committed (if any becomes
    /// committed because of the insertion) along the way. This function assumes that `block` satisfies the SafeBlock predicate.
    /// 
    /// ## Boundary scenarios
    /// If block.height < 4, this function exhibits some special behavior. In particular:
    /// 
    /// ### If block.height == 0
    /// block is the genesis Block, with no parent, so steps 4 and beyond are skipped.
    /// 
    /// ### If block.height == 1 or block.height == 2
    /// No blocks become committed by this insertion, so steps 6 and beyond are skipped.
    /// 
    /// ### If block.height == 3
    /// The block with height == 0 becomes committed by this insertion. But because block does not have a great-great-grandparent, step 7 
    /// is skipped.
    pub(crate) fn insert_block(&self, block: &Block, write_set: &StorageMutations) {
        log::info!("insert_block: height: {}, hash: {}", block.height, Base64URL::encode(block.hash).as_str());
        let mut wb = WriteBatch::default();

        // 1. Insert block to the NODES keyspace. 
        Self::set_block(&mut wb, &block);

        // 2. Insert block's write_set to the WriteSet keyspace.
        Self::set_write_set(&mut wb, &block.hash, write_set);

        // 3. Update TOP_BLOCK.
        if self.get_top_block().is_none() || block.justify.view_number > self.get_top_block().unwrap().justify.view_number {
            Self::set_top_block_hash(&mut wb, &block.hash);
        };

        if block.height >= 1 {
            // 4. Insert block to parent's ChildrenList.
            let parent_block = self.get_block(&block.justify.block_hash).unwrap();
            let mut parent_children = self
                .get_children_list(&parent_block.hash)
                .map_or(ChildrenList::new(), identity);
            parent_children.insert(block.hash);
            Self::set_children_list(&mut wb, &parent_block.hash, &parent_children);
            
            // 5. Update LOCKED_VIEW.
            // As `block` is assumed to satisfy the SafeBlock predicate, the QC it carries is guaranteed to have a view number greater
            // than or equal to locked_view.
            Self::set_locked_view(&mut wb, parent_block.justify.view_number);

            if block.height >= 3 {
                let grandparent_block = self.get_block(&parent_block.justify.block_hash).unwrap();
                let great_grandparent_block = self.get_block(&grandparent_block.justify.block_hash).unwrap();

                // 6. Check if great_grandparent.height is greater than HIGHEST_COMMITTED_NODE.height. If so, commit great_grandparent.
                // TODO: argue that no more than one Block can become committed because of a single insertion.
                if let Some(highest_committed_block) = self.get_highest_committed_block() {
                    if great_grandparent_block.height > highest_committed_block.height {
                        self.commit_block(&mut wb, &great_grandparent_block);
                    }
                } else {
                    // This is entered when block.height == 3; in this case, no block has been committed yet.
                    self.commit_block(&mut wb, &great_grandparent_block);
                }

                if block.height >= 4 {
                    let great_grandparent_block = self.get_block(&grandparent_block.justify.block_hash).unwrap(); 
                    let great_great_grandparent_block = self.get_block(&great_grandparent_block.justify.block_hash).unwrap();

                    // 7. Abandon siblings of great_grandparent_block. They will never get a chance to become committed, so removing them
                    // from the BlockTree is harmless.
                    let great_great_grandparent_children = self.get_children_list(&great_great_grandparent_block.hash).unwrap(); 
                    let siblings = great_great_grandparent_children
                        .iter()
                        .filter(|child_hash| **child_hash != great_grandparent_block.hash);
                    for sibling_hash in siblings {
                        self.delete_branch(&mut wb, &sibling_hash)
                    }
                }
            }
        }
                
        // 8. Write changes to persistent storage.
        self.db.write(wb)
            .expect("Programming or Configuration error: fail to insert Block into persistent storage.");
    }

    fn commit_block(&self, wb: &mut WriteBatch, block: &Block) {
        let writes = self.get_write_set(&block.hash).unwrap();
        Self::apply_write_set(wb, &writes);
        Self::delete_write_set(wb, &block.hash);
        Self::set_highest_committed_block(wb, &block.hash);
        Self::set_committed_block_height_to_block_hash(wb, block.height, &block.hash);
    }
}

// This impl block defines getters and setters for 'special' keys. These point to key value pairs that store data of global, cross-cutting significance.
impl BlockTreeWriter {
    /// Gets the Locked View, the highest view number in a QuorumCertificate that is contained in a 'parent' (i.e., descended-from) Block.
    pub(crate) fn get_locked_view(&self) -> Option<u64> {
        Some(u64::from_le_bytes(self.db.get(special_paths::LOCKED_VIEW).unwrap()?.try_into().unwrap()))
    }

    /// Gets the Top Block, the Block containing the QuorumCertificate with the highest View Number in the BlockTree.
    pub(crate) fn get_top_block(&self) -> Option<Block> {
        let top_block_hash = BlockHash::try_from(self.db.get(special_paths::TOP_BLOCK_HASH).unwrap()?).unwrap();
        Some(self.get_block(&top_block_hash).unwrap())
    }

    /// Gets the committed Block containing the QuorumCertificate with the highest View Number in the BlockTree.
    pub(crate) fn get_highest_committed_block(&self) -> Option<Block> {
        let highest_committed_block_hash = BlockHash::try_from(self.db.get(special_paths::HIGHEST_COMMITTED_BLOCK_HASH).unwrap()?).unwrap();
        Some(self.get_block(&highest_committed_block_hash).unwrap())
    }

    fn set_locked_view(wb: &mut WriteBatch, view_num: ViewNumber) {
        wb.put(special_paths::LOCKED_VIEW, view_num.to_le_bytes());
    }

    fn set_top_block_hash(wb: &mut WriteBatch, block_hash: &BlockHash) {
        wb.put(special_paths::TOP_BLOCK_HASH, block_hash)
    }

    fn set_highest_committed_block(wb: &mut WriteBatch, block_hash: &BlockHash) {
        wb.put(special_paths::HIGHEST_COMMITTED_BLOCK_HASH, block_hash)
    } 

    fn set_committed_block_height_to_block_hash(wb: &mut WriteBatch, block_height: BlockHeight, block_hash: &BlockHash) {
        wb.put(combine(&special_paths::COMMITTED_BLOCK_HEIGHT_TO_BLOCK_HASH, &block_height.to_le_bytes()),&block_hash);      
    }
}

// This impl block defines getters and setters for Blocks and chains of Blocks.
impl BlockTreeWriter { 
    pub(crate) fn get_block(&self, block_hash: &BlockHash) -> Option<Block> {
        let block_prefix = combine(&special_paths::BLOCK_HASH_TO_BLOCK, block_hash);

        let app_id = {
            let key = combine(&block_prefix, &special_paths::APP_ID);
            let bs = self.db.get(&key).unwrap()?;
            AppID::deserialize(&mut bs.as_slice()).unwrap()
        };

        // Safety: If app_id is Some, then it is guaranteed that all of the following keys are Some, and therefore safe to unwrap.
        let height = {
            let key = combine(&block_prefix, &special_paths::HEIGHT);
            let bs = self.db.get(&key).unwrap().unwrap();
            BlockHeight::deserialize(&mut bs.as_slice()).unwrap()
        };
        let justify = {
            let key = combine(&block_prefix, &special_paths::JUSTIFY);
            let bs = self.db.get(&key).unwrap().unwrap();
            QuorumCertificate::deserialize(&mut bs.as_slice()).unwrap()
        };
        let data_hash = {
            let key = combine(&block_prefix, &special_paths::DATA_HASH);
            let bs = self.db.get(&key).unwrap().unwrap();
            DataHash::deserialize(&mut bs.as_slice()).unwrap()
        };
        let data = {
            let block_data_prefix = combine(&block_prefix, &special_paths::DATA);
            let data_len = {
                let bs = self.db.get(&block_data_prefix).unwrap().unwrap();
                DataLen::deserialize(&mut bs.as_slice()).unwrap()
            };
            let mut data = Vec::with_capacity(data_len as usize);
            for i in 0..data_len {
                let datum = {
                    let key = combine(&block_data_prefix, &i.to_le_bytes());
                    let bs = self.db.get(&key).unwrap().unwrap();
                    bs
                };
                data.push(datum)
            };
            data
        };

        Some(Block {
            app_id,
            height,
            hash: *block_hash,
            justify,
            data_hash,
            data
        })
    }

    fn set_block(wb: &mut WriteBatch, block: &Block) {
        let block_prefix = combine(&special_paths::BLOCK_HASH_TO_BLOCK, &block.hash); 

        let app_id_key = combine(&block_prefix, &special_paths::APP_ID);
        wb.put(&app_id_key, block.app_id.try_to_vec().unwrap());

        let height_key = combine(&block_prefix, &special_paths::HEIGHT);
        wb.put(&height_key, block.height.try_to_vec().unwrap());

        let justify_key = combine(&block_prefix, &special_paths::JUSTIFY);
        wb.put(&justify_key, block.justify.try_to_vec().unwrap());

        let data_hash_key = combine(&block_prefix, &special_paths::DATA_HASH);
        wb.put(&data_hash_key, block.data_hash.try_to_vec().unwrap());

        // Insert data length.
        let data_prefix = combine(&block_prefix, &special_paths::DATA);
        wb.put(&data_prefix, block.data.len().try_to_vec().unwrap());

        // Insert datums.
        for (i, datum) in block.data.iter().enumerate() {
            let datum_key = combine(&data_prefix, &i.to_le_bytes());
            wb.put(&datum_key, datum);
        }
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
        let block_prefix = combine(&special_paths::BLOCK_HASH_TO_BLOCK, block_hash); 

        let app_id_key = combine(&block_prefix, &special_paths::APP_ID);
        wb.delete(&app_id_key);

        let height_key = combine(&block_prefix, &special_paths::HEIGHT);
        wb.delete(&height_key);

        let justify_key = combine(&block_prefix, &special_paths::JUSTIFY);
        wb.delete(&justify_key);

        let data_hash_key = combine(&block_prefix, &special_paths::DATA_HASH);
        wb.delete(&data_hash_key);
       
    }
}

// This impl block defines getters and getters for WriteSets.
impl BlockTreeWriter {
    pub(crate) fn get_write_set(&self, block_hash: &BlockHash) -> Option<StorageMutations> {
        let key = combine(&special_paths::BLOCK_HASH_TO_WRITE_SET, block_hash);
        Some(StorageMutations::deserialize(&mut self.db.get(key).unwrap()?.as_slice()).unwrap())
    }

    fn set_write_set(wb: &mut WriteBatch, block_hash: &BlockHash, write_set: &StorageMutations) {
        let key = combine(&special_paths::BLOCK_HASH_TO_WRITE_SET, block_hash);
        wb.put(key, write_set.try_to_vec().unwrap());
    } 

    fn apply_write_set(wb: &mut WriteBatch, write_set: &StorageMutations) {
        for (storage_key, value) in write_set.inserts() {
            let key = combine(&special_paths::APP_STORAGE, storage_key);
            wb.put(key, value);
        } 

        for storage_key in write_set.deletes() {
            let key = combine(&special_paths::APP_STORAGE, storage_key);
            wb.delete(key);
        }
    }

    fn delete_write_set(wb: &mut WriteBatch, block_hash: &BlockHash) {
        let key = combine(&special_paths::BLOCK_HASH_TO_WRITE_SET, block_hash);
        wb.delete(key)
    } 
}

// This impl block defines a lone method for getting a KV pair out of *persistent* (i.e., non-speculative) storage.
impl BlockTreeWriter {
    pub(crate) fn get_from_storage(&self, key: &Key) -> Option<Value> {
        let key = combine(&special_paths::APP_STORAGE, key);
        self.db.get(key).unwrap()
    }
}

// This impl block defines getters and setters for ChildrenLists.
impl BlockTreeWriter {
    fn set_children_list(wb: &mut WriteBatch, block_hash: &BlockHash, children_list: &ChildrenList) {
        let key = combine(&special_paths::BLOCK_HASH_TO_CHILDREN_LIST, block_hash);
        wb.put(key, children_list.try_to_vec().unwrap())
    }

    fn get_children_list(&self, block_hash: &BlockHash) -> Option<ChildrenList> {
        let key = combine(&special_paths::BLOCK_HASH_TO_CHILDREN_LIST, block_hash);
        let bs = self.db.get(key).unwrap()?;
        Some(ChildrenList::deserialize(&mut bs.as_slice()).unwrap())
    }

    fn delete_children_list(wb: &mut WriteBatch, block_hash: &BlockHash) {
        let key = combine(&special_paths::BLOCK_HASH_TO_CHILDREN_LIST, block_hash);
        wb.delete(key)
    }
}


/// A factory that creates consistent, immutable snapshots of the BlockTree.
#[derive(Clone)]
pub struct BlockTreeSnapshotFactory {
    db: Arc<DB>,
}

impl BlockTreeSnapshotFactory {
    pub fn snapshot(&self) -> BlockTreeSnapshot {
        BlockTreeSnapshot {
            db_snapshot: Arc::new(self.db.snapshot())
        }
    }
}

/// BlockTreeSnapshot exposes methods for reading a consistent, immutable snapshot of the BlockTree. This includes getting from committed Storage.
/// 
/// # 'get block' vs 'get committed block'
/// Because the BlockTree contains speculative Blocks, several Blocks may occupy the same height. As such, methods that get a Block (or one of its fields)
/// from the BlockTree that take in a height can only return committed Blocks. 
/// 
/// If you really need to get a speculative Block from the BlockTree from some reason, then you need to get it by its BlockHash. 
#[derive(Clone)]
pub struct BlockTreeSnapshot<'a> {
    db_snapshot: Arc<Snapshot<'a>>,
}

// Defines functions that get a Block, or one of its fields, from the BlockTree provided a **BlockHash**.
impl<'a> BlockTreeSnapshot<'a> {
    pub fn get_block_by_hash(&self, block_hash: &BlockHash) -> Option<Block> {
        let block_prefix = combine(&special_paths::BLOCK_HASH_TO_BLOCK, block_hash);

        let app_id = {
            let key = combine(&block_prefix, &special_paths::APP_ID);
            let bs = self.db_snapshot.get(&key).unwrap()?;
            AppID::deserialize(&mut bs.as_slice()).unwrap()
        };

        // Safety: If app_id is Some, then it is guaranteed that all of the following keys are Some, and therefore safe to unwrap.
        let height = {
            let key = combine(&block_prefix, &special_paths::HEIGHT);
            let bs = self.db_snapshot.get(&key).unwrap().unwrap();
            BlockHeight::deserialize(&mut bs.as_slice()).unwrap()
        };
        let justify = {
            let key = combine(&block_prefix, &special_paths::JUSTIFY);
            let bs = self.db_snapshot.get(&key).unwrap().unwrap();
            QuorumCertificate::deserialize(&mut bs.as_slice()).unwrap()
        };
        let data_hash = {
            let key = combine(&block_prefix, &special_paths::DATA_HASH);
            let bs = self.db_snapshot.get(&key).unwrap().unwrap();
            DataHash::deserialize(&mut bs.as_slice()).unwrap()
        };
        let data = {
            let block_data_prefix = combine(&block_prefix, &special_paths::DATA);
            let data_len = {
                let bs = self.db_snapshot.get(&block_data_prefix).unwrap().unwrap();
                DataLen::deserialize(&mut bs.as_slice()).unwrap()
            };
            let mut data = Vec::with_capacity(data_len as usize);
            for i in 0..data_len {
                let datum = {
                    let key = combine(&block_data_prefix, &i.to_le_bytes());
                    let bs = self.db_snapshot.get(&key).unwrap().unwrap();
                    bs
                };
                data.push(datum)
            };
            data
        };

        Some(Block {
            app_id,
            height,
            hash: *block_hash,
            justify,
            data_hash,
            data
        })
    } 

    pub fn get_block_height(&self, hash: &BlockHash) -> Option<BlockHeight> {
        let block_key = combine(&special_paths::BLOCK_HASH_TO_BLOCK, hash);
        let block_height_key = combine(&block_key, &special_paths::HEIGHT);
        let block_height = {
            let bs = self.db_snapshot.get(&block_height_key).unwrap()?;
            BlockHeight::deserialize(&mut bs.as_slice()).unwrap()
        };
        Some(block_height)
    }

    pub fn get_block_justify_by_hash(&self, hash: &BlockHash) -> Option<QuorumCertificate> {
        let block_key = combine(&special_paths::BLOCK_HASH_TO_BLOCK, hash);
        let block_justify_key = combine(&block_key, &special_paths::JUSTIFY);
        let block_justify = {
            let bs = self.db_snapshot.get(&block_justify_key).unwrap()?;
            QuorumCertificate::deserialize(&mut bs.as_slice()).unwrap()
        };
        Some(block_justify)
    } 

    pub fn get_block_data_len_by_hash(&self, hash: &BlockHash) -> Option<DataLen> {
        let block_key = combine(&special_paths::BLOCK_HASH_TO_BLOCK, hash);
        let data_len_key = combine(&block_key, &special_paths::DATA);
        let data_len = self.db_snapshot.get(&data_len_key).unwrap()?;
        Some(DataLen::deserialize(&mut data_len.as_slice()).unwrap())
    } 

    pub fn get_block_data_by_hash(&self, hash: &BlockHash) -> Option<Data> {
        let block_key = combine(&special_paths::BLOCK_HASH_TO_BLOCK, hash);
        let block_data_prefix = combine(&block_key, &special_paths::DATA);
        let block_data = {
            let data_len = {
                let bs = self.db_snapshot.get(&block_data_prefix).unwrap()?;
                DataLen::deserialize(&mut bs.as_slice()).unwrap()
            };
            let mut data = Vec::with_capacity(data_len as usize);
            for i in 0..data_len {
                let datum = {
                    let key = combine(&block_data_prefix, &i.to_le_bytes());
                    let bs = self.db_snapshot.get(&key).unwrap().unwrap();
                    bs
                };
                data.push(datum)
            };
            data
        };
        Some(block_data)
    } 

    pub fn get_block_datum_by_hash(&self, hash: &BlockHash, index: usize) -> Result<Option<Datum>, BlockNotFoundError> {
        let block_key = combine(&special_paths::BLOCK_HASH_TO_BLOCK, hash);
        let block_data_prefix = combine(&block_key, &special_paths::DATA);

        // Check if Block is actually in the BlockTree. If it is in the BlockTree, then Data's len should be Some.
        if self.db_snapshot.get(&block_data_prefix).unwrap().is_none() {
            Err(BlockNotFoundError)
        } else {
            let datum_key = combine(&block_data_prefix, &index.to_le_bytes());
            Ok(self.db_snapshot.get(datum_key).unwrap())
        }
    } 
}

// Defines functions that get a Block, or one of its fields, from the BlockTree provided a **BlockHeight**.
impl<'a> BlockTreeSnapshot<'a> {
    pub fn get_committed_block_by_height(&self, height: BlockHeight) -> Option<Block> {
        let block_hash = self.get_committed_block_hash(height)?;
        self.get_block_by_hash(&block_hash)
    }

    pub fn get_committed_block_hash(&self, height: BlockHeight) -> Option<BlockHash> {
        let block_hash_key = combine(&special_paths::COMMITTED_BLOCK_HEIGHT_TO_BLOCK_HASH, &height.to_le_bytes());
        let block_hash = {
            let bs = self.db_snapshot.get(block_hash_key).unwrap()?;
            BlockHash::deserialize(&mut bs.as_slice()).unwrap()
        };

        Some(block_hash)
    }

    pub fn get_committed_block_justify_by_height(&self, height: BlockHeight) -> Option<QuorumCertificate> {
        let block_hash = self.get_committed_block_hash(height)?;
        self.get_block_justify_by_hash(&block_hash)
    }

    pub fn get_commited_block_data_len_by_height(&self, height: BlockHeight) -> Option<DataLen> {
        let block_hash = self.get_committed_block_hash(height)?;
        self.get_block_data_len_by_hash(&block_hash)
    }

    pub fn get_committed_block_data_by_height(&self, height: BlockHeight) -> Option<Data> {
        let block_hash = self.get_committed_block_hash(height)?;
        self.get_block_data_by_hash(&block_hash)
    }

    pub fn get_committed_block_datum_by_height(&self, height: BlockHeight, index: usize) -> Result<Option<Datum>, BlockNotFoundError> {
        if let Some(block_hash) = self.get_committed_block_hash(height) {
            self.get_block_datum_by_hash(&block_hash, index)
        } else {
            Err(BlockNotFoundError)
        }
    }
}

// Defines methods that get a chain of Blocks from the BlockTree.
impl<'a> BlockTreeSnapshot<'a> {
    pub fn get_blocks_from_tail(&self, tail_block_hash: &BlockHash, limit: usize, speculate: bool) -> Option<Vec<Block>> {
        let mut res = Vec::with_capacity(limit);

        // 1. Get tail block.
        let tail_block = self.get_block_by_hash(tail_block_hash)?;
        let mut cursor = tail_block.hash;
        res.push(tail_block);

        // 2. Walk through tail block's descendants until limit is satisfied or we hit uncommitted blocks.
        while res.len() < limit {
            let child = match self.get_committed_block_child(&cursor) {
                Ok(block) => block,
                Err(ChildrenNotYetCommittedError) => break,
            };
            cursor = child.hash;
            res.push(child);
        }

        // 3. If limit is not yet satisfied and we are allowed to speculate, get speculative blocks.
        if res.len() < limit && speculate {
            if let Some(block) = self.get_top_block() {
                // We reverse (.rev) uncommitted blocks so that blocks with lower heights appear first.
                let uncommitted_blocks: Vec<Block> = self.get_chain_between_speculative_block_and_highest_committed_block(&block.hash).into_iter().rev().collect();
                res.extend_from_slice(&uncommitted_blocks[..min(limit - res.len(), uncommitted_blocks.len())]);
            }
        }

        Some(res)
    }

    pub fn get_blocks_up_to_head(&self, head_block_hash: &BlockHash, limit: usize, speculate: bool) -> Option<Vec<Block>> {
        let mut res = Vec::with_capacity(limit);

        // 1. Get head block.
        let head_block = self.get_block_by_hash(head_block_hash)?;
        let head_block_parent_hash = head_block.justify.block_hash;
        let head_block_height = head_block.height;
        res.push(head_block);
        if limit == 1 {
            // 1.1. If only one block is requested, return immediately.
            return Some(res)
        }

        // 2. Check whether head block is speculative.
        let highest_committed_block = self.get_highest_committed_block();
        if highest_committed_block.is_none() || head_block_height > highest_committed_block.as_ref().unwrap().height {
            // Start block IS speculative. 
            if !speculate {
                return None
            }

            // 2.1. Get speculative ancestors of head block.
            let speculative_ancestors: Vec<Block> = self.get_chain_between_speculative_block_and_highest_committed_block(&head_block_hash).into_iter().rev().collect();
            res.extend_from_slice(&speculative_ancestors[..min(limit - res.len(), speculative_ancestors.len())]);

            // 2.2. Get non-speculative ancestors of head block, up to the number necessary to satisfy limit.
            if let Some(block) = highest_committed_block {
                let mut cursor = block;
                while limit - res.len() > 0 {
                    let next_hash = cursor.justify.block_hash;
                    let cursor_height = cursor.height;
                    res.push(cursor);
                    if cursor_height == 0 {
                        break
                    }

                    cursor = self.get_block_by_hash(&next_hash).unwrap();
                }
            } 
        } else {
            // Start block IS NOT speculative.

            // 2.1. Get ancestors of head block, up to the number necessary to satisfy limit.
            let mut cursor = self.get_block_by_hash(&head_block_parent_hash).unwrap();
            while limit - res.len() > 0 {
                let next_hash = cursor.justify.block_hash;
                let cursor_height = cursor.height;
                res.push(cursor);
                if cursor_height == 0 {
                    break
                }

                cursor = self.get_block_by_hash(&next_hash).unwrap();
            }

        }

        Some(res)
    }

    // Unintuitive behaviors: 
    // 1. The returned chain does not include the highest_committed_block (it stops short on its child).
    // 2. The returned chain is ordered from GREATER block height to SMALLER block height.
    // 3. If highest committed block is None, returns an empty vector.
    //
    // # Panic
    // if head_block_hash is not in the BlockTree.
    fn get_chain_between_speculative_block_and_highest_committed_block(&self, head_block_hash: &BlockHash) -> Vec<Block> {
        let mut res = Vec::new();

        let head_block = self.get_block_by_hash(head_block_hash).unwrap();
        if let Some(highest_committed_block) = self.get_highest_committed_block() {
            let mut cursor = head_block.justify.block_hash;
            res.push(head_block);

            while cursor != highest_committed_block.hash {
                let block = self.get_block_by_hash(&cursor).unwrap();
                cursor = block.justify.block_hash;
                res.push(block);
            }
        };

        res
    }
}

// Defines other BlockTreeSnapshot methods.
impl<'a> BlockTreeSnapshot<'a> {
    pub fn get_committed_block_child(&self, parent_block_hash: &BlockHash) -> Result<Block, ChildrenNotYetCommittedError> {
        let highest_committed_block_hash = BlockHash::try_from(self.db_snapshot.get(special_paths::HIGHEST_COMMITTED_BLOCK_HASH).unwrap().unwrap()).unwrap();
        let highest_committed_block = self.get_block_by_hash(&highest_committed_block_hash).unwrap();
        let parent_block = self.get_block_by_hash(&parent_block_hash).unwrap();
        if parent_block.height >= highest_committed_block.height {
            return Err(ChildrenNotYetCommittedError)
        }

        let parent_children = ChildrenList::deserialize(&mut &self.db_snapshot.get(combine(&special_paths::BLOCK_HASH_TO_CHILDREN_LIST, parent_block_hash)).unwrap().unwrap()[..]).unwrap();
        // Safety: parent_children.len() must be 1, since parent is an ancestor of a committed Block. 
        let child_hash = parent_children.iter().next().unwrap();
        Ok(self.get_block_by_hash(child_hash).unwrap())
    }

    pub fn get_top_block(&self) -> Option<Block> {
        let top_block_hash = self.db_snapshot.get(special_paths::TOP_BLOCK_HASH).unwrap()?.try_into().unwrap();
        Some(self.get_block_by_hash(&top_block_hash).unwrap())
    }

    pub fn get_highest_committed_block_hash(&self) -> Option<BlockHash> {
        let highest_committed_block_hash = self.db_snapshot.get(special_paths::HIGHEST_COMMITTED_BLOCK_HASH).unwrap()?;
        Some(BlockHash::try_from(highest_committed_block_hash).unwrap())
    }

    pub fn get_highest_committed_block(&self) -> Option<Block> {
        let highest_committed_block_hash = self.get_highest_committed_block_hash()?;
        Some(self.get_block_by_hash(&highest_committed_block_hash).unwrap())
    }

    pub fn get_from_storage(&self, key: &Key) -> Option<Value> {
        self.db_snapshot.get(combine(&special_paths::APP_STORAGE, key)).unwrap()
    }
}

pub struct BlockNotFoundError;

pub struct ChildrenNotYetCommittedError;

mod special_paths {
    // Database key prefixes
    pub(super) const BLOCK_HASH_TO_BLOCK: [u8; 1] = [00];
    pub(super) const BLOCK_HASH_TO_WRITE_SET: [u8; 1] = [01];
    pub(super) const BLOCK_HASH_TO_CHILDREN_LIST: [u8; 1] = [02];
    pub(super) const COMMITTED_BLOCK_HEIGHT_TO_BLOCK_HASH: [u8; 1] = [03];
    pub(super) const APP_STORAGE: [u8; 1] = [04]; 

    // Special database keys. These should not be prefixed by anything.
    pub(super) const LOCKED_VIEW: [u8; 1] = [10];
    pub(super) const TOP_BLOCK_HASH: [u8; 1] = [11];
    pub(super) const HIGHEST_COMMITTED_BLOCK_HASH: [u8; 1] = [12];

    // Structure of the BLOCK_HASH_TO_BLOCK keyspace.
    //
    // Sometimes, users only need to get a particular field of Block. In these cases, it would be wasteful to have to get the
    // entire Block from the database. Therefore, each field of a Block is stored in separate Key-Value pairs within the
    // BLOCK_HASH_TO_BLOCK keyspace, each prefixed by the Block's Hash.
    // 
    // More precisely, the BLOCK_HASH_TO_BLOCK keyspace is structured in this manner:
    // → {block_hash}
    //   → APP_ID: block.app_id
    //   → HEIGHT: block.height
    //   → JUSTIFY: block.justify
    //   → DATA_HASH: block.data_hash
    //   → DATA: block.data.len()
    //      → 0u64: block.data[0]
    //      → 1u64: block.data[1]
    //      → ...
    //      → block.data.len(): block.data[block.data.len()]
    // 
    // block.hash is not stored explicitly, since all code that needs to get a block should already know the block's hash in
    // the first place.
    pub(super) const APP_ID: [u8; 1] = [00];
    pub(super) const HEIGHT: [u8; 1] = [01];
    pub(super) const JUSTIFY: [u8; 1] = [02];
    pub(super) const DATA_HASH: [u8; 1] = [03];
    pub(super) const DATA: [u8; 1] = [04];
}


/// Takes references to two byteslices and returns a vector containing the bytes of the first one, and then the bytes of the 
/// second one.
fn combine(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut res = Vec::with_capacity(a.len() + b.len());
    res.extend_from_slice(a);
    res.extend_from_slice(b);
    res
}
