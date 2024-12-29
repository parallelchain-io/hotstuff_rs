/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Messages sent between replicas as part of the Pacemaker subprotocol.

use std::mem;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    hotstuff::types::PhaseCertificate,
    networking::{messages::ProgressMessage, receiving::Cacheable},
    types::{
        crypto_primitives::Keypair,
        data_types::*,
        signed_messages::{SignedMessage, Vote},
    },
};

use super::types::TimeoutCertificate;

/// Enum wrapper around any kind of message sent between replicas as part of the Pacemaker subprotocol.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum PacemakerMessage {
    /// See [`TimeoutVote`].
    TimeoutVote(TimeoutVote),

    /// See [`AdvanceView`].
    AdvanceView(AdvanceView),
}

impl PacemakerMessage {
    /// Create a [`PacemakerMessage::TimeoutVote`] that indicates that the validator identified by `keypair`
    /// which is currently replicating the `chain_id` blockchain would like to leave the specified
    /// Epoch-Change `view`.
    pub(crate) fn timeout_vote(
        keypair: &Keypair,
        chain_id: ChainID,
        view: ViewNumber,
        highest_tc: Option<TimeoutCertificate>,
    ) -> PacemakerMessage {
        let message = &(chain_id, view).try_to_vec().unwrap();
        let signature = keypair.sign(message);

        PacemakerMessage::TimeoutVote(TimeoutVote {
            chain_id,
            view,
            signature,
            highest_tc,
        })
    }

    /// Create a [`PacemakerMessage::AdvanceView`] which that to get other replicas to leave
    /// [`progress_certificate.view()`](ProgressCertificate::view) and enter
    /// `progress_certificate.view() + 1`.
    pub fn advance_view(progress_certificate: ProgressCertificate) -> PacemakerMessage {
        PacemakerMessage::AdvanceView(AdvanceView {
            progress_certificate,
        })
    }

    /// Get the `ChainID` in the inner message.
    pub fn chain_id(&self) -> ChainID {
        match self {
            PacemakerMessage::TimeoutVote(TimeoutVote { chain_id, .. }) => *chain_id,
            PacemakerMessage::AdvanceView(AdvanceView {
                progress_certificate,
            }) => progress_certificate.chain_id(),
        }
    }

    /// Get the `ViewNumber` in the inner message.
    pub fn view(&self) -> ViewNumber {
        match self {
            PacemakerMessage::TimeoutVote(TimeoutVote { view, .. }) => *view,
            PacemakerMessage::AdvanceView(AdvanceView {
                progress_certificate,
            }) => progress_certificate.view(),
        }
    }

    /// Get the size (in bytes) of the in-memory representation of the inner message.
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

/// Vote in favor of leaving a specified Epoch-Change `view` and moving on to the next view.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct TimeoutVote {
    /// `ChainID` of the block tree that the replica's validator set extends.
    pub chain_id: ChainID,

    /// The view that this timeout vote seeks to terminate.
    pub view: ViewNumber,

    /// Signature over `chain_id` and `view` made using the sending replica's keypair.
    pub signature: SignatureBytes,

    /// The current highest timeout certificate of the sending replica, i.e., the the one with the highest
    /// [`view`](TimeoutCertificate::view).
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

impl Vote for TimeoutVote {
    fn chain_id(&self) -> ChainID {
        self.chain_id
    }

    fn view(&self) -> ViewNumber {
        self.view
    }
}

/// Message containing cryptographic proof that a replica can safely advance to higher view.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct AdvanceView {
    pub progress_certificate: ProgressCertificate,
}

/// Enum wrapper around every kind of [`Certificate`](crate::types::signed_messages::Certificate) that
/// can cause a replica to move to another view.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum ProgressCertificate {
    TimeoutCertificate(TimeoutCertificate),
    PhaseCertificate(PhaseCertificate),
}

impl ProgressCertificate {
    pub fn chain_id(&self) -> ChainID {
        match self {
            ProgressCertificate::TimeoutCertificate(TimeoutCertificate { chain_id, .. }) => {
                *chain_id
            }
            ProgressCertificate::PhaseCertificate(PhaseCertificate { chain_id, .. }) => *chain_id,
        }
    }

    pub fn view(&self) -> ViewNumber {
        match self {
            ProgressCertificate::TimeoutCertificate(TimeoutCertificate { view, .. }) => *view,
            ProgressCertificate::PhaseCertificate(PhaseCertificate { view, .. }) => *view,
        }
    }
}

impl From<PhaseCertificate> for ProgressCertificate {
    fn from(value: PhaseCertificate) -> Self {
        ProgressCertificate::PhaseCertificate(value)
    }
}

impl From<TimeoutCertificate> for ProgressCertificate {
    fn from(value: TimeoutCertificate) -> Self {
        ProgressCertificate::TimeoutCertificate(value)
    }
}
