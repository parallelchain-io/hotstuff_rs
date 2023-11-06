/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions of hotstuff-rs events for user-defined event handling and for logging.
//! Hotstuff-rs events refer to the high-level descriptions of actions that are expected of
//! a replica correctly executing the protocol.
//! 
//! The [event](Event) enum is a wrapper enum for the all event types defined in this module.
//! The event types are structs, storing all necessary information about a given event. 
//! This information always includes a timestamp corresponding to the exact time when the event happened.
//! 
//! Event notifications are sent from the [algorithm](crate::algorithm) and [sync_server](crate::sync_server)
//! threads and received and processed by the [event_bus](crate::event_bus) thread, according to the
//! predefined handlers.
//! 
//! Additionally, a [convenience method](Event::publish) is defined for publishing an event by sending it
//! via an appropriate channel.

use std::time::{SystemTime, Duration};
use std::sync::mpsc::Sender;

use crate::types::{Block, QuorumCertificate, CryptoHash, ViewNumber, ValidatorSetUpdates, VerifyingKey};
use crate::messages::{Proposal, Nudge, Vote, NewView};

/// Enumerates all events defined for hotstuff-rs.
pub enum Event {
    // Events that change persistent state.
    InsertBlock(InsertBlockEvent),
    CommitBlock(CommitBlockEvent),
    PruneBlock(PruneBlockEvent),
    UpdateHighestQC(UpdateHighestQCEvent),
    UpdateLockedView(UpdateLockedViewEvent),
    UpdateValidatorSet(UpdateValidatorSetEvent),
    // Events that involve broadcasting/sending a Progress Message.
    Propose(ProposeEvent),
    Nudge(NudgeEvent),
    Vote(VoteEvent),
    NewView(NewViewEvent),
    // Events that involve receiving a Progress Message.
    ReceiveProposal(ReceiveProposalEvent),
    ReceiveNudge(ReceiveNudgeEvent),
    ReceiveVote(ReceiveVoteEvent),
    ReceiveNewView(ReceiveNewViewEvent),
    // Progress mode events.
    StartView(StartViewEvent),
    ViewTimeout(ViewTimeoutEvent),
    CollectQC(CollectQCEvent),
    // Sync mode events.
    StartSync(StartSyncEvent),
    EndSync(EndSyncEvent),
    ReceiveSyncRequest(ReceiveSyncRequestEvent),
    SendSyncResponse(SendSyncResponseEvent),
}

impl Event {
    /// Publishes a given instance of the [Event](Event) enum on the event publisher channel (if the channel is defined).
    pub(crate) fn publish(self, event_publisher: &Option<Sender<Event>>) {
        if let Some(event_publisher) = event_publisher {
            let _ = event_publisher.send(self);
        }
    }
}

/// This event corresponds to the replica inserting a new block into its [Block Tree](crate::state::BlockTree).
/// The event includes all information about the block contained in the [Block](crate::types::Block) struct.
pub struct InsertBlockEvent {
    pub timestamp: SystemTime, 
    pub block: Block,
}

/// This event corresponds to the replica committing a block, identifiable by its [hash](crate::types::CryptoHash),
/// and hence writing the changes associated with committing the block to its [Block Tree](crate::state::BlockTree).
pub struct CommitBlockEvent {
    pub timestamp: SystemTime,
    pub block: CryptoHash,
}

/// This event corresponds to the replica pruning a block, identifiable by its [hash](crate::types::CryptoHash),
/// i.e., deleting the block's siblings from its [Block Tree](crate::state::BlockTree).
pub struct PruneBlockEvent {
    pub timestamp: SystemTime,
    pub block: CryptoHash,
}

/// This event corresponds to the replica updating the Highest Quroum Certificate stored in its [Block Tree](crate::state::BlockTree),
/// and includes the new Highest [Quroum Certificate](crate::types::QuorumCertificate).
pub struct UpdateHighestQCEvent {
    pub timestamp: SystemTime,
    pub highest_qc: QuorumCertificate,
}

/// This event corresponds to the replica updating the Locked View stored in its [Block Tree](crate::state::BlockTree),
/// and includes the [view number](crate::types::ViewNumber) of the new Locked View.
pub struct UpdateLockedViewEvent {
    pub timestamp: SystemTime,
    pub locked_view: ViewNumber,
}

