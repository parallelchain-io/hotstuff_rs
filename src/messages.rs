/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! Definitions for structures messages that are sent between replicas. 

use borsh::{BorshSerialize, BorshDeserialize};
use ed25519_dalek::{Verifier, ed25519::signature::Signature, Signer};

use crate::types::*;

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum Message {
    ProgressMessage(ProgressMessage),
    SyncMessage(SyncMessage),
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum ProgressMessage {
    Proposal(Proposal),
    Nudge(Nudge), 
    Vote(Vote),
    NewView(NewView),
}

impl ProgressMessage {
    pub fn proposal(app_id: AppID, view: ViewNumber, block: Block) -> ProgressMessage {
        ProgressMessage::Proposal(Proposal {
            app_id,
            view,
            block,
        }) 
    }

    /// # Panics
    /// justify.phase must be Prepare or Precommit. This function panics otherwise.
    pub fn nudge(app_id: AppID, view: ViewNumber, justify: QuorumCertificate) -> ProgressMessage {
        match justify.phase {
            Phase::Generic | Phase::Commit(_) => panic!(),
            Phase::Prepare | Phase::Precommit(_) => {
                ProgressMessage::Nudge(Nudge {
                    app_id,
                    view,
                    justify
                })
            }
        }
    }

    pub fn new_view(app_id: AppID, view: ViewNumber, highest_qc: QuorumCertificate) -> ProgressMessage {
        ProgressMessage::NewView(NewView { app_id, view, highest_qc })
    }

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

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Proposal {
    pub app_id: AppID,
    pub view: ViewNumber,
    pub block: Block,
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Nudge {
    pub app_id: AppID,
    pub view: ViewNumber,
    pub justify: QuorumCertificate,
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Vote {
    pub app_id: AppID,
    pub view: ViewNumber,
    pub block: CryptoHash,
    pub phase: Phase,
    pub signature: SignatureBytes,
}

impl Vote {
    /// # Panics
    /// pk must be a valid public key.
    pub fn is_correct(&self, pk: &PublicKeyBytes) -> bool {
        if let Ok(signature) = Signature::from_bytes(&self.signature) {
            PublicKey::from_bytes(pk).unwrap().verify(&(self.app_id, self.view, self.block, self.phase).try_to_vec().unwrap(), &signature).is_ok()
        } else {
            false
        }
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct NewView {
    pub app_id: AppID,
    pub view: ViewNumber,
    pub highest_qc: QuorumCertificate
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum SyncMessage {
    SyncRequest(SyncRequest),
    SyncResponse(SyncResponse),
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct SyncRequest {
    pub start_height: BlockHeight,
    pub limit: u32,
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct SyncResponse {
    pub blocks: Vec<Block>,
    pub highest_qc: QuorumCertificate, 
}
 
/// A wrapper around [DalekKeypair] which implements [a convenience method](ProgressMessage::vote) for creating properly
/// signed [votes](Vote).
pub(crate) struct Keypair(pub(crate) DalekKeypair);

impl Keypair {
    pub(crate) fn new(keypair: DalekKeypair) -> Keypair {
        Keypair(keypair)
    } 

    pub(crate) fn vote(&self, app_id: AppID, view: ViewNumber, block: CryptoHash, phase: Phase) -> ProgressMessage {
        let signature = self.0.sign(&(app_id, view, block, phase).try_to_vec().unwrap()).to_bytes();
        ProgressMessage::Vote(Vote { app_id, view, block, phase, signature })
    } 

    pub(crate) fn public(&self) -> PublicKeyBytes {
        self.0.public.to_bytes()
    }
}
