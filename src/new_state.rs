/// Ineert a block, causing all of the necessary state changes, including possibly block commit, to happen.
/// 
/// If the insertion causes a block/blocks to be committed, returns the updates that this causes to the
/// validator set, if any.
/// 
/// # Precondition
/// [Self::safe_block]
fn insert_block(
    &mut self,
    block: &Block,
    app_state_updates: Option<&AppStateUpdates>,
    validator_set_updates: Option<&ValidatorSetUpdates>,
) -> Option<ValidatorSet> {
    let mut wb = BlockTreeWriteBatch::new();

    // Insert block. 
    wb.set_block(block);
    wb.set_newest_block(&block.hash);
    if let Some(app_state_updates) = app_state_updates {
        wb.set_pending_app_state_updates(&block.hash, app_state_updates);
    }
    if let Some(validator_set_updates) = validator_set_updates {
        wb.set_pending_validator_set_updates(&block.hash, validator_set_updates);
    }

    let mut siblings = self.children(&block.justify.block).unwrap_or(ChildrenList::new());
    siblings.push(block.hash);
    wb.set_children_list(&block.justify.block, &siblings);

    // Consider updating highest qc.
    if block.justify.view > self.highest_qc().view {
        wb.set_highest_qc(&block.justify);
    }

    // If block does not ancestors, return.
    if block.justify.is_genesis_qc() {
        self.write(wb);
        return None
    }

    // Otherwise, do things to ancestors according to whether the block contains a commit qc or a generic qc.
    let validator_set_updates = match block.justify.phase {
        Phase::Generic => {
            // Consider setting locked view to parent.justify.view.
            let parent = block.justify.block;
            let parent_justify_view = self.block_justify(&parent).unwrap().view;
            if parent_justify_view > self.locked_view() {
                wb.set_locked_view(parent_justify_view);
            }
            
            // Get great-grandparent.
            let great_grandparent = {
                let parent_justify = self.block_justify(&parent).unwrap();
                if parent_justify.is_genesis_qc() {
                    self.write(wb);
                    return None
                }

                let grandparent = parent_justify.block;
                let grandparent_justify = self.block_justify(&grandparent).unwrap();
                if grandparent_justify.is_genesis_qc() {
                    self.write(wb);
                    return None
                } 

                grandparent_justify.block
            };

            // Commit great_grandparent if not committed yet.
            self.commit_block(wb, &great_grandparent)
        },

        Phase::Commit(precommit_qc_view) => {
            // Consider setting locked view to precommit_qc_view.
            if precommit_qc_view > self.locked_view() {
                wb.set_locked_view(precommit_qc_view);
            }

            let parent = block.justify.block;

            // Commit parent if not committed yet.
            self.commit_block(wb, &parent);
        },

        _ => panic!()
    };

    self.write(wb);

    let validator_set_updates = if validator_set_updates.len() > 0 {
        validator_set_updates.last()
    } else {
        None
    };

    validator_set_updates
}

/// Commits a block and its ancestors if they have not been committed already. If the commits cause
/// updates to the validator set, this returns them in sequence.
fn commit_block(
    &mut self,
    wb: &mut BlockTreeWriteBatch<K::WriteBatch>,
    block: &CryptoHash,
) -> Vec<ValidatorSetUpdates> {
    let block_height = self.block_height(block).unwrap();
    info::committing(block, block_height);

    // Base case:
    if let Some(highest_committed_block_height) = self.highest_committed_block_height() {
        if block.height < highest_committed_block_hash {
            return None
        }
    }

    // Recursion step:
    let validator_set_updates = if !block.justify.is_genesis_qc() {
        let parent = block.justify.block;
        self.commit_block(parent);
    } else {
        Vec::new()
    };

    // Work steps:

    // Delete all of block's siblings.
    self.delete_siblings(wb, block);

    // Apply pending app state updates.
    if let Some(pending_app_state_updates) = self.pending_app_state_updates(block) {
            wb.apply_app_state_updates(&pending_app_state_updates); 
            wb.delete_pending_app_state_updates(block);
    }
        
    // Apply pending validator set updates.
    if let Some(pending_validator_set_updates) = self.pending_validator_set_updates(block) {
        debug::updating_validator_set(block);

        validator_set_updates.push(pending_validator_set_updates);

        let mut committed_validator_set = self.committed_validator_set();
        committed_validator_set.apply_updates(&pending_validator_set_updates);

        wb.set_committed_validator_set(&committed_validator_set);
        wb.delete_pending_validator_set_updates(block);
    }

    // Update the highest committed block. 
    wb.set_highest_committed_block(block);

    validator_set_updates
}


// # Precondition
// Block is in its parents' (or the genesis) children list.
//
// # Panics
// Panics if block's parent (or genesis) does not have a children list.
fn delete_siblings(wb: &mut BlockTreeWriteBatch<K::WriteBatch>, block: &CryptoHash) {
    let parent_or_beginning = block.justify.block;
    let siblings = self.children(&parent_or_beginning).unwrap().iter().filter(|sib| *sib != block);
    for sibling in siblings {
        self.delete_branch(wb)
    }

    wb.set_children_list(&block, &vec![*block]);
} 