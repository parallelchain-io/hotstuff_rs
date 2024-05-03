/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Notifications that are emitted when significant things happen in the local HotStuff-rs replica.
//! 
//! ## Event enum
//! 
//! Significant occurences in the replica include committing a block, starting a new view, broadcasting
//! a proposal, or receiving a proposal.
//! 
//! Each of these significant occurences correspond to a variant of the [event enum](Event). Each 
//! variant tuple in turn contains an inner struct type. For example, the 
//! [insert block variant](Event::InsertBlock) contains the [InsertBlockEvent] struct type.
//! 
//! Each inner struct stores information that summarizes the particular kind of event. This information
//! always includes a timestamp corresponding to the exact time when the event occured.
//! 
//! ## Registering event handlers
//! 
//! Library users can register event handler closures, which are then internally called by the library's 
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

use std::time::{SystemTime, Duration};
use std::sync::mpsc::Sender;

use ed25519_dalek::VerifyingKey;

use crate::hotstuff::messages::{NewView, Nudge, Proposal, Vote};
use crate::hotstuff::types::QuorumCertificate;
use crate::pacemaker::messages::{AdvanceView, TimeoutVote};
use crate::pacemaker::types::TimeoutCertificate;
use crate::types::basic::{CryptoHash, ViewNumber};
use crate::types::block::Block;
use crate::types::validators::ValidatorSetUpdates;

/// Enumerates all events defined for HotStuff-rs.
pub enum Event {
    // Events that change persistent state.
    InsertBlock(InsertBlockEvent),
    CommitBlock(CommitBlockEvent),
    PruneBlock(PruneBlockEvent),
    UpdateHighestQC(UpdateHighestQCEvent),
    UpdateLockedQC(UpdateLockedQCEvent),
    UpdateHighestTC(UpdateHighestTCEvent),
    UpdateValidatorSet(UpdateValidatorSetEvent),

    // Events that involve broadcasting or sending a Progress Message.
    Propose(ProposeEvent),
    Nudge(NudgeEvent),
    Vote(VoteEvent),
    NewView(NewViewEvent),
    TimeoutVote(TimeoutVoteEvent),
    AdvanceView(AdvanceViewEvent),

    // Events that involve receiving a Progress Message.
    ReceiveProposal(ReceiveProposalEvent),
    ReceiveNudge(ReceiveNudgeEvent),
    ReceiveVote(ReceiveVoteEvent),
    ReceiveNewView(ReceiveNewViewEvent),
    ReceiveTimeoutVote(ReceiveTimeoutVoteEvent),
    ReceiveAdvanceView(ReceiveAdvanceViewEvent),

    // Other progress mode events.
    StartView(StartViewEvent),
    ViewTimeout(ViewTimeoutEvent),
    CollectQC(CollectQCEvent),
    CollectTC(CollectTCEvent),

    // Sync mode events.
    StartSync(StartSyncEvent),
    EndSync(EndSyncEvent),
    ReceiveSyncRequest(ReceiveSyncRequestEvent),
    SendSyncResponse(SendSyncResponseEvent),
}

impl Event {
    /// Publishes a given instance of the [Event](Event) enum on the event publisher channel (if the channel
    /// is defined).
    pub fn publish(self, event_publisher: &Option<Sender<Event>>) {
        if let Some(event_publisher) = event_publisher {
            let _ = event_publisher.send(self);
        }
    }
}

/// A new block was inserted into the [Block Tree](crate::state::BlockTree) in a persistent manner.
/// Includes all information about the insrted block contained in the
/// [Block](crate::types::block::Block) struct.
pub struct InsertBlockEvent {
    pub timestamp: SystemTime, 
    pub block: Block,
}

/// A block, identifiable by its [hash](crate::types::CryptoHash), was committed.
/// This involves persistent changes to the [Block Tree](crate::state::BlockTree).
pub struct CommitBlockEvent {
    pub timestamp: SystemTime,
    pub block: CryptoHash,
}

/// A block, identifiable by its [hash](crate::types::CryptoHash), was pruned,
/// i.e., the block's siblings were permanently deleted from the [Block Tree](crate::state::BlockTree).
pub struct PruneBlockEvent {
    pub timestamp: SystemTime,
    pub block: CryptoHash,
}
/// The Highest Quroum Certificate, stored in the [Block Tree](crate::state::block_tree::BlockTree), 
/// was updated. Includes the new Highest [Quroum Certificate](crate::hotstuff::types::QuorumCertificate).
pub struct UpdateHighestQCEvent {
    pub timestamp: SystemTime,
    pub highest_qc: QuorumCertificate,
}

/// The Locked QC stored in the [Block Tree](crate::state::BlockTree) was updated.
/// Includes the new locked [quorum certificate](crate::hotstuff::types::QuorumCertificate).
pub struct UpdateLockedQCEvent {
    pub timestamp: SystemTime,
    pub locked_qc: QuorumCertificate,
}

