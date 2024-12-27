//! Subprotocol for Byzantine view synchronization and leader selection.
//!
//! The documentation for this module is divided into two sections, the first describing how the
//! Pacemaker subprotocol provides [Byzantine View Synchronization](#byzantine-view-synchronization)
//! functionality for HotStuff-rs, and the second describing how it provides
//! [Leader Selection](#leader-selection) functionality.
//!
//! # Byzantine View Synchronization
//!
//! SMR protocols like [HotStuff](super::hotstuff) can only make progress if a quorum of replicas spend
//! a long enough time in the same view in order to go through all of the protocol's voting phases. In
//! other words, state machine replication is only live once **view synchronization** is achieved.
//!
//! Achieving view synchronization is hard, and it is *especially* hard if we have to make any of the
//! following four assumptions, all of which we have to make in order to make a robust BFT SMR solution:
//! 1. **Replicas do not have synchronized clocks**: replicas' local clocks can go out of sync, and
//!    further, replicas cannot rely on any "global" clock such as that provided by the Network Time
//!    Protocol (NTP), since these would be centralized attack targets.
//! 2. **Replicas can fail in arbitrary ways**: replicas can crash, refuse to respond to messages, or
//!    generally behave in malicious ways as if controlled by an adversary.
//! 3. **Message complexity matters**: network bandwidth is limited, so we want to limit the number (and
//!    size) of messages that have to be exchanged in order to achieve view synchronization.
//! 4. **Optimistic responsiveness is desirable**: we want view synchronization to be achieved as quickly
//!    as network delays allow it to be achieved. This eliminates protocols such as "just increment your
//!    local view number every `n` seconds" from our consideration.
//!
//! The view synchronization mechanism used in HotStuff-rs versions 0.3 and prior (as well as mentioned
//! in the original PODC'19 HotStuff paper) is "exponentially increasing view timeouts". This mechanism
//! nominally succeeds in not relying on synchronized clocks, being resistant to Byzantine failures, and
//! having a low message complexity (no messages are ever sent!). However, it completely fails to ensure
//! optimistic responsiveness.
//!
//! The current version of HotStuff-rs (v0.4) achieves view synchronization under all four
//! assumptions by incorporating a brand new **Pacemaker** module (this module) that implements a
//! modified version of the Byzantine View Synchronization algorithm described in
//! [Lewis-Pye (2021)](https://arxiv.org/pdf/2201.01107.pdf).
//!
//! ## High-level idea
//!
//! The modified Lewis-Pye (2021) algorithm implemented by this module divides views into sequences
//! called "epochs". Each of these epochs are of equal length; i.e., if we denote this length as `N`,
//! then:
//! - The 0th epoch would contain views `[0, 1, ..., N-2, N-1]`.
//! - The 1st epoch would contain views `[N, N+1, ..., 2N-2, 2N-1]`.
//! - ...and so on.
//!
//! On top of the delineation of views into epochs described above, the algorithm distinguishes between
//! two kinds of views, "Epoch-Change Views", and "Normal Views":
//! - **Epoch-Change View**: the last view in every epoch (i.e., `{N-1, 2N-1, ...}`).
//! - **Normal View**: every other view.
//!
//! The Pacemaker [`implementation`] works so that views progress in a monotonically increasing fashion,
//! i.e., from a smaller view, to a higher view. However, the ways a replica can leave its current view
//! to enter the next view differ fundamentally depending on what kind of view the current view is.
//! After reading this documentation, you’ll understand how these different ways of leaving a view work
//! together to guarantee the safety and liveness of the Pacemaker subprotocol, as well as HotStuff-rs
//! as a whole.
//!
//! The following three subsections explain the different ways a replica can leave a view:
//! 1. [Leaving a Normal View](#leaving-a-normal-view) lists the two lightweight ways a replica can leave
//!    a normal view.
//! 2. [The need to vote for Epoch Changes](#the-need-to-vote-for-epoch-changes) explains why the
//!    lightweight mechanisms used to leave a normal view are not sufficient in the long run to maintain
//!    view synchronization even if it were initially achieved.
//! 3. [Leaving an Epoch-Change View](#leaving-an-epoch-change-view) picks up from the previous subsection
//!    and describes the heavyweight way a replica can leave an Epoch-Change view, which allows for view
//!    synchronization to be recovered in case it was lost.
//!
//! ### Leaving a Normal View
//!
//! A replica leaves a normal view when one of the following two things happen:
//! 1. It receives a valid `AdvanceView` message containing a
//!    [`PhaseCertificate`](crate::hotstuff::types::PhaseCertificate) for the current view or higher.
//! 2. It has spent longer than a predetermined amount of time in the view (i.e., when its local timer
//!    times out).
//!
//! Both mechanisms for leaving a normal view are "lightweight" in the sense that neither requires a
//! dedicated voting round. The first mechanism reuses `PhaseCertificate`s produced by the HotStuff
//! subprotocol in order to advance to the next view as fast as consensus decisions happen (thereby
//! achieving Optimistic Responsiveness), while the second mechanism serves as a fallback in case the
//! leader of the current view fails to make progress in time (which could happen if the current leader
//! is Byzantine).
//!
//! ### The need to vote for Epoch Changes
//!
//! Recall our assumption that replicas do not have synchronized clocks. This means that if we do not
//! intervene, replicas' local clocks will slowly but surely go out of sync. This progressive
//! desynchronization is manageable as long as the HotStuff subprotocol continues to make new consensus
//! decisions, since in this case `AdvanceView` messages will continue to be delivered, acting as a
//! rudimentary block synchronization mechanism.
//!
//! Inevitably, however, the HotStuff subprotocol *will* fail to make progress for an extended period:
//! network connections will go down, replicas will fail, and messages will fail to get delivered. If
//! this period of no progress continues for long enough, replicas’ clocks will eventually get so out of
//! sync that there will no longer be a quorum of validators currently in any single view. Consensus
//! decisions beyond this point will become impossible, and the blockchain will stop growing forever.
//!
//! To prevent views from getting *too* out of sync, and to help restore view synchronization after it
//! has been lost, leaving an Epoch-Change View after it has timed out requires an explicit voting step,
//! as described in the next section.
//!
//! ### Leaving an Epoch-Change View
//!
//! Just like with Normal Views, a replica still leaves an Epoch-Change view upon receiving a valid
//! `AdvanceView` message containing a `PhaseCertificate` for the current view or higher. The difference
//! between the procedures for leaving the two different kinds of views has to do with what happens when
//! a replica times out in an Epoch-Change View.
//!
//! When a validator times out in an Epoch-Change View, it does not immediately move to the next view.
//! Instead, it broadcasts a `TimeoutVote` message to all `N` leaders of the next epoch, and then waits.
//!
//! Concurrently, when a leader of the next epoch receives a `TimeoutVote`, it buffers it temporarily.
//! Once it has managed to buffer a set of `TimeoutVote`s for the same view from a quorum of replicas in
//! an [active validator set](crate::types::signed_messages::ActiveCollectorPair#active-validator-sets),
//! it collects them in a [`TimeoutCertificate`](types::TimeoutCertificate) and broadcasts it to every
//! replica.
//!   
//! Upon receiving a valid `TimeoutCertificate` for the current view or a higher view, a replica waiting
//! at the end of an Epoch-Change View re-broadcasts the `TimeoutCertificate` to all other replicas and
//! then moves to the view that follows immediately after the view indicated in the certificate.
//!
//! The idea behind this mechanism is that if there is no view which currently contains a quorum of
//! validators, replicas will eventually stop and  "park" themselves in the same epoch-change view and
//! get going again when there's again a quorum of validators in the view ready to proceed.
//!
//! ## Modifications on Lewis-Pye (2021)
//!
//! This module does not implement the protocol specified in Lewis-Pye (2021) exactly, but instead makes
//! a few subtle modifications. These modifications are described in the subsections below alongside the
//! justification for each.
//!
//! ### Modification 1: Terminological differences
//!
//! We call the "epoch e" messages defined in Lewis-Pye (2021) `TimeoutVote`s, and the
//! "EpochCertificate" type as `TimeoutCertificate`s.
//!
//! This is so that the terminology used in the Pacemaker subprotocol aligns better with the terminology
//! used in the HotStuff subprotocol, in which [`PhaseVote`](crate::hotstuff::messages::PhaseVote)s are
//! aggregated into [`PhaseCertificate`](crate::hotstuff::types::PhaseCertificate)s.
//!
//! ### Modification 2: User-configurable epoch length
//!
//! A key part of Lewis-Pye (2021)'s correctness is the property that at **at least 1** validator in any
//! in any epoch is honest (non-Byzantine).
//!
//! Lewis-Pye (2021) guarantees this under **static validator sets** by setting the length of epochs
//! (`N`) to be `f+1`, where `f` is the maximum number of validators that can be Byzantine at any
//! moment. This makes it so that even in the "worst case" where all `f` faulty validators are selected
//! to be leaders of the same epoch, said epoch will still contain `1` honest leader.
//!
//! With **dynamic validator sets**, which HotStuff-rs
//! [supports](crate::app#two-app-mutable-states-app-state-and-validator-set), `f` ceases to be a
//! constant. To guarantee the property that at least 1 validator in any epoch is honest under this
//! setting, two options come into mind:
//! 1. Make the Pacemaker somehow dynamically change epoch lengths.
//! 2. Make the epoch length user-configurable, and advise the user to set it to be a large enough value.
//!
//! HotStuff-rs' Pacemaker goes with the **second option** for for completely pragmatic reasons: if we
//! had gone for the first option, we’d have to create an algorithm that speculates not only about
//! dynamically-changing validator sets, but also dynamically-changing epoch lengths, and intuitively,
//! it seems difficult to create one that satisfies safety and liveness while still remaining simple.
//!
//! A reasonable choice for `N` could be the expected maximum number of validators throughout the
//! lifetime of the blockchain. This is large enough that `f` should remain well below `N` even if the
//! number of validators in the network exceeds expectations, and small enough that it does not affect
//! the asymptotic complexities of the algorithm.
//!
//! ### Modification 3: `AdvanceView` replaces View Certificates (VCs)
//!
//! The "optimistic" mechanism for leaving a normal view in Lewis-Pye (2021) involves a dedicated voting
//! round where replicas send a "view v" message to the leader of view v, and then waits to receive a
//! View Certificate (VC) from said leader.
//!
//! The [corresponding mechanism](#leaving-a-normal-view) in HotStuff-rs replaces VCs with a new message
//! type called `AdvanceView`. These contain existing `PhaseCertificate`s produced by the HotStuff
//! subprotocol.
//!
//! This optimization is sound because the whole purpose of `ViewCertificate`s is to indicate that a
//! quorum of validators are "done" with a specific view; and a `PhaseCertificate` serve this purpose
//! just as well, and without requiring an extra voting round.
//!
//! ### Modification 4: TimeoutVote includes the `highest_tc`
//!
//! In Lewis-Pye (2021), [Epoch-Change views end](#leaving-an-epoch-change-view) with a leader of the
//! next epoch broadcasting a newly-collected `TimeoutCertificate`.
//!
//! `TimeoutCertificate`s are also broadcasted at the end of an Epoch-Change view in HotStuff-rs's
//! Pacemaker subprotocol, but are also **additionally** sent out in another message type:
//! [`TimeoutVote`](messages::TimeoutVote)s. Specifically, `TimeoutVote` has a
//! [`highest_tc`](messages::TimeoutVote::highest_tc) field not present in Lewis-Pye (2021). This is
//! set by the sending validator to be the `TimeoutCertificate` with the highest view number it knows
//! of.
//!
//! Theoretically speaking, including the `highest_tc` in `TimeoutVote`s is unnecessary: in the
//! partially synchronous network model that Lewis-Pye (2021) assumes, message deliveries can be delayed
//! arbitrarily before Global Stabilization Time (GST), but always eventually get delivered, and
//! therefore the all-to-all broadcast of `TimeoutCertificate`s that happen at the end of every
//! Epoch-Change View should be sufficient to keep a quorum of replicas up-to-date about the current
//! epoch.
//!
//! Practically, however, network infrastructure does occasionally suffer catastrophic failures that
//! cause the vast majority of messages between replicas to be lost. `highest_tc` helps Pacemaker be
//! robust against these kinds of failures by giving replicas a "second chance" to receive a
//! `TimeoutCertificate` if they had missed it during an all-to-all broadcast.
//!
//! # Leader Selection
//!
//! The second of the two functionalities that Pacemaker provides to HotStuff-rs is called Leader
//! Selection.
//!
//! The basic requirement that a Leader Selection Mechanism for a generic SMR algorithm must meet
//! is that it must provide a mapping from *Views to Leaders*, i.e., a function with signature
//! `ViewNumber -> VerifyingKey`.
//!
//! Leader Selection for HotStuff-rs, however, comes with three additional requirements:
//! 1. **Support for Dynamic Validator Sets**: the validator set in that replicates a HotStuff-rs
//!    blockchain can [change
//!    dynamically](crate::app#two-app-mutable-states-app-state-and-validator-set) according to the
//!    application's instructions. This means that the function that the Leader Selection mechanism
//!    provides must have the signature `(ViewNumber, ValidatorSet) -> VerifyingKey`.
//! 2. **Frequent rotation**: if a Byzantine replica were to become leader for an extended, continuous
//!    sequence of views, it could harm the network in a variety of ways: ranging from the immediately
//!    apparent (e.g., not proposing any blocks, thereby preventing the blockchain from growing), to
//!    the more insiduous (e.g., refusing to include transactions from specific clients in the blocks
//!    it proposes, i.e., censoring them). To prevent these kinds of service disruptions, our leader
//!    selection mechanism should ensure that leadership of the validator set is rotated among all
//!    validators, ensuring that a Byzantine replica cannot be leader for too long.
//! 3. **Weighted selection**: In most popular Proof of Stake (PoS) blockchain systems, a validator’s stake
//!    determines not only the weight of its votes in consensus decisions but also likelihood of being
//!    selected as the leader for a given view. Likewise, we would like our leader selection mechanism to
//!    generate a leader sequence where each validator appears with a frequency proportional to its Power
//!    relative to the TotalPower of the validator set (i.e., if a validator makes up roughly X% of the
//!    validator set’s total power, then it should be the leader of approximately X% of views).
//!
//! To meet these requirements, Pacemaker's leader selection implementation
//! ([`select_leader`](implementation::select_leader)) uses the Interleaved Weighted Round-Robin
//! ([Interleaved WRR](https://en.wikipedia.org/wiki/Weighted_round_robin#Interleaved_WRR)) algorithm.

pub mod messages;

pub(crate) mod implementation;

pub mod types;
