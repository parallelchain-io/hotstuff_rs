/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/
pub struct BlockTreeWriteBatch<W: WriteBatch>(pub(super) W);

use borsh::BorshSerialize;
use paths::*;

use crate::hotstuff::types::QuorumCertificate;
use crate::pacemaker::types::TimeoutCertificate;
use crate::types::basic::{AppStateUpdates, BlockHeight, ChildrenList, CryptoHash, DataLen, ViewNumber};
use crate::types::block::Block;
use crate::types::validators::{ValidatorSet, ValidatorSetBytes, ValidatorSetUpdates, ValidatorSetUpdatesBytes, ValidatorSetUpdatesStatus, ValidatorSetUpdatesStatusBytes};

use super::block_tree::BlockTreeError;
use super::kv_store::Key;
use super::paths;
use super::utilities::combine;

pub trait WriteBatch {
    fn new() -> Self;
    fn set(&mut self, key: &[u8], value: &[u8]);
    fn delete(&mut self, key: &[u8]);
}

impl<W: WriteBatch> BlockTreeWriteBatch<W> {
    pub(crate) fn new() -> BlockTreeWriteBatch<W> {
        BlockTreeWriteBatch(W::new())
    }

    pub fn new_unsafe() -> BlockTreeWriteBatch<W> {
        Self::new()
    }

    /* ↓↓↓ Block ↓↓↓  */

