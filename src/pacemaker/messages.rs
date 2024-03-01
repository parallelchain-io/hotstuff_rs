/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for structured messages that are sent between replicas as part of the [Pacemaker] protocol.
use std::mem;

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{ed25519::SignatureBytes, Signer};

use crate::{messages::{Message, ProgressMessage}, types::{
    basic::*, certificates::*, keypair::*, voting::*
}};

pub enum PacemakerMessage {
    TimeoutVote(TimeoutVote),
    AdvanceView(AdvanceView),
}

impl PacemakerMessage {

    pub fn timeout_vote(
        me: Keypair,
        chain_id: ChainID,
        view: ViewNumber,
        highest_progress_certificate: ProgressCertificate,
    ) -> PacemakerMessage {
        let message = &(chain_id, view)
            .try_to_vec()
            .unwrap();
        let signature = me.sign(message);

        PacemakerMessage::TimeoutVote(TimeoutVote { 
            chain_id,
            view,
            signature,
            highest_progress_certificate
        })
    }

    pub fn advance_view(progress_certificate: ProgressCertificate) -> PacemakerMessage {
        PacemakerMessage::AdvanceView(AdvanceView { progress_certificate })
    }

    pub fn chain_id(&self) -> ChainID {
        match self {
            PacemakerMessage::TimeoutVote(TimeoutVote { chain_id, ..}) => chain_id,
            PacemakerMessage::AdvanceView(AdvanceView { progress_certificate }) => progress_certificate.chain_id()
        }
    }

    pub fn view(&self) -> ViewNumber {
        match self {
            PacemakerMessage::TimeoutVote(TimeoutVote { view, ..}) => view,
            PacemakerMessage::AdvanceView(AdvanceView { progress_certificate }) => progress_certificate.view()
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            PacemakerMessage::TimeoutVote(_) => mem::size_of::<TimeoutVote>(),
            PacemakerMessage::AdvanceView(_) => mem::size_of::<AdvanceView>()
        }
    }

}

impl Into<Message> for PacemakerMessage {
    fn into(self) -> Message {
        Message::ProgressMessage(ProgressMessage::PacemakerMessage(self))
    }
}

pub struct TimeoutVote {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub signature: SignatureBytes,
    pub highest_progress_certificate: ProgressCertificate,
}

impl Vote for TimeoutVote {
    fn to_message_bytes(&self) -> Option<Vec<u8>> {
        &(self.chain_id, self.view)
            .try_to_vec()
            .unwrap()
    }

    fn signature(&self) -> crate::types::basic::SignatureBytes {
        self.signature
    }
}

pub struct AdvanceView {
    pub progress_certificate: ProgressCertificate,
}

pub enum ProgressCertificate {
    TimeoutCertificate(TimeoutCertificate),
    QuorumCertificate(QuorumCertificate),
}

impl ProgressCertificate {

    pub fn chain_id(&self) -> ChainID {
        match self {
            ProgressCertificate::TimeoutCertificate(TimeoutCertificate { chain_id, ..}) => chain_id,
            ProgressCertificate::QuorumCertificate(QuorumCertificate { chain_id, ..}) => chain_id,
        }
    }

    pub fn view(&self) -> ViewNumber {
        match self {
            ProgressCertificate::TimeoutCertificate(TimeoutCertificate { view, ..}) => view,
            ProgressCertificate::QuorumCertificate(QuorumCertificate { view, ..}) => view,
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