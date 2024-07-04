/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Structured messages that are sent between replicas as part of the HotStuff subprotocol.
//!
//! ## Messages
//!
//! HotStuff-rs' modified HotStuff protocol involves four types of messages:
//! 1. [`Proposal`]: broadcasted by the leader of a given view, who proposes to extend the blockchain by
//!    inserting a block contained in the proposal.
//! 2. [`Nudge`]: broadcasted by the leader of a given view, who nudges other validators to participate in
//!    a next voting phase for a block with a given quorum certificate.
//! 3. [`Vote`]: sent by a validator to the leader of a next view to vote for a given proposal or nudge,
//!    contains a cryptographic signature over the information passed through a vote.
//! 4. [`NewView`]: sent by a replica to the next leader on view timeout, serves to update the next leader
//!    on the highestQC that replicas know of.
//!
//! ## `NewView` and view synchronization
//!
//! In the original HotStuff protocol, the leader of the next view keeps track of the number of
//! `NewView` messages collected in the current view with the aim of advancing to the next view once a
//! quorum of `NewView` messages are seen. This behavior can be thought of as implementing a rudimentary
//! view synchronization mechanism, which is helpful in the original HotStuff protocol because it did
//! not come with a "fully-featured" BFT view synchronization mechanism.
//!
//! HotStuff-rs, on the other hand, *does* include a separate BFT view synchronization mechanism (in the
//! form of the [Pacemaker](crate::pacemaker) module). Therefore, we deem replicating this behavior
//! unnecessary.

use std::mem;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::messages::{Cacheable, ProgressMessage, SignedMessage};
use crate::types::{basic::*, block::*, keypair::*};

use super::types::{Phase, QuorumCertificate};

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum HotStuffMessage {
    Proposal(Proposal),
    Nudge(Nudge),
    Vote(Vote),
    NewView(NewView),
}

impl HotStuffMessage {
    /// Returns the chain ID associated with a given [`HotStuffMessage`].
    pub fn chain_id(&self) -> ChainID {
        match self {
            HotStuffMessage::Proposal(Proposal { chain_id, .. }) => *chain_id,
            HotStuffMessage::Nudge(Nudge { chain_id, .. }) => *chain_id,
            HotStuffMessage::Vote(Vote { chain_id, .. }) => *chain_id,
            HotStuffMessage::NewView(NewView { chain_id, .. }) => *chain_id,
        }
    }

    /// Returns the view number associated with a given [`HotStuffMessage`].
    pub fn view(&self) -> ViewNumber {
        match self {
            HotStuffMessage::Proposal(Proposal { view, .. }) => *view,
            HotStuffMessage::Nudge(Nudge { view, .. }) => *view,
            HotStuffMessage::Vote(Vote { view, .. }) => *view,
            HotStuffMessage::NewView(NewView { view, .. }) => *view,
        }
    }

    /// Returns the number of bytes required to store a given instance of the [`HotStuffMessage`] enum.
    pub fn size(&self) -> u64 {
        match self {
            HotStuffMessage::Proposal(_) => mem::size_of::<Proposal>() as u64,
            HotStuffMessage::Nudge(_) => mem::size_of::<Nudge>() as u64,
            HotStuffMessage::Vote(_) => mem::size_of::<Vote>() as u64,
            HotStuffMessage::NewView(_) => mem::size_of::<NewView>() as u64,
        }
    }

    pub fn is_proposal(&self) -> bool {
        matches!(self, HotStuffMessage::Proposal(_))
    }

    pub fn is_nudge(&self) -> bool {
        matches!(self, HotStuffMessage::Nudge(_))
    }
}

impl Cacheable for HotStuffMessage {
    fn size(&self) -> u64 {
        self.size()
    }

    fn view(&self) -> ViewNumber {
        self.view()
    }
}

impl From<Proposal> for HotStuffMessage {
    fn from(proposal: Proposal) -> Self {
        HotStuffMessage::Proposal(proposal)
    }
}

impl From<Nudge> for HotStuffMessage {
    fn from(nudge: Nudge) -> Self {
        HotStuffMessage::Nudge(nudge)
    }
}

impl From<Vote> for HotStuffMessage {
    fn from(vote: Vote) -> Self {
        HotStuffMessage::Vote(vote)
    }
}

impl From<NewView> for HotStuffMessage {
    fn from(new_view: NewView) -> Self {
        HotStuffMessage::NewView(new_view)
    }
}

impl Into<ProgressMessage> for HotStuffMessage {
    fn into(self) -> ProgressMessage {
        ProgressMessage::HotStuffMessage(self)
    }
}

/// Broadcasted by the leader of a given view, who proposes to extend the blockchain by inserting a
/// block contained in the proposal.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Proposal {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub block: Block,
}

/// Broadcasted by the leader of a given view, who nudges other validators to participate in a next
/// voting phase for a block with a given quorum certificate.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Nudge {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub justify: QuorumCertificate,
}

impl Nudge {
    /// Create a new `Nudge` message containing the given `chain_id`, `view`, and `justify`-ing QC.
    ///
    /// # Panics
    ///
    /// `justify.phase` must be `Prepare` or `Precommit`. This function panics otherwise.
    pub fn new(chain_id: ChainID, view: ViewNumber, justify: QuorumCertificate) -> Self {
        match justify.phase {
            Phase::Generic | Phase::Decide => {
                panic!("`justify.phase` should be either Prepare or Precommit")
            }
            Phase::Prepare | Phase::Precommit | Phase::Commit => Self {
                chain_id,
                view,
                justify,
            },
        }
    }
}

/// Sent by a validator to the leader of a next view to vote for a given proposal or nudge, contains a
/// cryptographic signature over the information passed through a vote.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Vote {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub block: CryptoHash,
    pub phase: Phase,
    pub signature: SignatureBytes,
}

impl Vote {
    /// Create a `Vote` for the given `chain_id`, `view`, `block`, and `phase` by signing over the values
    /// with the provided `keypair`.
    pub fn new(
        keypair: &Keypair,
        chain_id: ChainID,
        view: ViewNumber,
        block: CryptoHash,
        phase: Phase,
    ) -> Self {
        let message_bytes = &(chain_id, view, block, phase).try_to_vec().unwrap();
        let signature = keypair.sign(message_bytes);

        Self {
            chain_id,
            view,
            block,
            phase,
            signature,
        }
    }
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

/// Sent by a replica to the next leader on view timeout, serves to update the next leader on the
/// highestQC that replicas know of.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct NewView {
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub highest_qc: QuorumCertificate,
}
