/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

#![doc(
    html_logo_url = "https://raw.githubusercontent.com/parallelchain-io/hotstuff_rs/refs/heads/final_touches/readme_assets/HotStuff-rs%20logo.png",
    html_favicon_url = "https://raw.githubusercontent.com/parallelchain-io/hotstuff_rs/refs/heads/final_touches/readme_assets/HotStuff-rs%20logo.png"
)]
#![allow(rustdoc::private_intra_doc_links)]

//! A Rust Programming Language library for performant Byzantine Fault Tolerant State Machine
//! Replication (BFT SMR).
//!
//! # Features
//!
//! - **Pluggable components**: library users get to provide their own business logic
//!   ([`app`]), key-value storage ([`kv_store`](crate::block_tree::pluggables::KVStore)), and peer-to-peer
//!   networking ([`network`](crate::networking::network)). HotStuff-rs is agnostic to these details and
//!   can therefore be adapted to many use-cases.
//! - **Dynamic validator sets**: `App`s can control and change the set of replicas ("validators") that
//!   vote to replicate the state machine without any downtime or manual configuration. Unlike older
//!   generations of SMR, "validator set updates" are not stop-the-world operations.
//! - **View synchronization**: HotStuff-rs implements Byzantine View Synchronization in the [`pacemaker`]
//!   module. This enables it to quickly bring replicas' view number in sync and therefore recover quickly
//!   from a network disruption.
//! - **Rigorous specification**: HotStuff-rs is not only rigorously specified, but its specification is
//!   integrated into its Rustdocs so that they remain up-to-date with the implementation. This includes
//!   proofs of correctness for each of its subprotocols.
//!
//! # Getting started
//!
//! To replicate your application using HotStuff-rs, you need to represent it as a type and then
//! have the type implement the [`App`](app) trait.
//!
//! Then, [`initialize`](replica::Replica::initialize) the replica's storage,
//! [`build`](replica::ReplicaSpec::builder) a `ReplicaSpec`,
//! and then [`start`](replica::ReplicaSpec::start) the replica.
//!
//! An example on [how to start a replica](replica#starting-a-replica) can be found in the rustdocs for
//! the `replica` module.
//!
//! # Understanding HotStuff-rs
//!
//! The best way to understand HotStuff-rs is by reading these Rustdocs (so be happy, you are already at
//! the right place!).
//!
//! HotStuff-rs' Rustdocs are designed to be both **documentation**, and **specification**. This means
//! that it does not only *describe* the current (v0.4.0) version of this (Rust) implementation of
//! HotStuff-rs, but also *prescribes* a protocol that developers can use to implement BFT SMR libraries
//! that are compatible with HotStuff-rs.
//!
//! Before starting to read these Rustdocs, it is helpful to have a good high-level awareness of the
//! concepts behind HotStuff-rs, as well as a high-level picture of how the library's modules are
//! organized.
//!
//! The [Concepts](#concepts) subsection helps with the former by introducing a slew of the important
//! concepts in a way gives a sense of the relationships between them, as well as linking to further
//! reading resources whenever they are available, while the [Organization](#organization) subsection
//! helps with the latter by briefly describing how the module tree is structured.
//!
//! ## Concepts
//!
//! The library user implements their business logic in an [`App`](app) and provides a handle to it
//! to HotStuff-rs via the `app` setter on [`ReplicaSpecBuilder`](replica::ReplicaSpec::builder). The
//! library user then [`start`](replica::ReplicaSpec::start)s a replica.
//!
//! Upon startup, the replica starts a background thread called the [`algorithm`] thread. The algorithm
//! thread then creates instances of HotStuff-rs' three subprotocols--namely [`hotstuff`],
//! [`pacemaker`], and [`block_sync`]--and starts to poll these instances in an infinite loop, driving
//! each subprotocol forward.
//!
//! The Pacemaker subprotocol works to keep track of an increasing counter called the "view number",
//! which is a sort of logical clock that the algorithm thread uses to decide the replica should do at
//! any moment in time. At any given view, a replica could either be a
//! [validator, or a listener](replica#kinds-of-replicas). Suppose that it is a validator. Then, it
//! needs to play a [role](hotstuff::roles) in the HotStuff subpprotocol.
//!
//! The two main roles validator can play in the HotStuff subprotocol are:
//! [Proposer](hotstuff::roles::is_proposer), and [Phase-Voter](hotstuff::roles::is_phase_voter). These
//! two roles are not mutually exclusive. For example, it could be that in a given view, a  validator
//! is both a proposer, and a phase-voter.
//!
//! Suppose that a validator is a proposer. Then, it will begin the view by calling
//! [`produce_block`](app::App::produce_block) to ask the `App` to generate the contents of a new
//! [`Block`](types::block::Block). Then, it will pack the block in a
//! [`Proposal`](hotstuff::messages::Proposal) and broadcast this to all other replicas through the
//! user-provided P2P [`Network`](networking::network).
//!
//! Suppose further that the validator is also phase-voter. Then, after broadcasting the proposal, the
//! validator will wait to receive a proposal from the `Network`. Upon receiving a `Proposal` (which
//! could be the same `Proposal` it just itself proposed), it will call
//! [`validate_block`](app::App::validate_block) to ask the `App` to ask it to validate the block
//! according to user-defined application-level semantics.
//!
//! If the block is valid, then the replica will create a [`PhaseVote`](hotstuff::messages::PhaseVote)
//! by signing a message with its  [`SigningKey`](types::crypto_primitives::SigningKey), and send this
//! to the [`phase_vote_recipient`](hotstuff::roles::phase_vote_recipient). Then, it will insert the
//! block to its [Block Tree](block_tree), which the library stores in the
//! [`KVStore`](block_tree::pluggables::KVStore) provided by the user.
//!
//! The overall result is an immutable "block chain" that is guaranteed to satisfy safety and liveness
//! [`invariants`](block_tree::invariants) as long as no more than 1/3rd of the
//! [`TotalPower`](types::data_types::TotalPower) of a
//! [`ValidatorSet`](types::validator_set::ValidatorSet) is faulty.
//!
//! As all of this is happening, the algorithm constantly emits [`events`], which user code can receive
//! as they occur by registering event handlers.
//!
//! ## Module organization
//!
//! HotStuff-rs' modules are organized into two levels depending on whether the definitions they contain
//! are **subprotocol**-specific, or commonly used across multiple subprotocols.
//!
//! The three subprotocols of HotStuff-rs and their associated modules are:
//! 1. **HotStuff** ([`hotstuff`]): the subprotocol for committing blocks.
//! 2. **Pacemaker** ([`pacemaker`]): the subprotocol for bringing replicas into the same view so that they
//!    can start committing blocks.
//! 3. **Block Sync** ([`block_sync`]): the subprotocol for bringing replicas' block trees up-to-date after
//!    periods of time in which they miss messages.
//!
//! Modules not listed above not subprotocol-specific.

pub mod app;

mod algorithm;

pub mod types;

pub mod logging;

pub mod pacemaker;

pub mod hotstuff;

pub mod block_tree;

pub mod networking;

pub mod block_sync;

pub mod replica;

mod event_bus;

pub mod events;
