/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for structured messages that are sent between replicas as part of the [HotStuff][crate::hotstuff::protocol::HotStuff] protocol.

use std::mem;

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{Verifier};

use crate::messages::{Message, ProgressMessage, SignedMessage};
use crate::types::{
    basic::*, 
    block::*, 
    keypair::*,
};

use super::types::{Phase, QuorumCertificate};

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum HotStuffMessage {
    Proposal(Proposal),
    Nudge(Nudge),
    Vote(Vote),
    NewView(NewView),
}

impl HotStuffMessage {
    pub fn proposal(chain_id: ChainID, view: ViewNumber, block: Block) -> HotStuffMessage {
        HotStuffMessage::Proposal(Proposal {
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
    ) -> HotStuffMessage {
        match justify.phase {
            Phase::Generic | Phase::Commit(_) => panic!(),
            Phase::Prepare | Phase::Precommit(_) => HotStuffMessage::Nudge(Nudge {
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
    ) -> HotStuffMessage {
        HotStuffMessage::NewView(NewView {
            chain_id,
            view,
            highest_qc,
        })
    }

    pub(crate) fn vote(
        me: Keypair,
        chain_id: ChainID,
        view: ViewNumber,
        block: CryptoHash,
        phase: Phase,
    ) -> HotStuffMessage {
        let message_bytes = &(chain_id, view, block, phase)
            .try_to_vec()
            .unwrap();
        let signature = me.sign(message_bytes);

        HotStuffMessage::Vote(Vote {
            chain_id,
            view,
            block,
            phase,
            signature
        })
    }

    /// Returns the chain ID associated with a given [HotStuffMessage].
    pub fn chain_id(&self) -> ChainID {
        match self {
            HotStuffMessage::Proposal(Proposal { chain_id, .. }) => *chain_id,
            HotStuffMessage::Nudge(Nudge { chain_id, .. }) => *chain_id,
            HotStuffMessage::Vote(Vote { chain_id, .. }) => *chain_id,
            HotStuffMessage::NewView(NewView { chain_id, .. }) => *chain_id,
        }
    }

    /// Returns the view number associated with a given [HotStuffMessage].
    pub fn view(&self) -> ViewNumber {
        match self {
            HotStuffMessage::Proposal(Proposal { view, .. }) => *view,
            HotStuffMessage::Nudge(Nudge { view, .. }) => *view,
            HotStuffMessage::Vote(Vote { view, .. }) => *view,
            HotStuffMessage::NewView(NewView { view, .. }) => *view,
        }
    }

    /// Returns the number of bytes required to store a given instance of the [HotStuffMessage] enum.
    pub fn size(&self) -> u64 {
        match self {
            HotStuffMessage::Proposal(_) => mem::size_of::<Proposal>() as u64,
            HotStuffMessage::Nudge(_) => mem::size_of::<Nudge>() as u64,
            HotStuffMessage::Vote(_) => mem::size_of::<Vote>() as u64,
            HotStuffMessage::NewView(_) => mem::size_of::<NewView>() as u64,
        }
    }
}

impl Into<Message> for HotStuffMessage {
    fn into(self) -> Message {
        Message::ProgressMessage(ProgressMessage::HotStuffMessage(self))
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

impl SignedMessage for Vote {
    fn message_bytes(&self) -> Vec<u8> {
        (self.chain_id, self.view, self.block, self.phase)
            .try_to_vec()
            .unwrap()
    }
    
    fn signature_bytes(&self) -> SignatureBytes {
        self.signature
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct NewView {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub highest_qc: QuorumCertificate,
}
