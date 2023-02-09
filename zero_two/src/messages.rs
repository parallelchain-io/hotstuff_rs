use crate::types::*;

pub enum Message {
    ProgressMessage(ProgressMessage),
    SyncMessage(SyncMessage),
}

pub enum ProgressMessage {
    Proposal(Proposal),
    Vote(Vote)
}

impl ProgressMessage {
    pub fn view(&self) -> ViewNumber {
        match self {
            ProgressMessage::Proposal(Proposal::New { vote, .. }) => vote.view,
            ProgressMessage::Proposal(Proposal::Nudge { vote, .. }) => vote.view,
            ProgressMessage::Vote(Vote { view, .. }) => *view,
        }
    } 
}

pub enum Proposal {
    New {
        block: Block,
        vote: Vote,
    },
    Nudge {
        justify: QuorumCertificate,
        vote: Vote,
    }
}

pub struct Vote {
    view: ViewNumber,
    block: CryptoHash,
    phase: Phase,
}

pub enum SyncMessage {
    SyncRequest(SyncRequest),
    SyncResponse(SyncResponse),
}

pub struct SyncRequest {
    highest_committed_block: CryptoHash,
    limit: u32,
}

pub struct SyncResponse {
    blocks: Vec<Block>,
    next_qc: Option<QuorumCertificate>, 
}