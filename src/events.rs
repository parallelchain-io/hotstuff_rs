/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Subscribable events that are published when significant things happen in the replica.
//!
//! ## Event enum
//!
//! Significant occurences in the replica include committing a block, starting a new view, broadcasting
//! a proposal, or receiving a proposal.
//!
//! Each of these significant occurences correspond to a variant of the [event enum](Event). Each
//! variant tuple in turn contains an inner struct type. For example, the
//! [insert block variant](Event::InsertBlock) contains the [`InsertBlockEvent`] struct type.
//!
//! Each inner struct stores information that summarizes the particular kind of event. This information
//! always includes a timestamp corresponding to the exact time when the event occured.
//!
//! ## Registering event handlers
//!
//! Library users can register event handler closures, these are internally called by the library's
//! [event bus](crate::event_bus::start_event_bus) thread when the handler's particular event variant
//! happens.
//!
//! Custom event handlers can be registered using the [replica builder pattern](crate::replica), while
//! default event handlers that log out events (e.g., into the terminal, or into a log file) can be
//! enabled in the [configuration](crate::replica::Configuration).
//!
//! ## Timing
//!
//! Events are always emitted **after** the corresponding occurence is "completed". So for example, the
//! [insert block event](InsertBlockEvent) is only emitted after the insertion has been persisted into
//! the backing storage of the block tree.

use std::{sync::mpsc::Sender, time::SystemTime};

use ed25519_dalek::VerifyingKey;

use crate::{
    hotstuff::{
        messages::{NewView, Nudge, PhaseVote, Proposal},
        types::PhaseCertificate,
    },
    pacemaker::{
        messages::{AdvanceView, TimeoutVote},
        types::TimeoutCertificate,
    },
    types::{
        block::Block,
        data_types::{BlockHeight, CryptoHash, ViewNumber},
        update_sets::ValidatorSetUpdates,
    },
};

/// Enumerates all events defined for HotStuff-rs.
pub enum Event {
    // Events that change persistent state.
    InsertBlock(InsertBlockEvent),
    CommitBlock(CommitBlockEvent),
    PruneBlock(PruneBlockEvent),
    UpdateHighestPC(UpdateHighestPCEvent),
    UpdateLockedPC(UpdateLockedPCEvent),
    UpdateHighestTC(UpdateHighestTCEvent),
    UpdateValidatorSet(UpdateValidatorSetEvent),

    // Events that involve broadcasting or sending a Progress Message.
    Propose(ProposeEvent),
    Nudge(NudgeEvent),
    PhaseVote(PhaseVoteEvent),
    NewView(NewViewEvent),
    TimeoutVote(TimeoutVoteEvent),
    AdvanceView(AdvanceViewEvent),

    // Events that involve receiving a Progress Message.
    ReceiveProposal(ReceiveProposalEvent),
    ReceiveNudge(ReceiveNudgeEvent),
    ReceivePhaseVote(ReceivePhaseVoteEvent),
    ReceiveNewView(ReceiveNewViewEvent),
    ReceiveTimeoutVote(ReceiveTimeoutVoteEvent),
    ReceiveAdvanceView(ReceiveAdvanceViewEvent),

    // Other progress mode events.
    StartView(StartViewEvent),
    ViewTimeout(ViewTimeoutEvent),
    CollectPC(CollectPCEvent),
    CollectTC(CollectTCEvent),

    // Sync mode events.
    StartSync(StartSyncEvent),
    EndSync(EndSyncEvent),
    ReceiveSyncRequest(ReceiveSyncRequestEvent),
    SendSyncResponse(SendSyncResponseEvent),
}

impl Event {
    /// Publishes a given instance of the [`Event`] enum on the event publisher channel (if the channel
    /// is defined).
    pub fn publish(self, event_publisher: &Option<Sender<Event>>) {
        if let Some(event_publisher) = event_publisher {
            let _ = event_publisher.send(self);
        }
    }
}

/// A new `block` was inserted into the Block Tree in a persistent manner.
pub struct InsertBlockEvent {
    pub timestamp: SystemTime,
    pub block: Block,
}

/// A `block` was committed. This involves persistent changes to the
/// Block Tree.
pub struct CommitBlockEvent {
    pub timestamp: SystemTime,
    pub block: CryptoHash,
}

/// A `block` was "pruned" (i.e., the block's siblings were permanently
/// deleted from the Block Tree).
pub struct PruneBlockEvent {
    pub timestamp: SystemTime,
    pub block: CryptoHash,
}

/// The "Highest `PhaseCertificate`" stored in the block tree was updated.
pub struct UpdateHighestPCEvent {
    pub timestamp: SystemTime,
    pub highest_pc: PhaseCertificate,
}

/// The "Locked `PhaseCertificate`" stored in the block tree was updated.
pub struct UpdateLockedPCEvent {
    pub timestamp: SystemTime,
    pub locked_pc: PhaseCertificate,
}

/// The "Highest `TimeoutCertificate`" stored in the block tree was updated.
pub struct UpdateHighestTCEvent {
    pub timestamp: SystemTime,
    pub highest_tc: TimeoutCertificate,
}

