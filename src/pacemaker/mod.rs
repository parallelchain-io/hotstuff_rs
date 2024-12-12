//! Subprotocol for Byzantine view synchronization and leader selection.
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
//! The Pacemaker [`implementation`] works so that views progress in a monotonically increasing fashion:
//! from a smaller view, to a higher view. However, the ways a replica can leave its current view to
//! enter the next view differ fundamentally depending on what kind of view the current view is. After
//! reading this documentation, you’ll understand how these different ways of leaving a view work
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
//!
//!
//! ### Terminological differences
//!
//!
//!
//! 1. "epoch e" -> TimeoutVote
//! 2. Epoch Certificate -> TimeoutCertificate
//!
//! ### Configurable, constant epoch lengths
//!
//! - It's important to the safety and liveness guarantees of the Lewis-Pye view synchronization protocol for **at least 1** validator in any epoch to be an honest (i.e., non-faulty)
//! replica.
//! - With static validator sets, this is achieved by setting the length of epochs to be f+1. This means that even in the worst case where all f faulty replicas are selected to be leaders in an epoch, (f + 1) - f = 1 replica in the epoch will be safe.
//! - With dynamic validator sets, "f" is not a constant. With this in mind, we need to choose between two options:
//!     1. Make pacemaker dynamically change epoch lengths.
//!     2. Set the epoch length to be a large constant.
//!
//! A reasonable O(n) choice should not impair asymptotic complexity since O(f) = O(n).
//!
//! ### `AdvanceView` replaces View Certificates (VCs)
//!
//! - In the original Lewis-Pye 2021, there are two ways a replica can leave a view to enter a higher view: optimistic path and pessimistic path.
//! - We still have the pessimistic path, but we optimize the optimistic path by not requiring a RTT to collect a view certificate.
//! - The insight here is that the decision that a View Certificate represents is subsumed by the decision that Phase Certificate represents.
//! - We don't need a separate round of voting to produce a VC. We can just use the Phase Certificate.
//! - We include this PhaseCertificate in AdvanceView.
//!
//! ### TimeoutVote includes the `highest_tc`
//!
//! - In Lewis-Pye 2021, the leaders of the next view broadcast the TimeoutCertificate that they collect to everyone, bringing everyone to the same epoch.
//! - This is guaranteed to work under the theoretical setting of Lewis-Pye (which is a partially synchronous network).
//! - In the assumed network setting, messages can be arbitrarily delayed, but will eventually be received.
//! - Practically however, internet infrastructure sometimes falls apart and messages can be lost.
//! - To make the protocol robust to these kinds of failures, we add the highest TC into timeoutvote as well.
//! - This way even if there are replicas who miss the timeout certificate broadcast ending a view, they can receive it later through a timeoutvote and end up in the same epoch as everyone.
//!
//! # Leader Selection
//!
//! Leaders are selected according to Interleaved Weighted Round Robin algorithm. This ensures that:
//! 1. The frequency with which a validator is selected as a leader is proportional to the validator's
//!    power,
//! 2. Validators are selected as leaders in an interleaved manner: unless a validator has more power
//!    than any other validator, it will never act as a leader for more than one consecutive view.

pub mod messages;

pub(crate) mod implementation;

pub mod types;
