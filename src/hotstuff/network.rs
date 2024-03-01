/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of the [HotStuffMessageBuffer] for efficient and overflow-resistant caching
//! of [HotStuffMessage].

// use std::collections::{BTreeMap, VecDeque};

// use ed25519_dalek::VerifyingKey;

// use crate::types::basic::ViewNumber;
// use crate::hotstuff::messages::HotStuffMessage;

// pub(crate) struct HotStuffMessageBuffer {
//     buffer_capacity: u64,
//     buffer: BTreeMap<ViewNumber, VecDeque<(VerifyingKey, HotStuffMessage)>>,
//     buffer_size: u64,
// }

// pub(crate) struct FailedToInsertError {}

// impl HotStuffMessageBuffer {

//     fn new(buffer_capacity: u64) {
//         Self {
//             buffer_capacity,
//             buffer: BTreeMap::new(),
//             buffer_size: 0,
//         }
//     }

//     fn insert_to_buffer(msg: HotStuffMessage, origin: VerifyingKey) -> Result<(), FailedToInsertError> {
//         todo!()
//     }

//     /// Given the number of bytes that need to be removed, removes just enough highest-viewed messages
//     /// to free up (at least) the required number of bytes in the buffer.
//     fn remove_from_overloaded_buffer(&mut self, bytes_to_remove: u64) {
//         todo!()
//     }
// }