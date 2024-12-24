/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Messages sent between replicas as part of the HotStuff subprotocol.

use std::mem;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    networking::{messages::ProgressMessage, receiving::Cacheable},
    types::{
        block::*,
        crypto_primitives::Keypair,
        data_types::*,
        signed_messages::{SignedMessage, Vote},
    },
};

use super::types::{Phase, PhaseCertificate};

/// Every kind of message sent between replicas as part of the HotStuff subprotocol.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum HotStuffMessage {
    /// See [`Proposal`].
    Proposal(Proposal),

    /// See [`Nudge`].
    Nudge(Nudge),

    /// See [`PhaseVote`].
    PhaseVote(PhaseVote),

    /// See [`NewView`].
    NewView(NewView),
}

impl HotStuffMessage {
    /// Get the `ChainID` associated with the `HotStuffMessage`.
    pub fn chain_id(&self) -> ChainID {
        match self {
            HotStuffMessage::Proposal(Proposal { chain_id, .. }) => *chain_id,
            HotStuffMessage::Nudge(Nudge { chain_id, .. }) => *chain_id,
            HotStuffMessage::PhaseVote(PhaseVote { chain_id, .. }) => *chain_id,
            HotStuffMessage::NewView(NewView { chain_id, .. }) => *chain_id,
        }
    }

    /// Get the view number associated with a given [`HotStuffMessage`].
    pub fn view(&self) -> ViewNumber {
        match self {
            HotStuffMessage::Proposal(Proposal { view, .. }) => *view,
            HotStuffMessage::Nudge(Nudge { view, .. }) => *view,
            HotStuffMessage::PhaseVote(PhaseVote { view, .. }) => *view,
            HotStuffMessage::NewView(NewView { view, .. }) => *view,
        }
    }

    /// Returns the number of bytes required to store a given instance of the [`HotStuffMessage`] enum.
    pub fn size(&self) -> u64 {
        match self {
            HotStuffMessage::Proposal(_) => mem::size_of::<Proposal>() as u64,
            HotStuffMessage::Nudge(_) => mem::size_of::<Nudge>() as u64,
            HotStuffMessage::PhaseVote(_) => mem::size_of::<PhaseVote>() as u64,
            HotStuffMessage::NewView(_) => mem::size_of::<NewView>() as u64,
        }
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

impl From<PhaseVote> for HotStuffMessage {
    fn from(vote: PhaseVote) -> Self {
        HotStuffMessage::PhaseVote(vote)
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

/// Message broadcasted by a leader in `view` to propose to other validators that the block tree
/// identified by `chain_id` be extended by inserting `block`.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Proposal {
    /// `ChainID` of the block tree that this `Proposal` should extend.
    pub chain_id: ChainID,

    /// Current `ViewNumber` of the [proposer](super::roles::is_proposer) that created this
    /// `Proposal`.
    pub view: ViewNumber,

    /// A `Block` extending the chain identified by `chain_id`.
    pub block: Block,
}

/// Message broadcasted by a leader in `view` to "nudge" other validators to participate in the voting
/// phase after `justify.phase` in order to make progress in committing a **validator-set-updating**
/// block in the block tree identified by `chain_id`.
///
/// # Permissible variants of `justify.phase`
///
/// `nudge.justify.phase` must be `Prepare`, `Precommit`, or `Commit`. This invariant is enforced in
/// two places:
/// 1. When a validator creates a `Nudge` using [`new`](Self::new).
/// 2. When a replica receives a `Nudge` and checks the
///    [`safe_nudge`](crate::block_tree::invariants::safe_nudge) predicate.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Nudge {
    /// `ChainID` of the block tree that `justify.block` is part of.
    pub chain_id: ChainID,

    /// Current `ViewNumber` of the [proposer](super::roles::is_proposer) that created this
    /// `Nudge`.
    pub view: ViewNumber,

    /// [`PhaseVote`]s for this `Nudge` should be for the `Phase` immediately after `justify.phase`. E.g.,
    /// if `justify.phase == Precommit`, then `phase_vote.phase` should be `Commit`.
    pub justify: PhaseCertificate,
}

impl Nudge {
    /// Create a new `Nudge` message containing the given `chain_id`, `view`, and `justify`-ing PC.
    ///
    /// # Panics
    ///
    /// `justify.phase` must be `Prepare` or `Precommit`. This function panics otherwise.
    pub fn new(chain_id: ChainID, view: ViewNumber, justify: PhaseCertificate) -> Self {
        assert!(
            justify.phase.is_prepare() || justify.phase.is_precommit() || justify.phase.is_commit()
        );

        Self {
            chain_id,
            view,
            justify,
        }
    }
}

/// Message sent by a validator to [a leader of `view + 1`](super::roles::phase_vote_recipient) to
/// indicate that the validator agrees to a specific [`Proposal`] or [`Nudge`].
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct PhaseVote {
    /// `chain_id` field of the `Proposal` or `Nudge` associated with this `PhaseVote`.
    pub chain_id: ChainID,

    /// `view` field of the `Proposal` or `Nudge` associated with this `PhaseVote`.
    pub view: ViewNumber,

    /// `block` field of the `Proposal` or `Nudge` associated with this `PhaseVote`.
    pub block: CryptoHash,

    /// `phase` field of the `Proposal` or `Nudge` associated with this `PhaseVote`.
    pub phase: Phase,

    /// Digital signature formed using
    pub signature: SignatureBytes,
}

impl PhaseVote {
    /// Create a `PhaseVote` for the given `chain_id`, `view`, `block`, and `phase` by signing over the values
    /// with the provided `keypair`.
    pub(crate) fn new(
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

impl SignedMessage for PhaseVote {
    fn message_bytes(&self) -> Vec<u8> {
        (self.chain_id, self.view, self.block, self.phase)
            .try_to_vec()
            .unwrap()
    }

    fn signature_bytes(&self) -> SignatureBytes {
        self.signature
    }
}

impl Vote for PhaseVote {
    fn chain_id(&self) -> ChainID {
        self.chain_id
    }

    fn view(&self) -> ViewNumber {
        self.view
    }
}

/// Message sent by a replica to [the leaders of the next view](super::roles::new_view_recipients) on
/// view timeout to make them aware of the replica's `highest_pc`.
///
/// # `NewView` and view synchronization
///
/// In the original HotStuff protocol, the leader of the next view keeps track of the number of
/// `NewView` messages collected in the current view with the aim of advancing to the next view once a
/// quorum of `NewView` messages are seen. This behavior implements a rudimentary view synchronization
/// mechanism, which is helpful in the original HotStuff protocol because it did not come with a
/// "fully-featured" BFT view synchronization mechanism.
///
/// HotStuff-rs, on the other hand, *does* include a separate BFT view synchronization mechanism (in the
/// form of the [Pacemaker](crate::pacemaker) module). Therefore, we deem this behavior unnecessary and
/// do not implement it.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct NewView {
    /// `ChainID` of the block tree that `highest_pc.block` is part of.
    pub chain_id: ChainID,

    /// The `view` that the replica sending this `NewView` is exiting.
    pub view: ViewNumber,

    /// The sending replica's `highest_pc`.
    pub highest_pc: PhaseCertificate,
}