/// This event corresponds to the replica updating the Committed Validator Set stored in its [Block Tree](crate::state::BlockTree).
/// The event includes the [hash](crate::types::CryptoHash) of the block with which the updates are associated, and the information
/// about the [validator set updates](crate::types::ValidatorSetUpdates), i.e., the insertions and deletions relative to the 
/// previous Committed Validator Set.
pub struct UpdateValidatorSetEvent {
    pub timestamp: SystemTime,
    pub cause_block: CryptoHash,
    pub validator_set_updates: ValidatorSetUpdates,
}

/// This event corresponds to the replica proposing a new block by broadcasting a [proposal](crate::messages::Proposal) to all
/// validators.
pub struct ProposeEvent {
    pub timestamp: SystemTime,
    pub proposal: Proposal,
}

/// This event corresponds to the replica nudging for a block by broadcasting a [nudge](crate::messages::Nudge) to all
/// validators.
pub struct NudgeEvent {
    pub timestamp: SystemTime,
    pub nudge: Nudge,
}

/// This event corresponds to the replica voting for a block by sending a [vote](crate::messages::Vote) to the leader.
pub struct VoteEvent {
    pub timestamp: SystemTime,
    pub vote: Vote,
}

/// This event corresponds to the replica sending a [new view](crate::messages::NewView) message to the leader upon moving to
/// a new view.
pub struct NewViewEvent {
    pub timestamp: SystemTime,
    pub new_view: NewView,
}

/// This event corresponds to the replica receiving a [proposal](crate::messages::Proposal) from the leader identified
/// by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveProposalEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub proposal: Proposal,
}

/// This event corresponds to the replica receiving a [nudge](crate::messages::Nudge) from the leader identified
/// by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveNudgeEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub nudge: Nudge,
}

/// This event corresponds to the replica receiving a [vote](crate::messages::Vote) from another replica identified
/// by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveVoteEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub vote: Vote,
}

/// This event corresponds to the replica receiving a [new view](crate::messages::NewView) message from another replica
/// identified by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveNewViewEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub new_view: NewView,
}

/// This event corresponds to the replica starting a new view with a given [view number](crate::types::ViewNumber) and
/// a given leader identified by its [public key](ed25519_dalek::VerifyingKey).
pub struct StartViewEvent {
    pub timestamp: SystemTime,
    pub leader: VerifyingKey,
    pub view: ViewNumber,
}

/// This event corresponds to the replica's view timeout for a view with a given [view number](crate::types::ViewNumber)
/// after spending a given [amount of time](core::time::Duration) in the view.
pub struct ViewTimeoutEvent {
    pub timestamp: SystemTime,
    pub view: ViewNumber,
    pub timeout: Duration,
}

/// This event corresponds to the replica collecting a new [Quorum Certificate](crate::types::QuorumCertificate)
/// from the votes it received from the validators.
pub struct CollectQCEvent {
    pub timestamp: SystemTime,
    pub quorum_certificate: QuorumCertificate,
}

/// This event corresponds to the replica entering the sync mode and trying to sync with a given
/// peer identified by its [public key](ed25519_dalek::VerifyingKey).
pub struct StartSyncEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
}

/// This event corresponds to the replica exiting the sync mode, during which it tried to sync with 
/// a given peer identified by its [public key](ed25519_dalek::VerifyingKey), and inserted a given
/// number of blocks received from the peer into its [Block Tree](crate::state::BlockTree).
pub struct EndSyncEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
    pub blocks_synced: u64,
}

/// This event corresponds to the replica's [sync_server](crate::sync_server) receiving a [sync request](crate::messages::SyncRequest)
/// from a peer identified by its [public key](ed25519_dalek::VerifyingKey). The event includes information
/// about the requested start height from which the peer wants to sync, and the limit on the number of blocks
/// that can be sent in a [sync response](crate::messages::SyncResponse).
pub struct ReceiveSyncRequestEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
    pub start_height: u64,
    pub limit: u32,
}

/// This event corresponds to the replica's [sync_server](crate::sync_server) sending a [sync response](crate::messages::SyncResponse)
/// to a peer identified by its [public key](ed25519_dalek::VerifyingKey). The event includes information
/// about the vector of [blocks](crate::types::Block) and the Highest [Quroum Certificate](crate::types::QuorumCertificate) sent to the peer.
pub struct SendSyncResponseEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
    pub blocks: Vec<Block>,
    pub highest_qc: QuorumCertificate,
}