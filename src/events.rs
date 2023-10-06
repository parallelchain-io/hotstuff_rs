use std::time::{SystemTime, Duration};

use crate::{types::*, messages::{Proposal, Nudge, Vote, NewView}};

pub enum Event {
    InsertBlock(InsertBlockEvent),
    CommitBlock(CommitBlockEvent),
    PruneBlock(PruneBlockEvent),
    Propose(ProposeEvent),
    ReceiveProposal(ReceiveProposalEvent),
    Nudge(NudgeEvent),
    ReceiveNudge(ReceiveNudgeEvent),
    Vote(VoteEvent),
    ReceiveVote(ReceiveVoteEvent),
    NewView(NewViewEvent),
    ReceiveNewView(ReceiveNewViewEvent),
    StartView(StartViewEvent),
    ViewTimeout(ViewTimeoutEvent),
    StartSync(StartSyncEvent),
    EndSync(EndSyncEvent),
}

pub struct InsertBlockEvent {
    pub timestamp: SystemTime, 
    pub block: Block,
}

pub struct CommitBlockEvent {
    pub timestamp: SystemTime,
    pub block: Block,
}

pub struct PruneBlockEvent {
    pub timestamp: SystemTime,
    pub block: Block,
}

pub struct ProposeEvent {
    pub timestamp: SystemTime,
    pub proposal: Proposal,
}

pub struct ReceiveProposalEvent {
    pub timestamp: SystemTime,
    pub origin: PublicKey,
    pub proposal: Proposal,
}

pub struct NudgeEvent {
    pub timestamp: SystemTime,
    pub nudge: Nudge,
}

pub struct ReceiveNudgeEvent {
    pub timestamp: SystemTime,
    pub origin: PublicKey,
    pub nudge: Nudge,
}

pub struct VoteEvent {
    pub timestamp: SystemTime,
    pub vote: Vote,
}

pub struct ReceiveVoteEvent {
    pub timestamp: SystemTime,
    pub origin: PublicKey,
    pub vote: Vote,
}

pub struct NewViewEvent {
    pub timestamp: SystemTime,
    pub new_view: NewView,
}

pub struct ReceiveNewViewEvent {
    pub timestamp: SystemTime,
    pub origin: PublicKey,
    pub new_view: NewView,
}

pub struct StartViewEvent {
    pub timestamp: SystemTime,
    pub view: ViewNumber,
}

pub struct ViewTimeoutEvent {
    pub timestamp: SystemTime,
    pub view: ViewNumber,
    pub timeout: Duration,
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