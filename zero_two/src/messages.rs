use crate::types::*;

pub enum Message {
    ProgressMessage(ProgressMessage),
    SyncMessage(SyncMessage),
}

pub enum ProgressMessage {
    Proposal(Proposal),
    Nudge(Nudge), 
    Vote(Vote),
    NewView(NewView),
}

impl ProgressMessage {
    pub fn app_id(&self) -> AppID {
        match self {
            ProgressMessage::Proposal(Proposal { app_id, .. }) => *app_id,
            ProgressMessage::Nudge(Nudge { app_id, .. }) => *app_id,
            ProgressMessage::Vote(Vote { app_id, .. })  => *app_id,
            ProgressMessage::NewView(NewView { app_id, .. }) => *app_id,
        }
    }

    pub fn view(&self) -> ViewNumber {
        match self {
            ProgressMessage::Proposal(Proposal { view, .. }) => *view,
            ProgressMessage::Nudge(Nudge { view, .. }) => *view,
            ProgressMessage::Vote(Vote { view, .. }) => *view,
            ProgressMessage::NewView(NewView { view, .. }) => *view,
        }
    } 
}

pub struct Proposal {
    app_id: AppID,
    view: ViewNumber,
    block: Block,
}

pub struct Nudge {
    app_id: AppID,
    view: ViewNumber,
    justify: QuorumCertificate,
}

pub struct Vote {
    app_id: AppID,
    view: ViewNumber,
    block: CryptoHash,
    phase: Phase,
    signature: SignatureBytes,
}

impl Vote {
    pub fn is_correct(&self, pk: &PublicKeyBytes) -> bool {
        todo!()
    }
}

pub struct NewView {
    app_id: AppID,
    view: ViewNumber,
    highest_qc: QuorumCertificate
}

pub enum SyncMessage {
    SyncRequest(SyncRequest),
    SyncResponse(SyncResponse),
}

pub struct SyncRequest {
    pub highest_committed_block: CryptoHash,
    pub limit: u32,
}

pub struct SyncResponse {
    pub blocks: Option<Vec<Block>>,
    pub highest_qc: Option<QuorumCertificate>, 
}

pub(crate) struct ProgressMessageFactory(pub(crate) Keypair);

impl ProgressMessageFactory {
    // phase must be either Generic or Prepare.
    pub(crate) fn new_proposal(&self, app_id: AppID, view: ViewNumber, block: Block, phase: Phase) -> ProgressMessage {
        todo!()
    }

    // justify.phase must be either Prepare or Precommit.
    pub(crate) fn nudge_proposal(&self, app_id: AppID, view: ViewNumber, block: CryptoHash, justify: QuorumCertificate) -> ProgressMessage {
        todo!()
    }

    pub(crate) fn vote(&self, app_id: AppID, view: ViewNumber, block: CryptoHash, phase: Phase) -> ProgressMessage {
        todo!()
    }

    pub(crate) fn new_view(&self, view: ViewNumber, highest_qc: QuorumCertificate) -> ProgressMessage {
        todo!()
    }
}