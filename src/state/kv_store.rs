/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/
//! Defines the [KVStore] trait, which specifies the required interface for the key-value store provided 
//! by the user.
//! 
//! Given a method to obtain the value for a given key in bytes, this module also provides methods to 
//! obtain the variables stored in the key-value store, as described in [crate::state::block_tree].

use std::fmt::Display;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::hotstuff::types::QuorumCertificate;
use crate::pacemaker::types::TimeoutCertificate;
use crate::types::validators::{ValidatorSetUpdatesStatus, ValidatorSetUpdatesStatusBytes};
use crate::types::{
    basic::{AppStateUpdates, BlockHeight, ChildrenList, CryptoHash, Data, DataLen, Datum, ViewNumber}, 
    block::Block,
    validators::{ValidatorSet, ValidatorSetBytes, ValidatorSetState}
};

use super::paths;
use super::utilities::combine;
use super::write_batch::WriteBatch;

pub trait KVStore: KVGet + Clone + Send + 'static {
    type WriteBatch: WriteBatch;
    type Snapshot<'a>: 'a + KVGet;

    fn write(&mut self, wb: Self::WriteBatch);
    fn clear(&mut self);
    fn snapshot<'b>(&'b self) -> Self::Snapshot<'_>;
}

pub trait KVGet {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>>;

    /* ↓↓↓ Block ↓↓↓  */

    fn block(&self, block: &CryptoHash) -> Result<Option<Block>, KVGetError> {
        let height = self.block_height(block)?; // Safety: if block height is Some, then all of the following fields are Some too.
        if height.is_none() {
            return Ok(None)
        }
        let justify = self.block_justify(block)?;
        let data_hash = self.block_data_hash(block)?;
        let data = self.block_data(block)?;

        if data_hash.is_none() {
            return Err(KVGetError::ValueNotFound{key: Key::BlockDataHash{block: block.clone()}});
        }

        if data.is_none() {
            return Err(KVGetError::ValueNotFound{key: Key::BlockData{block: block.clone()}});
        }

        Ok(
            Some(Block {
                height: height.unwrap(),
                hash: *block,
                justify,
                data_hash: data_hash.unwrap(),
                data: data.unwrap(),
            })
        )
    }

    fn block_height(&self, block: &CryptoHash) -> Result<Option<BlockHeight>, KVGetError> {
        let block_key = combine(&paths::BLOCKS, &block.bytes());
        let block_height_key = combine(&block_key, &paths::BLOCK_HEIGHT);
        if let Some(bytes) = self.get(&block_height_key) {
            Ok(
                Some(
                    BlockHeight::deserialize(&mut bytes.as_slice())
                    .map_err(|err| KVGetError::DeserializeValueError{key: Key::BlockHeight{block: block.clone()}, source: err})?
                )
            )
        } else {
            Ok(None)
        }
    }

    fn block_justify(&self, block: &CryptoHash) -> Result<QuorumCertificate, KVGetError> {
        QuorumCertificate::deserialize(
                &mut &*self.get(&combine(&paths::BLOCKS, &combine(&block.bytes(), &paths::BLOCK_JUSTIFY)))
                .ok_or(KVGetError::ValueNotFound{key: Key::BlockJustify{block: block.clone()}})?,
        ).map_err(|err| KVGetError::DeserializeValueError{key: Key::BlockJustify{block: block.clone()}, source: err})
    }

    fn block_data_hash(&self, block: &CryptoHash) -> Result<Option<CryptoHash>, KVGetError> {
        if let Some(bytes) = self.get(&combine(&paths::BLOCKS, &combine(&block.bytes(), &paths::BLOCK_DATA_HASH))) {
            Ok(
                Some(
                    CryptoHash::deserialize(&mut &*bytes)
                    .map_err(|err| KVGetError::DeserializeValueError{key: Key::BlockDataHash{block: block.clone()}, source: err})?
                )
            )
        } else {
            Ok(None)
        }
    }

    fn block_data_len(&self, block: &CryptoHash) -> Result<Option<DataLen>, KVGetError> {
        if let Some(bytes) = self.get(&combine(&paths::BLOCKS, &combine(&block.bytes(), &paths::BLOCK_DATA_LEN))) {
            Ok(
                Some(
                    DataLen::deserialize(&mut &*bytes)
                    .map_err(|err| KVGetError::DeserializeValueError{key: Key::BlockDataLength{block: block.clone()}, source: err})?
                )
            )
        } else {
            Ok(None)
        }
    }

