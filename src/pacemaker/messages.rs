/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for structured messages that are sent between replicas as part of the [Pacemaker][crate::pacemaker::protocol::Pacemaker] protocol.
use std::mem;

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::ed25519::SignatureBytes;

use crate::hotstuff::types::QuorumCertificate;
use crate::messages::{Message, ProgressMessage, SignedMessage};
use crate::types::{
    basic::*, 
    keypair::*,
};

use super::types::TimeoutCertificate;

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum PacemakerMessage {
    TimeoutVote(TimeoutVote),
    AdvanceView(AdvanceView),
}

impl PacemakerMessage {

    pub(crate) fn timeout_vote(
        me: Keypair,
        chain_id: ChainID,
        view: ViewNumber,
        highest_tc: TimeoutCertificate,
    ) -> PacemakerMessage {
        let message = &(chain_id, view)
            .try_to_vec()
            .unwrap();
        let signature = me.sign(message);

        PacemakerMessage::TimeoutVote(TimeoutVote { 
            chain_id,
            view,
            signature,
            highest_tc
        })
    }

    pub fn advance_view(progress_certificate: ProgressCertificate) -> PacemakerMessage {
        PacemakerMessage::AdvanceView(AdvanceView { progress_certificate })
    }

    pub fn chain_id(&self) -> ChainID {
        match self {
            PacemakerMessage::TimeoutVote(TimeoutVote { chain_id, ..}) => *chain_id,
            PacemakerMessage::AdvanceView(AdvanceView { progress_certificate }) => progress_certificate.chain_id()
        }
    }

    pub fn view(&self) -> ViewNumber {
        match self {
            PacemakerMessage::TimeoutVote(TimeoutVote { view, ..}) => *view,
            PacemakerMessage::AdvanceView(AdvanceView { progress_certificate }) => progress_certificate.view()
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            PacemakerMessage::TimeoutVote(_) => mem::size_of::<TimeoutVote>() as u64,
            PacemakerMessage::AdvanceView(_) => mem::size_of::<AdvanceView>() as u64,
        }
    }

}

impl Into<Message> for PacemakerMessage {
    fn into(self) -> Message {
        Message::ProgressMessage(ProgressMessage::PacemakerMessage(self))
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct TimeoutVote {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub signature: SignatureBytes,
    pub highest_tc: TimeoutCertificate,
}

impl SignedMessage for TimeoutVote {
    fn message_bytes(&self) -> Vec<u8> {
        (self.chain_id, self.view)
            .try_to_vec()
            .unwrap()
    }

    fn signature_bytes(&self) -> crate::types::basic::SignatureBytes {
        self.signature
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct AdvanceView {
    pub progress_certificate: ProgressCertificate,
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum ProgressCertificate {
    TimeoutCertificate(TimeoutCertificate),
    QuorumCertificate(QuorumCertificate),
}

impl ProgressCertificate {

    pub fn chain_id(&self) -> ChainID {
        match self {
            ProgressCertificate::TimeoutCertificate(TimeoutCertificate { chain_id, ..}) => *chain_id,
            ProgressCertificate::QuorumCertificate(QuorumCertificate { chain_id, ..}) => *chain_id,
        }
    }

    pub fn view(&self) -> ViewNumber {
        match self {
            ProgressCertificate::TimeoutCertificate(TimeoutCertificate { view, ..}) => *view,
            ProgressCertificate::QuorumCertificate(QuorumCertificate { view, ..}) => *view,
        }
    }
}

impl From<QuorumCertificate> for ProgressCertificate {
    fn from(value: QuorumCertificate) -> Self {
        ProgressCertificate::QuorumCertificate(value)
    }
}

impl From<TimeoutCertificate> for ProgressCertificate {
    fn from(value: TimeoutCertificate) -> Self {
        ProgressCertificate::TimeoutCertificate(value)
    }
}