/// The "Committed Validator Set" stored in the block tree, was updated.
///
/// Includes the [hash](crate::types::data_types::CryptoHash) of the block with which the updates are
/// associated, and the information about the
/// [validator set updates](crate::types::update_sets::ValidatorSetUpdates), i.e., the insertions and
/// deletions relative to the previous committed validator set.
pub struct UpdateValidatorSetEvent {
    pub timestamp: SystemTime,
    pub cause_block: CryptoHash,
    pub validator_set_updates: ValidatorSetUpdates,
}

/// The replica proposed a block by broadcasting it as a [proposal](crate::hotstuff::messages::Proposal)
/// to all validators.
pub struct ProposeEvent {
    pub timestamp: SystemTime,
    pub proposal: Proposal,
}

/// The replica nudged for a block by broadcasting a [nudge](crate::hotstuff::messages::Nudge) for the
/// block to all validators.
pub struct NudgeEvent {
    pub timestamp: SystemTime,
    pub nudge: Nudge,
}

/// The replica voted for a phase of a block by sending `phase_vote` to a leader of the next view.
pub struct PhaseVoteEvent {
    pub timestamp: SystemTime,
    pub vote: PhaseVote,
}

/// The replica sent a [new view](crate::hotstuff::messages::NewView) message for its current view
/// to the leader of the next view upon moving to a new view.
pub struct NewViewEvent {
    pub timestamp: SystemTime,
    pub new_view: NewView,
}

/// The replica broadcasted an [advance view](crate::pacemaker::messages::AdvanceView) message to all
/// peers.
pub struct AdvanceViewEvent {
    pub timestamp: SystemTime,
    pub advance_view: AdvanceView,
}

/// The replica broadcasted a [timeout vote](crate::pacemaker::messages::TimeoutVote) message to all
/// peers.
pub struct TimeoutVoteEvent {
    pub timestamp: SystemTime,
    pub timeout_vote: TimeoutVote,
}

/// The replica received a `proposal` for the replica's current view from the leader of the view,
/// identified by its [`VerifyingKey`].
pub struct ReceiveProposalEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub proposal: Proposal,
}

/// The replica received a `nudge` for the replica's current view from the leader of the view,
/// identified by its [`VerifyingKey`].
pub struct ReceiveNudgeEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub nudge: Nudge,
}

/// The replica received a `phase_vote` for the replica's current view from the replica identified by
/// `origin`.
pub struct ReceivePhaseVoteEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub phase_vote: PhaseVote,
}

/// The replica received a [new view](crate::hotstuff::messages::NewView) message for the current view from
/// another replica identifiable by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveNewViewEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub new_view: NewView,
}

/// The replica received an `advance_view` message from another replica, identified by its
/// `verifying_key`.
pub struct ReceiveAdvanceViewEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub advance_view: AdvanceView,
}

/// The replica received a `timeout_certificate` for the replica's current view from another replica,
/// identified by `verifying_key`.
pub struct ReceiveTimeoutVoteEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub timeout_vote: TimeoutVote,
}

/// The replica started a new view with a given [`ViewNumber`].
pub struct StartViewEvent {
    pub timestamp: SystemTime,
    pub view: ViewNumber,
}

/// The replica's view, with a given [`ViewNumber`], timed out.
pub struct ViewTimeoutEvent {
    pub timestamp: SystemTime,
    pub view: ViewNumber,
}

/// The replica collected a new `phase_certificate` from the votes it received from the validators in
/// the current view.
pub struct CollectPCEvent {
    pub timestamp: SystemTime,
    pub phase_certificate: PhaseCertificate,
}

/// The replica collected a new `timeout_certificate` from the votes it received from the validators in
/// the current view.
pub struct CollectTCEvent {
    pub timestamp: SystemTime,
    pub timeout_certificate: TimeoutCertificate,
}

/// The replica entered sync mode and tried to sync with a given peer, identified by its
/// [`VerifyingKey`].
pub struct StartSyncEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
}

/// The replica exited sync mode, during which it tried to sync with a given peer identifiable by its
/// [public key](ed25519_dalek::VerifyingKey), and inserted a given number of blocks received from the
/// peer into its block tree.
pub struct EndSyncEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
    pub blocks_synced: u64,
}

/// The replica's [sync server](crate::block_sync::server::BlockSyncServer) received a
/// [sync request](crate::block_sync::messages::BlockSyncRequest) from a peer identifiable by its
/// [public key](ed25519_dalek::VerifyingKey). Includes information about the requested start height
/// from which the peer wants to sync, and the limit on the number of blocks that can be sent in a
/// [sync response](crate::block_sync::messages::BlockSyncResponse).
pub struct ReceiveSyncRequestEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
    pub start_height: BlockHeight,
    pub limit: u32,
}

/// The replica's [sync server](crate::block_sync::server::BlockSyncServer) sent a
/// [sync response](crate::block_sync::messages::BlockSyncResponse) to a peer identifiable by its
/// [public key](ed25519_dalek::VerifyingKey). Includes information about the vector of
/// [blocks](crate::types::block::Block) and the Highest
/// [Quroum Certificate](crate::hotstuff::types::PhaseCertificate) sent to the peer.
pub struct SendSyncResponseEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
    pub blocks: Vec<Block>,
    pub highest_pc: PhaseCertificate,
}
