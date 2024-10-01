/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/
//! Byte-prefixes that specify where each Block Tree variable is stored in the user-provided key-value
//! store.
//!
//! # Storage of state variables
//!
//! As explained in the docs for the `block_tree` module, the block tree is comprised of 17
//! [state variables](super::block_tree#state-variables).
//!
//! This module specifies how each state variable is persisted into user-provided
//! [key-value stores (`KVStore`)](super::kv_store). Each variable is stored as **Borsh-serialized
//! values** in one or more **keys** in the key-value store. These keys are formed by combining the
//! [constants](#constants) defined in this module in specific ways, as described in the following three
//! subsections:
//!
//! ## Single values
//!
//! "Single values" (e.g., committed validator set, locked PC) are stored in one-byte, constant keys
//! defined in constants sharing the variable's name.
//!
//! For example, the locked PC variable is set by serializing a `PhaseCertificate` and setting it at
//! the [`LOCKED_PC`] key.
//!
//! ## Mappings
//!
//! Mappings of the form "`A` -> `B`" (e.g., blocks) are stored in multiple keys, each key being the
//! concatenation of a specific constant one-byte prefix sharing the variable's name, and then the
//! serialization of an instance of the `A` type.
//!
//! For example, to store a key-value pair into the app state directly using
//! [`WriteBatch`](super::write_batch::WriteBatch), one would do:
//!
//! ```
//! # use borsh::ser::BorshSerialize;
//! # use hotstuff_rs::{
//! #     types::{
//! #         data_types::{BlockHeight, CryptoHash, Data},
//! #         block::Block,
//! #     },
//! #     hotstuff::types::PhaseCertificate,
//! #     state::{
//! #         write_batch::WriteBatch,
//! #         paths::{self, combine},
//! #     },
//! # };
//! #
//! # fn write_to_app_state(mut write_batch: impl WriteBatch) {
//! const as_key: &str = "hello";
//! const as_value: &str = "world";
//! let key = combine(&paths::COMMITTED_APP_STATE, &as_key.try_to_vec().unwrap());
//! write_batch.set(&key, &as_value.try_to_vec().unwrap());
//! # }
//! ```
//!
//! Note: if you ever need to set variables in the block tree, you shouldn't need to use `WriteBatch`
//! directly. Instead, use [`BlockTreeWriteBatch`](super::write_batch::BlockTreeWriteBatch), which
//! abstracts the forming of keys from you and then internally calls `WriteBatch::set`.
//!
//! ## Blocks
//!
//! Blocks are neither single values nor mappings and are stored in a special way. Each block can be
//! thought of as being stored in **5** separate state variables, with each state variable corresponding
//! to one of the block's 5 fields.
//!
//! The following two subsections describe how these fields are stored. The first 4 of these fields (the
//! non-`data` fields) are stored in a straightforward manner and are discussed in the first subsection.
//! Storage of the remaining field, `data`, is more complicated and so is discussed separately in the
//! second subsection. **Note** that because blocks appear in the block tree only as part of the
//! "blocks" state variable, which is a mapping from `CryptoHash` to `Block`, all of the following
//! discussions will assume this context.
//!
//! ### Non-`data` fields
//!
//! The non-`data` fields of a block (`height`, `hash`, `justify`, and `data_hash`) are stored like
//! **separate** single values at keys formed by concatenating three bytestrings in sequence:
//! 1. The [`BLOCKS`] constant.
//! 2. `block.hash`.
//! 3. A constant byte sharing the same name as the field. (e.g., for a block's height, this byte is
//!    `BLOCK_HEIGHT`.
//!
//! ### Data field
//!
//! A block's `data` is stored in two sets of keys, or more specifically, one single key and one
//! separate set of keys:
//! 1. The single key (`BLOCKS` + `block.hash` + [`BLOCK_DATA_LEN`]) stores `data.len() as u32`.
//! 2. The set of keys (`BLOCKS` + `block.hash` + [`BLOCK_DATA`] + `block.data[i]`) stores each datum
//!    in the data vector in sequence.
//!
//! If `block.data` is empty, then none of the keys in the second set will be set. However, the data
//! length key will be set to `0 as u32`. Generally speaking, if a block exists in the block tree, its
//! data length key will always be set.

// State variables
pub const BLOCKS: [u8; 1] = [0];
pub const BLOCK_AT_HEIGHT: [u8; 1] = [1];
pub const BLOCK_TO_CHILDREN: [u8; 1] = [2];
pub const COMMITTED_APP_STATE: [u8; 1] = [3];
pub const PENDING_APP_STATE_UPDATES: [u8; 1] = [4];
pub const COMMITTED_VALIDATOR_SET: [u8; 1] = [5];
pub const VALIDATOR_SET_UPDATES_STATUS: [u8; 1] = [6];
pub const LOCKED_PC: [u8; 1] = [7];
pub const HIGHEST_VIEW_ENTERED: [u8; 1] = [8];
pub const HIGHEST_PC: [u8; 1] = [9];
pub const HIGHEST_COMMITTED_BLOCK: [u8; 1] = [10];
pub const NEWEST_BLOCK: [u8; 1] = [11];
pub const HIGHEST_TC: [u8; 1] = [12];
pub const PREVIOUS_VALIDATOR_SET: [u8; 1] = [13];
pub const VALIDATOR_SET_UPDATE_BLOCK_HEIGHT: [u8; 1] = [14];
pub const VALIDATOR_SET_UPDATE_DECIDED: [u8; 1] = [15];
pub const HIGHEST_VIEW_PHASE_VOTED: [u8; 1] = [16];

// Fields of Block
pub const BLOCK_HEIGHT: [u8; 1] = [0];
pub const BLOCK_JUSTIFY: [u8; 1] = [1];
pub const BLOCK_DATA_HASH: [u8; 1] = [2];
pub const BLOCK_DATA_LEN: [u8; 1] = [3];
pub const BLOCK_DATA: [u8; 1] = [4];

/// Concatenate two byteslices into one vector.
pub fn combine(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut res = Vec::with_capacity(a.len() + b.len());
    res.extend_from_slice(a);
    res.extend_from_slice(b);
    res
}