/// The Highest Timeout Certificate, stored in the [Block Tree](crate::state::block_tree::BlockTree), 
/// was updated. Includes the new Highest [Timeout Certificate](crate::pacemaker::types::TimeoutCertificate).
pub struct UpdateHighestTCEvent {
    pub timestamp: SystemTime,
    pub highest_tc: TimeoutCertificate,
}

/// The committed validator set, stored in the [Block Tree](crate::state::BlockTree), was updated.
/// Includes the [hash](crate::types::basic::CryptoHash) of the block with which the updates are
/// associated, and the information about the 
/// [validator set updates](crate::types::validators::ValidatorSetUpdates), i.e., the insertions and
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

/// The replica voted for a block by sending a [vote](crate::hotstuff::messages::Vote) to the leader of
/// the next view.
pub struct VoteEvent {
    pub timestamp: SystemTime,
    pub vote: Vote,
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

/// The replica received a [proposal](crate::hotstuff::messages::Proposal) for the replica's current view from the
/// leader of the view, identifiable by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveProposalEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub proposal: Proposal,
}

/// The replica received a [nudge](crate::hotstuff::messages::Nudge) for the replica's current view from the
/// leader of the view, identifiable by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveNudgeEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub nudge: Nudge,
}

/// The replica received a [vote](crate::hotstuff::messages::Vote) for the replica's current view from another
/// replica identifiable by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveVoteEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub vote: Vote,
}

/// The replica received a [new view](crate::hotstuff::messages::NewView) message for the current view from
/// another replica identifiable by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveNewViewEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub new_view: NewView,
}

/// The replica received an [adavance view](crate::pacemaker::messages::AdvanceView) message from
/// another replica identifiable by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveAdvanceViewEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub advance_view: AdvanceView,
}

/// The replica received a [timeout vote](crate::pacemaker::messages::TimeoutVote) for the replica's
/// current view from another replica identifiable by its [public key](ed25519_dalek::VerifyingKey).
pub struct ReceiveTimeoutVoteEvent {
    pub timestamp: SystemTime,
    pub origin: VerifyingKey,
    pub timeout_vote: TimeoutVote,
}

/// The replica started a new view with a given [view number](crate::types::basic::ViewNumber) and
/// a given leader identifiable by its [public key](ed25519_dalek::VerifyingKey).
pub struct StartViewEvent {
    pub timestamp: SystemTime,
    pub view: ViewNumber,
}

/// The replica's view, with a given [view number](crate::types::basic::ViewNumber), timed out after 
/// a given [amount of time](core::time::Duration).
pub struct ViewTimeoutEvent {
    pub timestamp: SystemTime,
    pub view: ViewNumber,
    pub timeout: Duration,
}

/// The replica collected a new [Quorum Certificate](crate::hotstuff::types::QuorumCertificate) from the
/// votes it received from the validators in the current view.
pub struct CollectQCEvent {
    pub timestamp: SystemTime,
    pub quorum_certificate: QuorumCertificate,
}

/// The replica collected a new [Timeout Certificate](crate::pacemaker::types::TimeoutCertificate) from
/// the votes it received from the validators in the current view.
pub struct CollectTCEvent {
    pub timestamp: SystemTime,
    pub timeout_certificate: TimeoutCertificate,
}


/// The replica entered sync mode and tried to sync with a given peer identifiable by its
/// [public key](ed25519_dalek::VerifyingKey).
pub struct StartSyncEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
}

/// The replica exited sync mode, during which it tried to sync with a given peer identifiable by its 
/// [public key](ed25519_dalek::VerifyingKey), and inserted a given number of blocks received from the
/// peer into its [Block Tree](crate::state::BlockTree).
pub struct EndSyncEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
    pub blocks_synced: u64,
}

/// The replica's [sync server](crate::sync_server) received a 
/// [sync request](crate::block_sync::messages::BlockSyncRequest) from a peer identifiable by its 
/// [public key](ed25519_dalek::VerifyingKey). Includes information about the requested start height 
/// from which the peer wants to sync, and the limit on the number of blocks that can be sent in a 
/// [sync response](crate::block_sync::messages::BlockSyncResponse).
pub struct ReceiveSyncRequestEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
    pub start_height: u64,
    pub limit: u32,
}

/// The replica's [sync server](crate::block_sync::server::BlockSyncServer) sent a 
/// [sync response](crate::block_sync::messages::BlockSyncResponse) to a peer identifiable by its 
/// [public key](ed25519_dalek::VerifyingKey). Includes information about the vector of 
/// [blocks](crate::types::block::Block) and the Highest 
/// [Quroum Certificate](crate::hotstuff::types::QuorumCertificate) sent to the peer.
pub struct SendSyncResponseEvent {
    pub timestamp: SystemTime,
    pub peer: VerifyingKey,
    pub blocks: Vec<Block>,
    pub highest_qc: QuorumCertificate,
}