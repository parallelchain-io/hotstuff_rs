//! Traits for pluggable Block Tree persistence.

use std::fmt::Display;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    hotstuff::types::PhaseCertificate,
    pacemaker::types::TimeoutCertificate,
    types::{
        block::Block,
        data_types::{BlockHeight, ChildrenList, CryptoHash, Data, DataLen, Datum, ViewNumber},
        update_sets::AppStateUpdates,
        validator_set::{
            ValidatorSet, ValidatorSetBytes, ValidatorSetState, ValidatorSetUpdatesStatus,
            ValidatorSetUpdatesStatusBytes,
        },
    },
};

use super::variables::{self, concat};

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
            return Ok(None);
        }
        let justify = self.block_justify(block)?;
        let data_hash = self.block_data_hash(block)?;
        let data = self.block_data(block)?;

        if data_hash.is_none() {
            return Err(KVGetError::ValueExpectedButNotFound {
                key: Key::BlockDataHash {
                    block: block.clone(),
                },
            });
        }

        if data.is_none() {
            return Err(KVGetError::ValueExpectedButNotFound {
                key: Key::BlockData {
                    block: block.clone(),
                },
            });
        }

        Ok(Some(Block {
            height: height.unwrap(),
            hash: *block,
            justify,
            data_hash: data_hash.unwrap(),
            data: data.unwrap(),
        }))
    }

    fn block_height(&self, block: &CryptoHash) -> Result<Option<BlockHeight>, KVGetError> {
        let block_key = concat(&variables::BLOCKS, &block.bytes());
        let block_height_key = concat(&block_key, &variables::BLOCK_HEIGHT);
        if let Some(bytes) = self.get(&block_height_key) {
            Ok(Some(
                BlockHeight::deserialize(&mut bytes.as_slice()).map_err(|err| {
                    KVGetError::DeserializeValueError {
                        key: Key::BlockHeight {
                            block: block.clone(),
                        },
                        source: err,
                    }
                })?,
            ))
        } else {
            Ok(None)
        }
    }

    fn block_justify(&self, block: &CryptoHash) -> Result<PhaseCertificate, KVGetError> {
        PhaseCertificate::deserialize(
            &mut &*self
                .get(&concat(
                    &variables::BLOCKS,
                    &concat(&block.bytes(), &variables::BLOCK_JUSTIFY),
                ))
                .ok_or(KVGetError::ValueExpectedButNotFound {
                    key: Key::BlockJustify {
                        block: block.clone(),
                    },
                })?,
        )
        .map_err(|err| KVGetError::DeserializeValueError {
            key: Key::BlockJustify {
                block: block.clone(),
            },
            source: err,
        })
    }

    fn block_data_hash(&self, block: &CryptoHash) -> Result<Option<CryptoHash>, KVGetError> {
        if let Some(bytes) = self.get(&concat(
            &variables::BLOCKS,
            &concat(&block.bytes(), &variables::BLOCK_DATA_HASH),
        )) {
            Ok(Some(CryptoHash::deserialize(&mut &*bytes).map_err(
                |err| KVGetError::DeserializeValueError {
                    key: Key::BlockDataHash {
                        block: block.clone(),
                    },
                    source: err,
                },
            )?))
        } else {
            Ok(None)
        }
    }

    fn block_data_len(&self, block: &CryptoHash) -> Result<Option<DataLen>, KVGetError> {
        if let Some(bytes) = self.get(&concat(
            &variables::BLOCKS,
            &concat(&block.bytes(), &variables::BLOCK_DATA_LEN),
        )) {
            Ok(Some(DataLen::deserialize(&mut &*bytes).map_err(|err| {
                KVGetError::DeserializeValueError {
                    key: Key::BlockDataLength {
                        block: block.clone(),
                    },
                    source: err,
                }
            })?))
        } else {
            Ok(None)
        }
    }

    fn block_data(&self, block: &CryptoHash) -> Result<Option<Data>, KVGetError> {
        let data_len = self.block_data_len(block)?;
        match data_len {
            None => Ok(None),
            Some(len) => {
                let mut data = (0..len.int()).map(|i| self.block_datum(block, i));
                if let None = data.find(|datum| datum.is_none()) {
                    Ok(Some(Data::new(data.map(|datum| datum.unwrap()).collect())))
                } else {
                    Err(KVGetError::ValueExpectedButNotFound {
                        key: Key::BlockData {
                            block: block.clone(),
                        },
                    })
                }
            }
        }
    }

    fn block_datum(&self, block: &CryptoHash, datum_index: u32) -> Option<Datum> {
        let block_data_prefix = concat(
            &variables::BLOCKS,
            &concat(&block.bytes(), &variables::BLOCK_DATA),
        );
        self.get(&concat(
            &block_data_prefix,
            &datum_index.try_to_vec().unwrap(),
        ))
        .map(|bytes| Datum::new(bytes))
    }

    /* ↓↓↓ Block Height to Block ↓↓↓ */

    fn block_at_height(&self, height: BlockHeight) -> Result<Option<CryptoHash>, KVGetError> {
        let block_hash_key = concat(&variables::BLOCK_AT_HEIGHT, &height.to_le_bytes());
        if let Some(bytes) = self.get(&block_hash_key) {
            Ok(Some(
                CryptoHash::deserialize(&mut bytes.as_slice()).map_err(|err| {
                    KVGetError::DeserializeValueError {
                        key: Key::BlockAtHeight { height },
                        source: err,
                    }
                })?,
            ))
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Block to Children ↓↓↓ */

    fn children(&self, block: &CryptoHash) -> Result<ChildrenList, KVGetError> {
        ChildrenList::deserialize(
            &mut &*self
                .get(&concat(&variables::BLOCK_TO_CHILDREN, &block.bytes()))
                .ok_or(KVGetError::ValueExpectedButNotFound {
                    key: Key::BlockChildren {
                        block: block.clone(),
                    },
                })?,
        )
        .map_err(|err| KVGetError::DeserializeValueError {
            key: Key::BlockChildren {
                block: block.clone(),
            },
            source: err,
        })
    }

    /* ↓↓↓ Committed App State ↓↓↓ */

    fn committed_app_state(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.get(&concat(&variables::COMMITTED_APP_STATE, key))
    }

    /* ↓↓↓ Pending App State Updates ↓↓↓ */

    fn pending_app_state_updates(
        &self,
        block: &CryptoHash,
    ) -> Result<Option<AppStateUpdates>, KVGetError> {
        if let Some(bytes) = self.get(&concat(
            &variables::PENDING_APP_STATE_UPDATES,
            &block.bytes(),
        )) {
            Ok(Some(AppStateUpdates::deserialize(&mut &*bytes).map_err(
                |err| KVGetError::DeserializeValueError {
                    key: Key::PendingAppStateUpdates {
                        block: block.clone(),
                    },
                    source: err,
                },
            )?))
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Committed Validator Set ↓↓↓ */

    fn committed_validator_set(&self) -> Result<ValidatorSet, KVGetError> {
        let committed_validator_set_bytes = ValidatorSetBytes::deserialize(
            &mut &*self.get(&variables::COMMITTED_VALIDATOR_SET).ok_or(
                KVGetError::ValueExpectedButNotFound {
                    key: Key::CommittedValidatorSet,
                },
            )?,
        )
        .map_err(|err| KVGetError::DeserializeValueError {
            key: Key::CommittedValidatorSet,
            source: err,
        })?;
        ValidatorSet::try_from(committed_validator_set_bytes).map_err(|err| {
            KVGetError::Ed25519DalekError {
                key: Key::CommittedValidatorSet,
                source: err,
            }
        })
    }

    /* ↓↓↓ Validator Set Updates Status ↓↓↓ */

    fn validator_set_updates_status(
        &self,
        block: &CryptoHash,
    ) -> Result<ValidatorSetUpdatesStatus, KVGetError> {
        let validator_set_updates_status_bytes = match self.get(&concat(
            &variables::VALIDATOR_SET_UPDATES_STATUS,
            &block.bytes(),
        )) {
            None => ValidatorSetUpdatesStatusBytes::None,
            Some(bytes) => {
                ValidatorSetUpdatesStatusBytes::deserialize(&mut &*bytes).map_err(|err| {
                    KVGetError::DeserializeValueError {
                        key: Key::ValidatorSetUpdatesStatus {
                            block: block.clone(),
                        },
                        source: err,
                    }
                })?
            }
        };
        ValidatorSetUpdatesStatus::try_from(validator_set_updates_status_bytes).map_err(|err| {
            KVGetError::Ed25519DalekError {
                key: Key::ValidatorSetUpdatesStatus {
                    block: block.clone(),
                },
                source: err,
            }
        })
    }

    /* ↓↓↓ Locked PC ↓↓↓ */

    fn locked_pc(&self) -> Result<PhaseCertificate, KVGetError> {
        PhaseCertificate::deserialize(
            &mut &*self
                .get(&variables::LOCKED_PC)
                .ok_or(KVGetError::ValueExpectedButNotFound { key: Key::LockedPC })?,
        )
        .map_err(|err| KVGetError::DeserializeValueError {
            key: Key::LockedPC,
            source: err,
        })
    }

    /* ↓↓↓ Highest View Entered ↓↓↓ */

    fn highest_view_entered(&self) -> Result<ViewNumber, KVGetError> {
        ViewNumber::deserialize(&mut &*self.get(&variables::HIGHEST_VIEW_ENTERED).ok_or(
            KVGetError::ValueExpectedButNotFound {
                key: Key::HighestViewEntered,
            },
        )?)
        .map_err(|err| KVGetError::DeserializeValueError {
            key: Key::HighestViewEntered,
            source: err,
        })
    }

    /* ↓↓↓ Highest Phase Certificate ↓↓↓ */

    fn highest_pc(&self) -> Result<PhaseCertificate, KVGetError> {
        PhaseCertificate::deserialize(&mut &*self.get(&variables::HIGHEST_PC).ok_or(
            KVGetError::ValueExpectedButNotFound {
                key: Key::HighestPC,
            },
        )?)
        .map_err(|err| KVGetError::DeserializeValueError {
            key: Key::HighestPC,
            source: err,
        })
    }

    /* ↓↓↓ Highest Committed Block ↓↓↓ */

    fn highest_committed_block(&self) -> Result<Option<CryptoHash>, KVGetError> {
        if let Some(bytes) = self.get(&variables::HIGHEST_COMMITTED_BLOCK) {
            let highest_committed_block = CryptoHash::deserialize(&mut &*bytes).map_err(|err| {
                KVGetError::DeserializeValueError {
                    key: Key::HighestCommittedBlock,
                    source: err,
                }
            })?;
            Ok(Some(highest_committed_block))
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Newest Block ↓↓↓ */

    fn newest_block(&self) -> Result<Option<CryptoHash>, KVGetError> {
        if let Some(bytes) = self.get(&variables::NEWEST_BLOCK) {
            let newest_block = CryptoHash::deserialize(&mut &*bytes).map_err(|err| {
                KVGetError::DeserializeValueError {
                    key: Key::NewestBlock,
                    source: err,
                }
            })?;
            Ok(Some(newest_block))
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Highest Timeout Certificate ↓↓↓ */

    fn highest_tc(&self) -> Result<Option<TimeoutCertificate>, KVGetError> {
        if let Some(bytes) = self.get(&variables::HIGHEST_TC) {
            let tc = TimeoutCertificate::deserialize(&mut &*bytes).map_err(|err| {
                KVGetError::DeserializeValueError {
                    key: Key::HighestTC,
                    source: err,
                }
            })?;
            Ok(Some(tc))
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Previous Validator Set ↓↓↓ */

    fn previous_validator_set(&self) -> Result<ValidatorSet, KVGetError> {
        let previous_validator_set_bytes = ValidatorSetBytes::deserialize(
            &mut &*self.get(&variables::PREVIOUS_VALIDATOR_SET).ok_or(
                KVGetError::ValueExpectedButNotFound {
                    key: Key::CommittedValidatorSet,
                },
            )?,
        )
        .map_err(|err| KVGetError::DeserializeValueError {
            key: Key::CommittedValidatorSet,
            source: err,
        })?;
        ValidatorSet::try_from(previous_validator_set_bytes).map_err(|err| {
            KVGetError::Ed25519DalekError {
                key: Key::CommittedValidatorSet,
                source: err,
            }
        })
    }

    /* ↓↓↓ Validator Set Update Block Height ↓↓↓ */

    fn validator_set_update_block_height(&self) -> Result<Option<BlockHeight>, KVGetError> {
        if let Some(bytes) = self.get(&variables::VALIDATOR_SET_UPDATE_BLOCK_HEIGHT) {
            let block_height = BlockHeight::deserialize(&mut &*bytes).map_err(|err| {
                KVGetError::DeserializeValueError {
                    key: Key::ValidatorSetUpdateHeight,
                    source: err,
                }
            })?;
            Ok(Some(block_height))
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Validator Set Update Complete ↓↓↓ */
    fn validator_set_update_complete(&self) -> Result<bool, KVGetError> {
        bool::deserialize(
            &mut &*self.get(&variables::VALIDATOR_SET_UPDATE_DECIDED).ok_or(
                KVGetError::ValueExpectedButNotFound {
                    key: Key::ValidatorSetUpdateDecided,
                },
            )?,
        )
        .map_err(|err| KVGetError::DeserializeValueError {
            key: Key::ValidatorSetUpdateDecided,
            source: err,
        })
    }

    /* ↓↓↓ Validator Set State ↓↓↓ */

    fn validator_set_state(&self) -> Result<ValidatorSetState, KVGetError> {
        Ok(ValidatorSetState::new(
            self.committed_validator_set()?,
            self.previous_validator_set()?,
            self.validator_set_update_block_height()?,
            self.validator_set_update_complete()?,
        ))
    }

    /* ↓↓↓ Highest View Phase-Voted ↓↓↓ */

    fn highest_view_phase_voted(&self) -> Result<Option<ViewNumber>, KVGetError> {
        if let Some(bytes) = self.get(&variables::HIGHEST_VIEW_PHASE_VOTED) {
            let view_number = ViewNumber::deserialize(&mut &*bytes).map_err(|err| {
                KVGetError::DeserializeValueError {
                    key: Key::HighestViewPhaseVoted,
                    source: err,
                }
            })?;
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
    DeserializeValueError {
        key: Key,
        source: std::io::Error,
    },
    ValueExpectedButNotFound {
        key: Key,
    },
    Ed25519DalekError {
        key: Key,
        source: ed25519_dalek::SignatureError,
    },
}

#[derive(Debug)]
pub enum Key {
    BlockHeight { block: CryptoHash },
    BlockJustify { block: CryptoHash },
    BlockDataHash { block: CryptoHash },
    BlockDataLength { block: CryptoHash },
    BlockData { block: CryptoHash },
    BlockAtHeight { height: BlockHeight },
    BlockChildren { block: CryptoHash },
    CommittedAppState { key: Vec<u8> },
    PendingAppStateUpdates { block: CryptoHash },
    CommittedValidatorSet,
    ValidatorSetUpdatesStatus { block: CryptoHash },
    LockedPC,
    HighestViewEntered,
    HighestPC,
    HighestCommittedBlock,
    NewestBlock,
    HighestTC,
    PreviousValidatorSet,
    ValidatorSetUpdateHeight,
    ValidatorSetUpdateDecided,
    HighestViewPhaseVoted,
}

impl Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            &Key::BlockHeight { block } => write!(f, "Block Height for block {}", block),
            &Key::BlockJustify { block } => write!(f, "Block Justify for block {}", block),
            &Key::BlockDataHash { block } => write!(f, "Block Data Hash for block {}", block),
            &Key::BlockDataLength { block } => write!(f, "Block Data length for block {}", block),
            &Key::BlockData { block } => write!(f, "Block Data for block {}", block),
            &Key::BlockAtHeight { height } => write!(f, "Block at height {}", height.int()),
            &Key::BlockChildren { block } => write!(f, "Block children for block {}", block),
            &Key::CommittedAppState { key } => write!(f, "Committed App State for key {:#?}", key),
            &Key::PendingAppStateUpdates { block } => {
                write!(f, "Pending App State Updates for block {}", block)
            }
            &Key::CommittedValidatorSet => write!(f, "Committed Validator Set"),
            &Key::ValidatorSetUpdatesStatus { block } => {
                write!(f, "Validator Set Updates Status for block {}", block)
            }
            &Key::LockedPC => write!(f, "Locked PC"),
            &Key::HighestViewEntered => write!(f, "Highest View Entered"),
            &Key::HighestPC => write!(f, "Highest Phase Certificate"),
            &Key::HighestCommittedBlock => write!(f, "Highest Committed Block"),
            &Key::NewestBlock => write!(f, "Newest Block"),
            &Key::HighestTC => write!(f, "Highest Timeout Certificate"),
            &Key::PreviousValidatorSet => write!(f, "Previous Validator Set"),
            &Key::ValidatorSetUpdateHeight => write!(f, "Validator Set Update Block Height"),
            &Key::ValidatorSetUpdateDecided => write!(f, "Validator Set Update Decided"),
            &Key::HighestViewPhaseVoted => write!(f, "Highest View Phase-Voted"),
        }
    }
}

pub trait WriteBatch {
    fn new() -> Self;
    fn set(&mut self, key: &[u8], value: &[u8]);
    fn delete(&mut self, key: &[u8]);
}
