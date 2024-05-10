/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/
//! This module defines the location of each of the variables stored in the key-value store,
//! as described [here](crate::state::block_tree).

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