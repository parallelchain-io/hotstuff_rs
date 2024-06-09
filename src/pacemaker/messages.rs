/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for structured messages that are sent between replicas as part of the
//! [`Pacemaker`][crate::pacemaker::protocol::Pacemaker] protocol.
//!
//! ## Messages
//!
//! The Pacemaker protocol involves two types of messages:
//! 1. [`TimeoutVote`], which a replica sends to signal to others that its epoch-change view has timed
//!    out.
//! 2. [`AdvanceView`], which a replica sends to prove to others that it is safe to move to the next
//!    view. The proof consists of either:
//!     - a `QuorumCertificate`, which serves as evidence that progress has been made in the current
//!       view, or
//!     - a `TimeoutCertificate`, which serves as evidence that a quorum of replicas have timed out in
//!       the current view.

use std::mem;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::hotstuff::types::QuorumCertificate;
use crate::messages::{Cacheable, ProgressMessage, SignedMessage};
use crate::types::{basic::*, keypair::*};

use super::types::TimeoutCertificate;

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum PacemakerMessage {
    TimeoutVote(TimeoutVote),
    AdvanceView(AdvanceView),
}

impl PacemakerMessage {
    pub(crate) fn timeout_vote(
        me: &Keypair,
        chain_id: ChainID,
        view: ViewNumber,
        highest_tc: Option<TimeoutCertificate>,
    ) -> PacemakerMessage {
        let message = &(chain_id, view).try_to_vec().unwrap();
        let signature = me.sign(message);

        PacemakerMessage::TimeoutVote(TimeoutVote {
            chain_id,
            view,
            signature,
            highest_tc,
        })
    }

    pub fn advance_view(progress_certificate: ProgressCertificate) -> PacemakerMessage {
        PacemakerMessage::AdvanceView(AdvanceView {
            progress_certificate,
        })
    }

    pub fn chain_id(&self) -> ChainID {
        match self {
            PacemakerMessage::TimeoutVote(TimeoutVote { chain_id, .. }) => *chain_id,
            PacemakerMessage::AdvanceView(AdvanceView {
                progress_certificate,
            }) => progress_certificate.chain_id(),
        }
    }

    pub fn view(&self) -> ViewNumber {
        match self {
            PacemakerMessage::TimeoutVote(TimeoutVote { view, .. }) => *view,
            PacemakerMessage::AdvanceView(AdvanceView {
                progress_certificate,
            }) => progress_certificate.view(),
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            PacemakerMessage::TimeoutVote(_) => mem::size_of::<TimeoutVote>() as u64,
            PacemakerMessage::AdvanceView(_) => mem::size_of::<AdvanceView>() as u64,
        }
    }
}

impl Cacheable for PacemakerMessage {
    fn size(&self) -> u64 {
        self.size()
    }

    fn view(&self) -> ViewNumber {
        self.view()
    }
}

impl Into<ProgressMessage> for PacemakerMessage {
    fn into(self) -> ProgressMessage {
        ProgressMessage::PacemakerMessage(self)
    }
}

/// A vote in favour of terminating a given view and moving to the next view. The signature is over
/// chain id and view.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct TimeoutVote {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub signature: SignatureBytes,
    pub highest_tc: Option<TimeoutCertificate>,
}

impl SignedMessage for TimeoutVote {
    fn message_bytes(&self) -> Vec<u8> {
        (self.chain_id, self.view).try_to_vec().unwrap()
    }

    fn signature_bytes(&self) -> SignatureBytes {
        self.signature
    }
}

/// A message containing a proof that the view can be advanced. The proof can be either a
/// [`QuorumCertificate`] or a [`TimeoutCertificate`] for the view.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct AdvanceView {
    pub progress_certificate: ProgressCertificate,
}

/// Proof that either:
/// 1. A quorum made a decision in the current view ([`QuorumCertificate`]), or
/// 2. A quorum voted for terminating the current view on timing out ([`TimeoutCertificate`]).
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum ProgressCertificate {
    TimeoutCertificate(TimeoutCertificate),
    QuorumCertificate(QuorumCertificate),
}

impl ProgressCertificate {
    pub fn chain_id(&self) -> ChainID {
        match self {
            ProgressCertificate::TimeoutCertificate(TimeoutCertificate { chain_id, .. }) => {
                *chain_id
            }
            ProgressCertificate::QuorumCertificate(QuorumCertificate { chain_id, .. }) => *chain_id,
        }
    }

    pub fn view(&self) -> ViewNumber {
        match self {
            ProgressCertificate::TimeoutCertificate(TimeoutCertificate { view, .. }) => *view,
            ProgressCertificate::QuorumCertificate(QuorumCertificate { view, .. }) => *view,
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
