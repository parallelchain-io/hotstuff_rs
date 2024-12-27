/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Byte-prefixes that specify where each Block Tree variable is stored in the user-provided key-value
//! store.
//!
//! # List of State Variables
//!
//! HotStuff-rs structures its state into 17 separate conceptual "variables" that are stored in tuples
//! that sit at [specific key paths](super::paths) in the library user's chosen
//! KV store. These 17 variables are listed below, grouped into 4 categories for ease of understanding:
//!
//! ## Blocks
//!
//! |Variable|Type|Description|
//! |---|---|---|
//! |Blocks|[`CryptoHash`](crate::types::data_types::CryptoHash) -> [`Block`](crate::types::block::Block)|Mapping between a block's hash and the block itself. This mapping contains all blocks that have been inserted into the block tree, excluding blocks that have been pruned.|
//! |Block at Height|[`BlockHeight`](crate::types::data_types::BlockHeight) -> [`CryptoHash`](crate::types::data_types::CryptoHash)|Mapping between a block's height and its block hash. This mapping only contains blocks that are committed, because if a block hasn't been committed, there may be multiple blocks at the same height.|
//! |Block to Children|[`CryptoHash`](crate::types::data_types::CryptoHash) -> [`ChildrenList`](crate::types::data_types::ChildrenList)|Mapping between a block's hash and the list of children it has in the block tree. A block may have multiple children if they have not been committed.|
//!
//! ## App State
//!
//! |Variable|Type|Description|
//! |---|---|---|
//! |Committed App State|[`Vec<u8>`] -> [`Vec<u8>`]|All key value pairs in the current committed app state. Produced by applying all app state updates in sequence from the genesis block up to the highest committed block.|
//! |Pending App State Updates|[`CryptoHash`](crate::types::data_types::CryptoHash) -> [`AppStateUpdates`](crate::types::update_sets::AppStateUpdates)|Mapping between a block's hash and its app state updates. This is empty for an existing block if at least one of the following two cases is true: <ol><li>The block does not update the app state.</li> <li>The block has been committed.</li></ol>|
//!
//! ## Validator Set
//!
//! |Variable|Type|Description|
//! |---|---|---|
//! |Committed Validator Set|[`ValidatorSet`](crate::types::validator_set::ValidatorSet)|The current committed validator set. Produced by applying all validator set updates in sequence from the genesis block up to the highest committed block.|
//! |Previous Validator Set|[`ValidatorSet`](crate::types::validator_set::ValidatorSet)|The committed validator set before the latest validator set update was committed. <br/><br/>Until a validator set update is considered "decided" (as indicated by the next variable), the previous validator set remains "active". That is, leaders will continue broadcasting nudges for the previous validator set and replicas will continue voting for such nudges.|
//! |Validator Set Update Decided|[`bool`]|A flag that indicates the most recently committed validator set update has been decided.|
//! |Validator Set Update Block Height|[`BlockHeight`](crate::types::data_types::BlockHeight)|The height of the block that caused the most recent committed (but perhaps not decided) validator set update.|
//! |Validator Set Updates Status|[`CryptoHash`](crate::types::data_types::CryptoHash) -> [`ValidatorSetUpdatesStatus`](crate::types::validator_set::ValidatorSetUpdatesStatus)|Mapping between a block's hash and its validator set updates. <br/><br/>Unlike [pending app state updates](#app-state), this is an enum, and distinguishes between the case where the block does not update the validator set and the case where the block updates the validator set but has been committed.|
//!
//! ## Safety
//!
//! |Variable|Type|Description|
//! |---|---|---|
//! |Locked Phase Certificate|[`PhaseCertificate`](crate::hotstuff::types::PhaseCertificate)|The currently locked PC. [Read more](invariants#locking)|
//! |Highest View Phase-Voted|[`ViewNumber`](crate::types::data_types::ViewNumber)|The highest view that this validator has phase-voted in.|
//! |Highest View Entered|[`ViewNumber`](crate::types::data_types::ViewNumber)|The highest view that this validator has entered.|
//! |Highest Phase Certificate|[`PhaseCertificate`](crate::hotstuff::types::PhaseCertificate)|Among the phase certificates this validator has seen and verified, the one with the highest view number.|
//! |Highest Timeout Certificate|[`TimeoutCertificate`](crate::pacemaker::types::TimeoutCertificate)|Among the timeout certificates this validator has seen and verified, the one with the highest view number.|
//! |Highest Committed Block|[`CryptoHash`](crate::types::data_types::CryptoHash)|The hash of the committed block that has the highest height.|
//! |Newest Block|[`CryptoHash`](crate::types::data_types::CryptoHash)|The hash of the most recent block to be inserted into the block tree.|
//!
//! # Persistence of state variables
//!
//! This section specifies how each state variable is persisted into user-provided
//! [key-value stores (`KVStore`)](super::pluggables). Each variable is stored as **Borsh-serialized
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
//! [`WriteBatch`](super::pluggables::WriteBatch), one would do:
//!
//! ```
//! # use borsh::ser::BorshSerialize;
//! # use hotstuff_rs::{
//! #     types::{
//! #         data_types::{BlockHeight, CryptoHash, Data},
//! #         block::Block,
//! #     },
//! #     block_tree::{
//! #         variables::{COMMITTED_APP_STATE, concat},
//! #         pluggables::WriteBatch,
//! #     },
//! #     hotstuff::types::PhaseCertificate,
//! # };
//! #
//! # fn write_to_app_state(mut write_batch: impl WriteBatch) {
//! const as_key: &str = "hello";
//! const as_value: &str = "world";
//! let key = concat(&COMMITTED_APP_STATE, &as_key.try_to_vec().unwrap());
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
pub fn concat(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut res = Vec::with_capacity(a.len() + b.len());
    res.extend_from_slice(a);
    res.extend_from_slice(b);
    res
}
