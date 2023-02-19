/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

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
    pub app_id: AppID,
    pub view: ViewNumber,
    pub block: Block,
}

pub struct Nudge {
    pub app_id: AppID,
    pub view: ViewNumber,
    pub justify: QuorumCertificate,
}

pub struct Vote {
    pub app_id: AppID,
    pub view: ViewNumber,
    pub block: CryptoHash,
    pub phase: Phase,
    pub signature: SignatureBytes,
}

impl Vote {
    pub fn is_correct(&self, pk: &PublicKeyBytes) -> bool {
        todo!()
    }
}

pub struct NewView {
    pub app_id: AppID,
    pub view: ViewNumber,
    pub highest_qc: QuorumCertificate
}

pub enum SyncMessage {
    SyncRequest(SyncRequest),
    SyncResponse(SyncResponse),
}

pub struct SyncRequest {
    pub highest_committed_block: Option<CryptoHash>,
    pub limit: u32,
}

pub struct SyncResponse {
    pub blocks: Vec<Block>,
    pub highest_qc: QuorumCertificate, 
}
 
pub(crate) struct Keypair(pub(crate) DalekKeypair);

impl Keypair {
    pub(crate) fn new(keypair: DalekKeypair) -> Keypair {
        Keypair(keypair)
    }

    // phase must be either Generic or Prepare.
    pub(crate) fn proposal(&self, app_id: AppID, view: ViewNumber, block: &Block) -> ProgressMessage {
        todo!()
    }

    // justify.phase must be either Prepare or Precommit.
    pub(crate) fn nudge(&self, app_id: AppID, view: ViewNumber, justify: &QuorumCertificate) -> ProgressMessage {
        todo!()
    }

    pub(crate) fn vote(&self, app_id: AppID, view: ViewNumber, block: CryptoHash, phase: Phase) -> ProgressMessage {
        todo!()
    }

    pub(crate) fn new_view(&self, app_id: AppID, view: ViewNumber, highest_qc: &QuorumCertificate) -> ProgressMessage {
        todo!()
    }

    pub(crate) fn public(&self) -> PublicKeyBytes {
        self.0.public.to_bytes()
    }
}
