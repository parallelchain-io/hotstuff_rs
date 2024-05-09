/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/
//! This module defines the location of each of the variables stored in the key-value store.
//! These variables are:
//! 
//! |Variable|"Type"|Description|
//! |---|---|---|
//! |Blocks|[CryptoHash] -> [Block]||
//! |Block at Height|[BlockHeight] -> [CryptoHash]|A mapping between a block's number and a block's hash. This mapping only contains blocks that are committed, because if a block hasn't been committed, there may be multiple blocks at the same height.|
//! |Block to Children|[CryptoHash] -> [ChildrenList]|A mapping between a block's hash and the children it has in the block tree. A block may have multiple children if they have not been committed.|
//! |Committed App State|[Vec<u8>] -> [Vec<u8>]||
//! |Pending App State Updates|[CryptoHash] -> [AppStateUpdates]||
//! |Committed Validator Set|[ValidatorSet]|The acting validator set.|
//! |Previous Validator Set|[ValidatorSet]|The previous acting validator set, possibly still active if the update has not been completed yet.|
//! |Validator Set Update Completed|[bool]|Whether the most recently initiated validator set update has been completed. A validator set update is initiated when a commit QC for the corresponding validator-set-updating block is seen.|
//! |Validator Set Update Block Height|The height of the block associated with the most recently initiated validator set update.|
//! |Validator Set Updates Status|[CryptoHash] -> [ValidatorSetUpdatesStatus]||
//! |Locked QC|[QuorumCertificate]| QC of a block that is about to be committed, unless there is evidence for a quorum switching to a conflicting branch. Refer to the HotStuff paper for details.|
//! |Highest Voted View|[ViewNumber]|The highest view that this validator has voted in.|
//! |Highest View Entered|[ViewNumber]|The highest view that this validator has entered.|
//! |Highest Quorum Certificate|[QuorumCertificate]|Among the quorum certificates this validator has seen and verified the signatures of, the one with the highest view number.|
//! |Highest Timeout Certificate|[TimeoutCertificate]|Among the timeout certificates this validator has seen and verified the signatures of, the one with the highest view number.|
//! |Highest Committed Block|[CryptoHash]|The hash of the committed block that has the highest height.|
//! |Newest BlocK|[CryptoHash]The hash of the most recent block to be inserted into the block tree.|

// State variables
pub(super) const BLOCKS: [u8; 1] = [0];
pub(super) const BLOCK_AT_HEIGHT: [u8; 1] = [1];
pub(super) const BLOCK_TO_CHILDREN: [u8; 1] = [2];
pub(super) const COMMITTED_APP_STATE: [u8; 1] = [3];
pub(super) const PENDING_APP_STATE_UPDATES: [u8; 1] = [4];
pub(super) const COMMITTED_VALIDATOR_SET: [u8; 1] = [5];
pub(super) const VALIDATOR_SET_UPDATES_STATUS: [u8; 1] = [6];
pub(super) const LOCKED_QC: [u8; 1] = [7];
pub(super) const HIGHEST_VIEW_ENTERED: [u8; 1] = [8];
pub(super) const HIGHEST_QC: [u8; 1] = [9];
pub(super) const HIGHEST_COMMITTED_BLOCK: [u8; 1] = [10];
pub(super) const NEWEST_BLOCK: [u8; 1] = [11];
pub(super) const HIGHEST_TC: [u8; 1] = [12];
pub(super) const PREVIOUS_VALIDATOR_SET: [u8; 1] = [13];
pub(super) const VALIDATOR_SET_UPDATE_BLOCK_HEIGHT: [u8; 1] = [14];
pub(super) const VALIDATOR_SET_UPDATE_COMPLETED: [u8; 1] = [15];
pub(super) const HIGHEST_VIEW_VOTED: [u8; 1] = [16];

// Fields of Block
pub(super) const BLOCK_HEIGHT: [u8; 1] = [0];
pub(super) const BLOCK_JUSTIFY: [u8; 1] = [1];
pub(super) const BLOCK_DATA_HASH: [u8; 1] = [2];
pub(super) const BLOCK_DATA_LEN: [u8; 1] = [3];
pub(super) const BLOCK_DATA: [u8; 1] = [4];