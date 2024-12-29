<div align="center">
  <img src="./readme_assets/HotStuff-rs%20logo.png" alt="Description" width="200" />
  <h1>HotStuff-rs</h1>
  
  <p>
    <b>Performant Byzantine Fault Tolerant State Machine Replication (BFT SMR) in Rust</b>
  </p>
  
  <a href="https://parallelchain-io.github.io/hotstuff_rs_docs/tag/v0.4.0/hotstuff_rs/index.html">Comprehensive Docs</a>
  | 
  <a href="https://github.com/parallelchain-io/hotstuff_rs/tree/v0.4.0/tests">Examples</a>
  |
  <a href="https://github.com/parallelchain-io/hotstuff_rs/releases">Changelog</a>

  <p>
<!-- prettier-ignore-start -->

[![crates.io](https://img.shields.io/crates/v/hotstuff_rs)](https://crates.io/crates/hotstuff_rs)
![License](https://img.shields.io/crates/l/hotstuff_rs)
[![dependency status](https://deps.rs/crate/hotstuff_rs/0.4.0/status.svg)](https://deps.rs/crate/hotstuff_rs/0.4.0)

<!-- prettier-ignore-end -->
  </p>

</div>

## Introduction

HotStuff-rs is a Rust Programming Language library for performant Byzantine Fault Tolerant State Machine Replication (BFT SMR).
  
HotStuff-rs implements a variant of the HotStuff consensus protocol, but with extensions like block-sync and dynamic
validator sets that makes this library suited for real-world use-cases (and not just research systems). Some desirable
properties that HotStuff-rs has are:
- **Provable Safety** in the face of up to 1/3rd of voting power being Byzantine at any given moment.
- **Optimal Performance**: consensus in (amortized) 1 round trip time.
- **Simplicity**: A small API (App) for plugging in arbitrary stateful applications.
- **Modularity**: pluggable networking, state persistence, and view-synchronization mechanism.
- **Dynamic Validator Sets** that can update without any downtime based on state updates: a must for PoS blockchain 
   applications.
- **Batteries included**: comes with a block-sync protocol and (coming soon) default implementations for networking,
   state, and pacemaker: you write the app, we handle the replication.

## Terminology
 
- **App**: user code which implements the App trait. This can be any business logic that can be expressed
  as a deterministic state machine, i.e., a pure function of kind: `(Blockchain, App State, Validator Set, Block) ->
  (Next Blockchain, Next App State, Next Validator Set)`.
- **Replica**: a public-key-identified process that hosts an implementation of the HotStuff-rs protocol, e.g., this
  library. There are two kinds of Replicas: validators and listeners, and each replica contains an instance of an app.
- **Blockchain**: a growing sequence of **Blocks**, which can be thought of as instructions to update a replica's App
  State and Validator Set.
- **App State**: a key-value store that applications can use to store anything; two replicas with the same Blockchain
  are guaranteed to have the same app state.
- **Validator Set**: the set of replicas who can vote in a consensus decision.
- **Progress protocol**: the protocol replicas use to create new blocks through consensus and grow the blockchain.
- **Sync protocol**: the protocol new or previously offline replicas use to quickly catch up to the head of the
  blockchain.

## Getting Started

The [integration tests](./tests/) serve as good examples on how to get a dummy HotStuff-rs network up and running. 

To really understand HotStuff-rs however, we recommend that you read the
 [comprehensive docs](https://parallelchain-io.github.io/hotstuff_rs_docs/tag/v0.4.0/hotstuff_rs/index.html). Like the [docs.rs](https://docs.rs/hotstuff_rs/latest/hotstuff_rs/), which is also available, these are generated using `cargo doc`. Unlike the docs.rs however,
these include documentation for internal modules and definitions instead of hiding them. We believe that the documentation
for these items are essential for understanding the features and guarantees of HotStuff-rs SMR, and that understanding them
require no prerequisite knowledge about SMR algorithms!


## Opening an issue

Open an issue in GitHub if you:
1. Have a feature request / feature idea,
2. Have any questions (particularly software related questions),
3. Think you may have discovered a bug.

Please try to label your issues appropriately.

## The HotStuff Consensus Protocol

HotStuff works by building a 'BlockTree': a directed acyclic graph of Blocks. Block is a structure with a `data` field which applications are free to populate with arbitrary byte-arrays. In consensus algorithm literature, we typically talk of consensus algorithms as maintaining state machines that change their internal states in response to commands, hence the choice of terminology.

HotStuff guarantees that committed Blocks are *immutable*. That is, they can never be *un*-committed as long as at least a supermajority of voting power faithfully execute the protocol. This guarantee enables applications to make hard-to-reverse actions with confidence. 

![A graphic depicting a Tree (DAG) of Blocks. Blocks are colored depending on how many confirmations they have.](./readme_assets/BlockTree%20Structure%20Diagram.png)

A Block becomes *committed* the instant its third confirmation is written into the BlockTree. A confirmation for a Block `A` is another Block `B` such that there is path between `B` to `A`.

The choice of third confirmation to define commitment--as opposed to first or second--is not arbitrary. HotStuff's safety and liveness properties actually hinge upon on this condition. If you really want to understand why this is the case, you should read the [paper](./readme_assets/HotStuff%20paper.pdf). To summarize:

1. Classic BFT consensus algorithms such as PBFT require only 2 confirmations for commitment, but this comes at the cost of expensive leader-replacement flows.
2. Tendermint require only 2 confirmations for commitment and has a simple leader-replacement flow, but needs an explicit 'wait-for-N seconds' step to guarantee liveness.

HotStuff is the first consensus algorithm with a simple leader-replacement algorithm that does not have a 'wait-for-N seconds' step, and thus can make progress as fast as network latency allows.
