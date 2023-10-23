/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for structured messages that are sent between replicas.
//!
//! This includes messages [used in the progress protocol](ProgressMessage), and those [used in the sync protocol](SyncMessage).

use std::mem;
use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{Signature, Signer, Verifier};

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
    pub fn proposal(chain_id: ChainID, view: ViewNumber, block: Block) -> ProgressMessage {
        ProgressMessage::Proposal(Proposal {
            chain_id,
            view,
            block,
        })
    }

    /// # Panics
    /// justify.phase must be Prepare or Precommit. This function panics otherwise.
    pub fn nudge(
        chain_id: ChainID,
        view: ViewNumber,
        justify: QuorumCertificate,
    ) -> ProgressMessage {
        match justify.phase {
            Phase::Generic | Phase::Commit(_) => panic!(),
            Phase::Prepare | Phase::Precommit(_) => ProgressMessage::Nudge(Nudge {
                chain_id,
                view,
                justify,
            }),
        }
    }

    pub fn new_view(
        chain_id: ChainID,
        view: ViewNumber,
        highest_qc: QuorumCertificate,
    ) -> ProgressMessage {
        ProgressMessage::NewView(NewView {
            chain_id,
            view,
            highest_qc,
        })
    }

    pub fn chain_id(&self) -> ChainID {
        match self {
            ProgressMessage::Proposal(Proposal { chain_id, .. }) => *chain_id,
            ProgressMessage::Nudge(Nudge { chain_id, .. }) => *chain_id,
            ProgressMessage::Vote(Vote { chain_id, .. }) => *chain_id,
            ProgressMessage::NewView(NewView { chain_id, .. }) => *chain_id,
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

    pub fn size(&self) -> u64 {
        match self {
            ProgressMessage::Proposal(_) => mem::size_of::<Proposal>() as u64,
            ProgressMessage::Nudge(_) => mem::size_of::<Nudge>() as u64,
            ProgressMessage:: Vote(_) => mem::size_of::<Vote>() as u64,
            ProgressMessage::NewView(_) => mem::size_of::<NewView>() as u64,
        }
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Proposal {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub block: Block,
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Nudge {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub justify: QuorumCertificate,
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Vote {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub block: CryptoHash,
    pub phase: Phase,
    pub signature: SignatureBytes,
}

impl Vote {
    /// # Panics
    /// pk must be a valid public key.
    pub fn is_correct(&self, pk: &VerifyingKey) -> bool {
        let signature = Signature::from_bytes(&self.signature);
        pk.verify(
        &(self.chain_id, self.view, self.block, self.phase)
            .try_to_vec()
            .unwrap(),
            &signature,
        )
        .is_ok()
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct NewView {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub highest_qc: QuorumCertificate,
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

/// A wrapper around SigningKey which implements [a convenience method](ProgressMessage::vote) for creating properly
/// signed [votes](Vote).
pub(crate) struct Keypair(pub(crate) SigningKey);

impl Keypair {
    pub(crate) fn new(signing_key: SigningKey) -> Keypair {
        Keypair(signing_key)
    }

    pub(crate) fn vote(
        &self,
        chain_id: ChainID,
        view: ViewNumber,
        block: CryptoHash,
        phase: Phase,
    ) -> ProgressMessage {
        let signature = self
            .0
            .sign(&(chain_id, view, block, phase).try_to_vec().unwrap())
            .to_bytes();
        ProgressMessage::Vote(Vote {
            chain_id,
            view,
            block,
            phase,
            signature,
        })
    }

    pub(crate) fn public(&self) -> VerifyingKey {
        self.0.verifying_key()
    }
}
