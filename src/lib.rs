/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! A Rust Programming Language library for Byzantine Fault Tolerant state machine replication, intended for production
//! systems.
//!  
//! HotStuff-rs implements a variant of the HotStuff consensus protocol, but with extensions like block-sync and dynamic
//! validator sets that makes this library suited for real-world use-cases (and not just research systems). Some desirable
//! properties that HotStuff-rs has are:
//! 1. **Provable Safety** in the face of up to 1/3rd of voting power being Byzantine at any given moment.
//! 2. **Optimal Performance**: consensus in (amortized) 1 round trip time.
//! 3. **Simplicity**: A small API [app](app::App) for plugging in arbitrary stateful applications.
//! 4. **Modularity**: pluggable [networking], [state persistence](state), and [view-synchronization mechanism](pacemaker).
//! 5. **Dynamic Validator Sets** that can update without any downtime based on state updates: a must for PoS blockchain
//!    applications.
//! 6. **Batteries included**: comes with a block-sync protocol and (coming soon) default implementations for networking,
//!    state, and pacemaker: you write the app, we handle the replication.
//! 7. **Custom event handlers**: includes a simple API for registering user-defined event handlers, triggered in response to
//!    HotStuff-rs protocol [event](events) notifications.
//!
//! ## Terminology
//!  
//! - **App**: user code which implements the [app trait](app::App). This can be any business logic that can be expressed
//!   as a deterministic state machine, i.e., a pure function of kind: `(Blockchain, App State, Validator Set, Block) ->
//!   (Next Blockchain, Next App State, Next Validator Set)`.
//! - **Replica**: a public-key-identified process that hosts an implementation of the HotStuff-rs protocol, e.g., this
//!   library. There are [two kinds of Replicas](replica): validators, and listeners, and each replica contains an instance
//!   of an app.
//! - **Blockchain**: a growing sequence of **Blocks**, which can be thought of as instructions to update a replica's App
//!   State and Validator Set.
//! - **App State**: a key-value store that applications can use to store anything; two replicas with the same Blockchain
//!   are guaranteed to have the same app state.
//! - **Validator Set**: the set of replicas who can vote in a consensus decision.
//! - **Progress protocol**: the protocol replicas use to create new blocks through consensus and grow the blockchain.
//! - **Sync protocol**: the protocol new or previously offline replicas use to quickly catch up to the head of the
//!   blockchain.
//!
//! ## Getting started
//!
//! To replicate your application using HotStuff-rs, you need to represent it as a type and then
//! have the type implement the [app] trait.
//!
//! Then, [initialize](replica::Replica::initialize) a [replica]'s storage, 
//! [build](replica::ReplicaSpec::builder) the replica's [specification](crate::replica::ReplicaSpec),
//! and then [start](replica::ReplicaSpec::start) it. 
//! 
//! An example of how to start a replica can be found [here](crate::replica).

pub mod app;

pub mod algorithm;

pub mod types;

pub mod logging;

pub mod messages;

pub mod pacemaker;

pub mod state;

pub mod networking;

pub mod sync_server;

pub mod replica;

pub mod event_bus;

pub mod events;