    pub fn set_block(&mut self, block: &Block) -> Result<(), BlockTreeError> {
        let block_prefix = combine(&paths::BLOCKS, &block.hash.bytes());

        self.0.set(
            &combine(&block_prefix, &paths::BLOCK_HEIGHT),
            &block.height.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::BlockHeight{block: block.hash.clone()}, source: err})?,
        );
        self.0.set(
            &combine(&block_prefix, &paths::BLOCK_JUSTIFY),
            &block.justify.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::BlockJustify{block: block.hash.clone()}, source: err})?,
        );
        self.0.set(
            &combine(&block_prefix, &paths::BLOCK_DATA_HASH),
            &block.data_hash.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::BlockDataHash{block: block.hash.clone()}, source: err})?,
        );
        self.0.set(
            &combine(&block_prefix, &paths::BLOCK_DATA_LEN),
            &block.data.len().try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::BlockDataLength{block: block.hash.clone()}, source: err})?,
        );

        // Insert datums.
        let block_data_prefix = combine(&block_prefix, &BLOCK_DATA);
        for (i, datum) in block.data.iter().enumerate() {
            let datum_key = combine(&block_data_prefix, &(i as u32).try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError{key: Key::BlockData{block: block.hash.clone()}, source: err})?);
            self.0.set(&datum_key, datum.bytes());
        };

        Ok(())
    }

    pub fn delete_block(&mut self, block: &CryptoHash, data_len: DataLen) {
        let block_prefix = combine(&BLOCKS, &block.bytes());

        self.0.delete(&combine(&block_prefix, &BLOCK_HEIGHT));
        self.0.delete(&combine(&block_prefix, &BLOCK_JUSTIFY));
        self.0.delete(&combine(&block_prefix, &BLOCK_DATA_HASH));
        self.0.delete(&combine(&block_prefix, &BLOCK_DATA_LEN));

        let block_data_prefix = combine(&block_prefix, &BLOCK_DATA);
        for i in 0..data_len.int() {
            let datum_key = combine(&block_data_prefix, &i.try_to_vec().unwrap());
            self.0.delete(&datum_key);
        }
    }

    /* ↓↓↓ Block at Height ↓↓↓ */

    pub fn set_block_at_height(&mut self, height: BlockHeight, block: &CryptoHash) -> Result<(), BlockTreeError> {
        Ok(
            self.0.set(
                &combine(&BLOCK_AT_HEIGHT, &height.try_to_vec().unwrap()),
                &block.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::BlockAtHeight{height}, source: err})?,
            )
        )
    }

    /* ↓↓↓ Block to Children ↓↓↓ */

    pub fn set_children(&mut self, block: &CryptoHash, children: &ChildrenList) -> Result<(), BlockTreeError> {
        Ok(
            self.0.set(
            &combine(&BLOCK_TO_CHILDREN, &block.bytes()),
            &children.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::BlockChildren{block: block.clone()}, source: err})?,
        ))
    }

    pub fn delete_children(&mut self, block: &CryptoHash) {
        self.0.delete(&combine(&paths::BLOCK_TO_CHILDREN, &block.bytes()));
    }

    /* ↓↓↓ Committed App State ↓↓↓ */

    pub fn set_committed_app_state(&mut self, key: &[u8], value: &[u8]) {
        self.0.set(&combine(&paths::COMMITTED_APP_STATE, key), value);
    }

    pub fn delete_committed_app_state(&mut self, key: &[u8]) {
        self.0.delete(&combine(&paths::COMMITTED_APP_STATE, key));
    }

    /* ↓↓↓ Pending App State Updates ↓↓↓ */

    pub fn set_pending_app_state_updates(
        &mut self,
        block: &CryptoHash,
        app_state_updates: &AppStateUpdates,
    ) -> Result<(), KVSetError>
    {   
        Ok(
            self.0.set(
            &combine(&paths::PENDING_APP_STATE_UPDATES, &block.bytes()),
            &app_state_updates.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::PendingAppStateUpdates{block: block.clone()}, source: err})?,
            )
        )
    }

    pub fn apply_app_state_updates(&mut self, app_state_updates: &AppStateUpdates) {
        for (key, value) in app_state_updates.inserts() {
            self.set_committed_app_state(key, value);
        }

        for key in app_state_updates.deletions() {
            self.delete_committed_app_state(key);
        }
    }

    pub fn delete_pending_app_state_updates(&mut self, block: &CryptoHash) {
        self.0.delete(&combine(&paths::PENDING_APP_STATE_UPDATES, &block.bytes()));
    }

    /* ↓↓↓ Commmitted Validator Set */

    pub fn set_committed_validator_set(&mut self, validator_set: &ValidatorSet) -> Result<(), BlockTreeError> {
        let validator_set_bytes: ValidatorSetBytes = validator_set.into();
        Ok(
            self.0.set(
            &paths::COMMITTED_VALIDATOR_SET,
            &validator_set_bytes.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::CommittedValidatorSet, source: err})?,
            )
        )
    }

    /* ↓↓↓ Pending Validator Set Updates */

    pub fn set_pending_validator_set_updates(
        &mut self,
        block: &CryptoHash,
        validator_set_updates: &ValidatorSetUpdates,
    ) -> Result<(), BlockTreeError>
    {
        let block_vs_updates_bytes = ValidatorSetUpdatesStatusBytes::Pending(validator_set_updates.into());
        Ok(
            self.0.set(
                &combine(&paths::BLOCK_VALIDATOR_SET_UPDATES, &block.bytes()),
                &block_vs_updates_bytes.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::ValidatorSetUpdatesStatus{block: block.clone()}, source: err})?,
            )
        )
    }

    pub fn set_committed_validator_set_updates(&mut self, block: &CryptoHash) -> Result<(), BlockTreeError> 
    {
        let block_vs_updates_bytes = ValidatorSetUpdatesStatusBytes::Committed;
        Ok(
            self.0.set(
                &combine(&paths::BLOCK_VALIDATOR_SET_UPDATES, &block.bytes()),
                &block_vs_updates_bytes.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::ValidatorSetUpdatesStatus{block: block.clone()}, source: err})?,
            )
        )
    }

    pub fn delete_block_validator_set_updates(&mut self, block: &CryptoHash) {
        self.0
            .delete(&combine(&paths::BLOCK_VALIDATOR_SET_UPDATES, &block.bytes()))
    }

    /* ↓↓↓ Locked View ↓↓↓ */

    pub fn set_locked_qc(&mut self, qc: &QuorumCertificate) -> Result<(), BlockTreeError> {
        Ok(self.0.set(&paths::LOCKED_QC, &qc.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::LockedView, source: err})?))
    }

    /* ↓↓↓ Highest View Entered ↓↓↓ */

    pub fn set_highest_view_entered(&mut self, view: ViewNumber) -> Result<(), BlockTreeError>{
        Ok(
            self.0
            .set(&paths::HIGHEST_VIEW_ENTERED, &view.try_to_vec()
            .map_err(|err| KVSetError::SerializeValueError{key: Key::HighestTC, source: err})?)
        )
    }

    /* ↓↓↓ Highest Quorum Certificate ↓↓↓ */

    pub fn set_highest_qc(&mut self, qc: &QuorumCertificate) -> Result<(), BlockTreeError>{
        Ok(self.0.set(&paths::HIGHEST_QC, &qc.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::HighestTC, source: err})?))
    }

    /* ↓↓↓ Highest Committed Block ↓↓↓ */

    pub fn set_highest_committed_block(&mut self, block: &CryptoHash) -> Result<(), BlockTreeError> {
        Ok(
            self.0
            .set(&paths::HIGHEST_COMMITTED_BLOCK, &block.try_to_vec()
            .map_err(|err| KVSetError::SerializeValueError{key: Key::HighestCommittedBlock, source: err})?)
        )
    }

    /* ↓↓↓ Newest Block ↓↓↓ */

    pub fn set_newest_block(&mut self, block: &CryptoHash) -> Result<(), BlockTreeError> {
        Ok(self.0.set(&paths::NEWEST_BLOCK, &block.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::NewestBlock, source: err})?))
    }

    /* ↓↓↓ Highest Timeout Certificate ↓↓↓ */

    pub fn set_highest_tc(&mut self, tc: &TimeoutCertificate) -> Result<(), BlockTreeError> {
        Ok(self.0.set(&paths::HIGHEST_TC, &tc.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::HighestTC, source: err})?))
    }

    /* ↓↓↓ Previous Validator Set  ↓↓↓ */
    pub fn set_previous_validator_set(&mut self, validator_set: &ValidatorSet) -> Result<(), BlockTreeError> {
        let validator_set_bytes: ValidatorSetBytes = validator_set.into();
        Ok(
            self.0.set(
            &paths::PREVIOUS_VALIDATOR_SET,
            &validator_set_bytes.try_to_vec().map_err(|err| KVSetError::SerializeValueError{key: Key::PreviousValidatorSet, source: err})?,
            )
        )
    }

    /* ↓↓↓ Validator Set Update Block Height ↓↓↓ */
    pub fn set_validator_set_update_block_height(&mut self, height: BlockHeight) -> Result<(), BlockTreeError> {
        Ok(
            self.0.set(
                &paths::VALIDATOR_SET_UPDATE_BLOCK_HEIGHT, 
                &height.try_to_vec()
                       .map_err(|err| KVSetError::SerializeValueError{key: Key::ValidatorSetUpdateHeight, source: err})?
            )
        )
    }

    /* ↓↓↓ Validator Set Update Complete ↓↓↓ */

    pub fn set_validator_set_update_complete(&mut self, update_complete: bool) -> Result<(), BlockTreeError> {
        Ok(
            self.0.set(
                &paths::VALIDATOR_SET_UPDATE_COMPLETE, 
                &update_complete.try_to_vec()
                       .map_err(|err| KVSetError::SerializeValueError{key: Key::ValidatorSetUpdateComplete, source: err})?
            )
        )
    }

    /* ↓↓↓ Highest View Voted ↓↓↓ */

    pub fn set_highest_view_voted(&mut self, view: ViewNumber) -> Result<(), BlockTreeError> {
        Ok(
            self.0.set(
                &paths::HIGHEST_VIEW_VOTED,
                &view.try_to_vec()
                       .map_err(|err| KVSetError::SerializeValueError{key: Key::HighestViewVoted, source: err})?
            )
        )
    }
}

#[derive(Debug)]
pub enum KVSetError {
    SerializeValueError{key: Key, source: std::io::Error}
}
