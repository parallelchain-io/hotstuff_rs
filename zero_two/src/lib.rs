/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! HotStuff-rs is a Rust Programming Language implementation of the HotStuff consensus protocol. It offers:
//! 1. Guaranteed Safety and Liveness in the face of up to 1/3rd of voting power being Byzantine at any given moment.
//! 2. A small API [app::App] for plugging in arbitrary state machine-based applications like blockchains.
//! 3. Modular [networking], [state], and [pacemaker](view-synchronization mechanism).
//! 4. Well-documented, 'obviously correct' source code, designed for easy analysis and testing.
//! 
//! If you're trying to learn about HotStuff-rs by reading the source code or Cargo docs, we recommend starting from
//! the [replica](replica) module. This is contains the entry-point of user code into HotStuff-rs.

pub mod app;

pub mod algorithm;

pub mod types;

pub mod messages;

pub mod pacemaker;

pub mod state;

pub mod networking;

#[cfg(test)]
pub mod tests;

pub mod sync_server;

pub mod replica;