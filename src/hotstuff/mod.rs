//! Subprotocol for committing `Block`s.
//!
//! # Introduction
//!
//! The HotStuff subprotocol is the core of HotStuff-rs BFT SMR.
//!
//! As the rustdocs for `block_tree::invariants`
//! [explains](crate::block_tree::invariants#committing-a-block), the block tree commits a block when
//! the [`BlockTreeSingleton::update`](crate::block_tree::accessors::internal::BlockTreeSingleton::update)
//! method creates a “3-Chain” extending the block. Fundamentally, the role of the HotStuff subprotocol
//! is to drive this process by orchestrating validators to work together to *create* the building
//! blocks of 3-Chains, namely `PhaseCertificate`s.
//!
//! Prerequisite to the HotStuff subprotocol being able to reliably create new `PhaseCertificates` is
//! for a quorum of validators to be in the same view for a "long enough" duration of time, which
//! HotStuff-rs guarantees via the [`pacemaker`](crate::pacemaker) subprotocol.
//!
//! The HotStuff subprotocol is based on the “HotStuff” SMR algorithm described in the
//! [PODC ‘19 paper](https://dl.acm.org/doi/pdf/10.1145/3293611.3331591) by Yin, et al., but deviates
//! significantly from the original algorithm to enable a useful feature, namely **dynamic validator set
//! updates**.
//!
//! # Challenges for dynamic validator set updates
//!
//! Concretely, support for dynamic validator set updates in HotStuff-rs means that `Block`s can cause
//! validator set updates in the same way they can cause app state updates. `App`s trigger the creation
//! of a validator-set-updating block by returning a `ProduceBlockResponse` with
//! [`Some(validator_set_updates)`](crate::app::ProduceBlockResponse::validator_set_updates) from
//! `produce_block`.
//!
//! But trying to implement dynamic validator set updates directly on top of the original "Chained
//! HotStuff" algorithm as specified in the PODC '19 paper raises two difficult challenges,
//! specifically: 1. A **usability challenge**, and 2. A **liveness challenge**.
//!
//! The following two subsections discuss each challenge in turn. Then, the next section discusses how
//! HotStuff-rs' HotStuff subprotocol solves them.
//!
//! ## Usability challenge
//!
//! In Chained HotStuff, a block only becomes committed when a 3-Chain is built on top of it. This means
//! that validator set updates do not happen "immediately": if block `B` updates the validator set,
//! blocks `B'` and `B''` (where `B <- B' <- B''` and "`<-`" is a `justify`-link) will still be proposed
//! and voted on by members of the previous, non-updated validator set.
//!
//! This behavior could be confusing for users, especially considering that app state updates (the
//! other of the [two kinds of updates](crate::app#two-app-mutable-states-app-state-and-validator-set)
//! that a block could cause) *are* immediate, because of the buffering of pending ancestor app state
//! updates by `AppBlockTreeView`.
//!
//! Even worse, while 3-Chain is a necessary requirement for a block to become committed, it is not a
//! sufficient requirement. In particular, commitment in Chained HotStuff additionally requires
//! ["consecutive views"](crate::block_tree::invariants#committing-a-block), and because `justify`-links
//! between consecutive blocks do not, by default, have consecutive views, implementing dynamic
//! validator set updates directly on Chained HotStuff would make it so that there is no upper bound
//! on how many blocks have to follow a validator-set-updating block in order for its validator set
//! updates to be committed.
//!
//! For our SMR algorithm, we would like validator set updates to happen immediately. Specifically,
//! validator set updates must satisfy a property we call **"immediacy"**: if a block updates the
//! validator set from `VS1` to `VS2`, then children of the block must be proposed by a member of `VS2`,
//! and voted on by members of `VS2`.
//!
//! ##  Liveness challenge
//!
//! Both Chained and non-Chained HotStuff algorithms described in the PODC '19 paper implicitly assume
//! that all replicas keep track of exactly one, constant validator set. Supporting dynamic validator
//! sets, then, minimally means replacing this constant validator set with a variable validator set.
//!
//! But this change alone is not enough to produce a workable SMR algorithm. In particular, we'll see
//! that unless these algorithms are changed further so that replicas can track *multiple* validator
//! sets, they are vulnerable to a serious liveness problem.
//!
//! Consider a version of the Chained HotStuff algorithm minimally modified to support dynamic validator
//! sets and where replicas only keep track of a single validator set at any given time; that is, the
//! latest validator set that the replica is aware of.
//!
//! Then, let `U` denote a validator set update `VS1 -U-> VS2` from validator set `VS1` to another
//! validator set `VS2`. In this setting, we could enter a situation where:
//! - **Over** 1/3rd of replicas in `VS1` have committed `U`,
//! - but **less than** 2/3rds of replicas in `VS2` have committed `U`.
//!
//! In such a situation, neither `VS1` or `VS2` will have a quorum of active validators, so no more new
//! `PhaseCertificate`s can be created. Then, if no existing `PhaseCertificate`s are set to arrive or
//! rebroadcasted to bring more replicas in `VS2` to commit `U`, the block tree will permanently cease
//! to grow.
//!
//! # Solving the usability and liveness challenges
//!
//! To solve the usability and liveness challenges, the HotStuff subprotocol differs from PODC '19
//! HotStuff in **three** ways. All of these differences are "additive", that is, they add new behavior
//! on top of PODC '19 HotStuff, as opposed to removing or modifying existing behavior.
//!
//! The below subsections explain how each difference works to solve the usability and liveness
//! challenges. In summary: d.o.m. solves the usability problem, v.s.s. solves the liveness problem, and
//! t.d.p. makes v.s.s. easier to implement.
//!
//! ## Dual operating modes
//!
//! The HotStuff subprotocol solves the problem of **immediacy** of validator set updates by
//! implementing two **"operating modes"**, the first used exclusively to commit non-validator set
//! updating blocks, and the second used exclusively to commit validator set updating blocks. These two
//! modes differ chiefly in the [`Phase`](types::Phase)s of the QCs that are produced in each mode:
//! - **Pipelined mode**: Only `Generic` QCs are produced, and they serve as `Prepare` QC for
//!   `justify.block`, `Precommit` QC for its parent, and `Commit` QC for its grandparent. This mode
//!   corresponds to Algorithm 3 (aka., Chained HotStuff) in the PODC '19 paper.
//! - **Phased mode**: Only `Prepare`, `Precommit`, `Commit`, and `Decide` QCs are produced, and the only
//!   block they justify is `justify.block`. This mode corresponds to Algorithm 1 in the PODC '19 paper.
//!
//! Phased mode ensures that a validator-set-updating block is committed before work on any of its
//! children begins. It does so by orchestrating replicas through multiple rounds of consensus decisions
//! *without* ever proposing a new `Block`.
//!
//! The message type that the phased mode uses to initiate these consensus decisions is the
//! [`Nudge`](messages::Nudge). These are like the [`Proposal`](messages::Proposal)s that the pipelined
//! mode uses, but contain a `justify` field instead of a `block`.
//!
//! During a view in the phased mode, the proposer broadcasts a `Nudge` to all replicas, and all
//! validators reply with a `Vote` where `vote.phase` is the phase *immediately after*
//! `nudge.justify.phase`. This means, for example, that the reply to a nudge containing a `Precommit`
//! PC must be a vote with `vote.phase == Commit`.
//!
//! ## Validator set speculation
//!
//! HotStuff-rs' insight from the discussion about the liveness issue is that in order of validator set
//! updates to reliably be committed, the network must always be able to create new `PhaseCertificate`s,
//! and in order for new `PhaseCertificate`s to be created during a validator set update, at least *one*
//! validator set must contain a quorum of active validators at any given time.
//!
//! The way HotStuff-rs guarantees that this property holds was hinted in the discussion about liveness
//! issue, which is to have replicas keep track of multiple validator sets, and in particular both the
//! previous validator set and the committed validator set. We call keeping track and participating in
//! multiple validator sets **"validator set speculation"**, and refer to the validator sets that a
//! given replica is currently participating in as its **"active validator sets"**.
//!
//! Validator set speculation is implemented in HotStuff-rs by keeping track of both the committed
//! validator set and the previous validator set in the block tree's
//! [validator set state](crate::block_tree::variables#validator-set).
//!
//! ## The `Decide` phase
//!
//! However, validator set speculation comes at a cost. While speculating, replicas may have to propose,
//! nudge, and vote (sometimes multiple times) in a single view. This extra activity requires extra
//! computational complexity, and since views have a fixed timeout (enforced by the
//! [`pacemaker`](crate::pacemaker)) there might not be enough time in a view to participate in every
//! validator set.
//!
//! So we need validator set speculation to ensure liveness, but to make validator set speculation
//! actually feasible to implement, we need to *limit* the time that a replica spends in speculation to
//! be as short as actually necessary.
//!
//! The HotStuff subprotocol limits time spent in speculation by introducing an additional voting phase
//! called [`Decide`](types::Phase::Decide), which is executed after the `Commit` phase.
//!
//! Using the `VS1 -U-> VS2` notation introduced in a previous section, the special thing about the
//! `Decide` phase compared to the other phases used in the phased mode is that `PhaseCertificate`s with
//! `pc.phase == Decide` contain signatures from `VS2`, as opposed to PCs from other phases, which
//! contain signatures from `VS1`.
//!
//! `Decide` allows the HotStuff subprotocol to define a minimal validator set speculation period.
//! Concretely, the period in which a replica speculates on validator set update `U` (and therefore
//! participates in both `VS1` and `VS2`):
//! - **Begins** when a `Commit` PC is received that justifies the block that caused `U`,
//! - and **ends** when when a `Decide` PC is received that justifies the same block.
//!
//! Ending validator set speculation upon receiving a `Decide` PC guarantees liveness because a `Decide`
//! PC being formed for the block that caused `U` implies that a quorum (more than 2/3rds) of validators
//! in `VS2` have committed `U`, and therefore are ready to form new `PhaseCertificate`s.

pub mod messages;

pub mod types;

pub(crate) mod implementation;

pub(crate) mod sequence_flow;

pub(crate) mod roles;
