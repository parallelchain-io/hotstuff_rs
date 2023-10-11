//! Definitions of hotstuff-rs events for event handling and logging
//! Note: an event for a given action indicates that the action has been completed

use crate::types::{Block, QuorumCertificate, CryptoHash, ViewNumber, ValidatorSetUpdates, PublicKey};
use crate::messages::{Proposal, Nudge, Vote, NewView};
use std::time::{SystemTime, Duration};
use std::sync::mpsc::Sender;

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
    pub(crate) fn publish(event_publisher: &Option<Sender<Event>>, event: Event) {
        if let Some(event_publisher) = event_publisher {
            event_publisher.send(event).unwrap()
        }
    }
}

pub struct InsertBlockEvent {
    pub timestamp: SystemTime, 
    pub block: Block,
}

pub struct CommitBlockEvent {
    pub timestamp: SystemTime,
    pub block: CryptoHash,
}

pub struct PruneBlockEvent {
    pub timestamp: SystemTime,
    pub block: CryptoHash,
}

pub struct UpdateHighestQCEvent {
    pub timestamp: SystemTime,
    pub highest_qc: QuorumCertificate,
}

pub struct UpdateLockedViewEvent {
    pub timestamp: SystemTime,
    pub locked_view: ViewNumber,
}

pub struct UpdateValidatorSetEvent {
    pub timestamp: SystemTime,
    pub cause_block: CryptoHash,
    pub validator_set_updates: ValidatorSetUpdates,
}

pub struct ProposeEvent {
    pub timestamp: SystemTime,
    pub proposal: Proposal,
}

pub struct NudgeEvent {
    pub timestamp: SystemTime,
    pub nudge: Nudge,
}

pub struct VoteEvent {
    pub timestamp: SystemTime,
    pub vote: Vote,
}

pub struct NewViewEvent {
    pub timestamp: SystemTime,
    pub new_view: NewView,
}

pub struct ReceiveProposalEvent {
    pub timestamp: SystemTime,
    pub origin: PublicKey,
    pub proposal: Proposal,
}

pub struct ReceiveNudgeEvent {
    pub timestamp: SystemTime,
    pub origin: PublicKey,
    pub nudge: Nudge,
}

pub struct ReceiveVoteEvent {
    pub timestamp: SystemTime,
    pub origin: PublicKey,
    pub vote: Vote,
}

pub struct ReceiveNewViewEvent {
    pub timestamp: SystemTime,
    pub origin: PublicKey,
    pub new_view: NewView,
}

pub struct StartViewEvent {
    pub timestamp: SystemTime,
    pub leader: PublicKey,
    pub view: ViewNumber,
}

pub struct ViewTimeoutEvent {
    pub timestamp: SystemTime,
    pub view: ViewNumber,
    pub timeout: Duration,
}

pub struct CollectQCEvent {
    pub timestamp: SystemTime,
    pub quorum_certificate: QuorumCertificate,
}

pub struct StartSyncEvent {
    pub timestamp: SystemTime,
    pub peer: PublicKey,
}

pub struct EndSyncEvent {
    pub timestamp: SystemTime,
    pub peer: PublicKey,
    pub blocks_synced: u64,
}

pub struct ReceiveSyncRequestEvent {
    pub timestamp: SystemTime,
    pub peer: PublicKey,
    pub start_height: u64,
    pub limit: u32,
}

pub struct SendSyncResponseEvent {
    pub timestamp: SystemTime,
    pub peer: PublicKey,
    pub blocks: Vec<Block>,
    pub highest_qc: QuorumCertificate,
}