    fn block_data(&self, block: &CryptoHash) -> Result<Option<Data>, KVGetError> {
        let data_len = self.block_data_len(block)?;
        match data_len {
            None => Ok(None),
            Some(len) => {
                let mut data = (0..len.int())
                    .map(|i| self.block_datum(block, i));
                if let None = data.find(|datum| datum.is_none()) {
                    Ok(Some(Data::new(data.map(|datum| datum.unwrap()).collect())))
                } else {
                    Err(KVGetError::ValueNotFound{key: Key::BlockData{block: block.clone()}})
                }
            }
        }
    }

    fn block_datum(&self, block: &CryptoHash, datum_index: u32) -> Option<Datum> {
        let block_data_prefix = combine(&paths::BLOCKS, &combine(&block.bytes(), &paths::BLOCK_DATA));
        self.get(&combine(
            &block_data_prefix,
            &datum_index.try_to_vec().unwrap(),
        ))
        .map(|bytes| Datum::new(bytes))
    }

    /* ↓↓↓ Block Height to Block ↓↓↓ */

    fn block_at_height(&self, height: BlockHeight) -> Result<Option<CryptoHash>, KVGetError> {
        let block_hash_key = combine(&paths::BLOCK_AT_HEIGHT, &height.to_le_bytes());
        if let Some(bytes) = self.get(&block_hash_key) {
            Ok(
                Some(
                    CryptoHash::deserialize(&mut bytes.as_slice())
                    .map_err(|err| KVGetError::DeserializeValueError{key: Key::BlockAtHeight{height}, source: err})?
                )
            )
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Block to Children ↓↓↓ */

    fn children(&self, block: &CryptoHash) -> Result<ChildrenList, KVGetError> {
        ChildrenList::deserialize(
            &mut &* self.get(&combine(&paths::BLOCK_TO_CHILDREN, &block.bytes()))
                   .ok_or(KVGetError::ValueNotFound{key: Key::BlockChildren{block: block.clone()}})?
        ).map_err(|err| KVGetError::DeserializeValueError{key: Key::BlockChildren{block: block.clone()}, source: err})
    }

    /* ↓↓↓ Committed App State ↓↓↓ */

    fn committed_app_state(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.get(&combine(&paths::COMMITTED_APP_STATE, key))
    }

    /* ↓↓↓ Pending App State Updates ↓↓↓ */

    fn pending_app_state_updates(&self, block: &CryptoHash) -> Result<Option<AppStateUpdates>, KVGetError> {
        if let Some(bytes) = self.get(&combine(&paths::PENDING_APP_STATE_UPDATES, &block.bytes())) {
            Ok(
                Some(
                    AppStateUpdates::deserialize(&mut &*bytes)
                    .map_err(|err| KVGetError::DeserializeValueError{key: Key::PendingAppStateUpdates{block: block.clone()}, source: err})?
                )
            )
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Committed Validator Set ↓↓↓ */

    fn committed_validator_set(&self) -> Result<ValidatorSet, KVGetError> {
        let committed_validator_set_bytes = 
            ValidatorSetBytes::deserialize(
                &mut &*self.get(&paths::COMMITTED_VALIDATOR_SET)
                .ok_or(KVGetError::ValueNotFound{ key: Key::CommittedValidatorSet})?
            ).map_err(|err| KVGetError::DeserializeValueError{key: Key::CommittedValidatorSet, source: err})?;
        ValidatorSet::try_from(committed_validator_set_bytes)
            .map_err(|err| KVGetError::Ed25519DalekError{key: Key::CommittedValidatorSet, source: err})
    }

    /* ↓↓↓ Validator Set Updates Status ↓↓↓ */

    fn validator_set_updates_status(&self, block: &CryptoHash) -> Result<ValidatorSetUpdatesStatus, KVGetError> {
        let validator_set_updates_status_bytes = 
            match self.get(&combine(&paths::VALIDATOR_SET_UPDATES_STATUS, &block.bytes())) {
                None => ValidatorSetUpdatesStatusBytes::None,
                Some(bytes) => {
                    ValidatorSetUpdatesStatusBytes::deserialize(&mut &*bytes)
                    .map_err(|err| KVGetError::DeserializeValueError{key: Key::ValidatorSetUpdatesStatus{block: block.clone()}, source: err})?
                }
            };
        ValidatorSetUpdatesStatus::try_from(validator_set_updates_status_bytes)
            .map_err(|err| KVGetError::Ed25519DalekError{key: Key::ValidatorSetUpdatesStatus{block: block.clone()}, source: err})
    }

    /* ↓↓↓ Locked View ↓↓↓ */

    fn locked_qc(&self) -> Result<QuorumCertificate, KVGetError> {
        QuorumCertificate::deserialize(
            &mut &*self.get(&paths::LOCKED_QC).ok_or(KVGetError::ValueNotFound{key: Key::LockedView})?
        ).map_err(|err| KVGetError::DeserializeValueError{key: Key::LockedView, source: err})
    }

    /* ↓↓↓ Highest View Entered ↓↓↓ */

    fn highest_view_entered(&self) -> Result<ViewNumber, KVGetError> {
        ViewNumber::deserialize(
            &mut &*self.get(&paths::HIGHEST_VIEW_ENTERED).ok_or(KVGetError::ValueNotFound{key: Key::HighestViewEntered})?
        ).map_err(|err| KVGetError::DeserializeValueError{key: Key::HighestViewEntered, source: err})
    }

    /* ↓↓↓ Highest Quorum Certificate ↓↓↓ */

    fn highest_qc(&self) -> Result<QuorumCertificate, KVGetError> {
        QuorumCertificate::deserialize(
            &mut &*self.get(&paths::HIGHEST_QC).ok_or(KVGetError::ValueNotFound{key: Key::HighestQC})?
        ).map_err(|err| KVGetError::DeserializeValueError{key: Key::HighestQC, source: err})
    }

    /* ↓↓↓ Highest Committed Block ↓↓↓ */

    fn highest_committed_block(&self) -> Result<Option<CryptoHash>, KVGetError> {
        if let Some(bytes) = self.get(&paths::HIGHEST_COMMITTED_BLOCK) {
            let highest_committed_block = CryptoHash::deserialize(&mut &*bytes)
                                          .map_err(|err| KVGetError::DeserializeValueError{key: Key::HighestCommittedBlock, source: err})?;
            Ok(Some(highest_committed_block))
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Newest Block ↓↓↓ */

    fn newest_block(&self) -> Result<Option<CryptoHash>, KVGetError> {
        if let Some(bytes) = self.get(&paths::NEWEST_BLOCK) {
            let newest_block = CryptoHash::deserialize(&mut &*bytes)
                               .map_err(|err| KVGetError::DeserializeValueError{key: Key::NewestBlock, source: err})?;
            Ok(Some(newest_block))
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Highest Timeout Certificate ↓↓↓ */

    fn highest_tc(&self) -> Result<Option<TimeoutCertificate>, KVGetError> {
        if let Some(bytes) = self.get(&paths::HIGHEST_TC) {
            let tc = TimeoutCertificate::deserialize(&mut &*bytes)
                    .map_err(|err| KVGetError::DeserializeValueError{key: Key::HighestTC, source: err})?;
            Ok(Some(tc))
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Previous Validator Set ↓↓↓ */

    fn previous_validator_set(&self) -> Result<ValidatorSet, KVGetError> {
        let previous_validator_set_bytes = 
            ValidatorSetBytes::deserialize(
                &mut &*self.get(&paths::PREVIOUS_VALIDATOR_SET)
                .ok_or(KVGetError::ValueNotFound{key: Key::CommittedValidatorSet})?
            ).map_err(|err| KVGetError::DeserializeValueError{key: Key::CommittedValidatorSet, source: err})?;
        ValidatorSet::try_from(previous_validator_set_bytes)
            .map_err(|err| KVGetError::Ed25519DalekError{key: Key::CommittedValidatorSet, source: err})
    }

    /* ↓↓↓ Validator Set Update Block Height ↓↓↓ */

    fn validator_set_update_block_height(&self) -> Result<Option<BlockHeight>, KVGetError> {
        if let Some(bytes) = self.get(&paths::VALIDATOR_SET_UPDATE_BLOCK_HEIGHT) {
            let block_height = BlockHeight::deserialize(&mut &*bytes)
                .map_err(|err| KVGetError::DeserializeValueError{key: Key::ValidatorSetUpdateHeight, source: err})?;
            Ok(Some(block_height))
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Validator Set Update Complete ↓↓↓ */
    fn validator_set_update_complete(&self) -> Result<bool, KVGetError> {
        bool::deserialize(
            &mut &*self.get(&paths::VALIDATOR_SET_UPDATE_COMPLETED)
            .ok_or(KVGetError::ValueNotFound{key: Key::ValidatorSetUpdateComplete})?
        ).map_err(|err| KVGetError::DeserializeValueError{key: Key::ValidatorSetUpdateComplete, source: err})
    }

    /* ↓↓↓ Validator Set State ↓↓↓ */

    fn validator_set_state(&self) -> Result<ValidatorSetState, KVGetError> {
        Ok(
            ValidatorSetState::new(
                self.committed_validator_set()?,
                self.previous_validator_set()?, 
                self.validator_set_update_block_height()?,
                self.validator_set_update_complete()?
            )
        )
    }

    /* ↓↓↓ Highest View Voted ↓↓↓ */
    
    fn highest_view_voted(&self) -> Result<Option<ViewNumber>, KVGetError> {
        if let Some(bytes) = self.get(&paths::HIGHEST_VIEW_VOTED) {
            let view_number = ViewNumber::deserialize(&mut &*bytes)
            .map_err(|err| KVGetError::DeserializeValueError{key: Key::HighestViewVoted, source: err})?;
            Ok(Some(view_number))
        } else {
            Ok(None)
        }
    }

}

/// Error when trying to read a value corresponding to a given key from the [key value store][KVStore].
/// The error may arise in the following circumstances:
/// 1. The value corresponding to a given key cannot be deserialized into its expected type,
/// 2. The value corresponding to a given key cannot be found,
/// 3. There was an error when processing Ed25519Dalek keypairs or signatures.
#[derive(Debug)]
pub enum KVGetError {
    DeserializeValueError{key: Key, source: std::io::Error},
    ValueNotFound{key: Key},
    Ed25519DalekError{key: Key, source: ed25519_dalek::SignatureError},
}

#[derive(Debug)]
pub enum Key {
    BlockHeight{block: CryptoHash},
    BlockJustify{block: CryptoHash},
    BlockDataHash{block: CryptoHash},
    BlockDataLength{block: CryptoHash},
    BlockData{block: CryptoHash},
    BlockAtHeight{height: BlockHeight},
    BlockChildren{block: CryptoHash},
    CommittedAppState{key: Vec<u8>},
    PendingAppStateUpdates{block: CryptoHash},
    CommittedValidatorSet,
    ValidatorSetUpdatesStatus{block: CryptoHash},
    LockedView,
    HighestViewEntered,
    HighestQC,
    HighestCommittedBlock,
    NewestBlock,
    HighestTC,
    PreviousValidatorSet,
    ValidatorSetUpdateHeight,
    ValidatorSetUpdateComplete,
    HighestViewVoted,
}

impl Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            &Key::BlockHeight{block} => 
                write!(f, "Block Height for block {}", block),
            &Key::BlockJustify{block} => 
                write!(f, "Block Justify for block {}", block),
            &Key::BlockDataHash{block } => 
                write!(f, "Block Data Hash for block {}", block),
            &Key::BlockDataLength{block} =>
                write!(f, "Block Data length for block {}", block),
            &Key::BlockData{block } => 
                write!(f, "Block Data for block {}", block),
            &Key::BlockAtHeight{height} =>
                write!(f, "Block at height {}", height.int()),
            &Key::BlockChildren{block} => 
                write!(f, "Block children for block {}", block),
            &Key::CommittedAppState{key} =>
                write!(f, "Committed App State for key {:#?}", key),
            &Key::PendingAppStateUpdates{block} => 
                write!(f, "Pending App State Updates for block {}", block),
            &Key::CommittedValidatorSet =>
                write!(f, "Committed Validator Set"),
            &Key::ValidatorSetUpdatesStatus{block} => 
                write!(f, "Validator Set Updates Status for block {}", block),
            &Key::LockedView => 
                write!(f, "Locked View"),
            &Key::HighestViewEntered =>
                write!(f, "Highest View Entered"),
            &Key::HighestQC =>
                write!(f, "Highest Quorum Certificate"),
            &Key::HighestCommittedBlock =>
                write!(f, "Highest Committed Block"),
            &Key::NewestBlock =>
                write!(f, "Newest Block"),
            &Key::HighestTC => 
                write!(f, "Highest Timeout Certificate"),
            &Key::PreviousValidatorSet =>
                write!(f, "Previous Validator Set"),
            &Key::ValidatorSetUpdateHeight =>
                write!(f, "Validator Set Update Block Height"),
            &Key::ValidatorSetUpdateComplete =>
                write!(f, "Validator Set Update Complete"),
            &Key::HighestViewVoted => 
                write!(f, "Highest View Voted"),
        }
    }
}
