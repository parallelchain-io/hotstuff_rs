use std::ops::Deref;
use std::io;
use borsh::{BorshSerialize, BorshDeserialize};
use sha2::Digest;
use sha2::Sha256;
use ed25519_dalek::{Signature as DalekSignature, SIGNATURE_LENGTH};
use crate::identity::{PublicAddr, ParticipantSet};

pub type ViewNumber = u64;

pub type AppID = u64;

pub type BlockHeight = u64;

pub const NODE_HASH_LEN: usize = 32;
pub type BlockHash = [u8; NODE_HASH_LEN];

/// A list of App-provided Datums. Datums are stored in Blocks as a delineated, indexable list so that
/// applications can quickly get the Datum sitting a particular index in a Block using 
/// `BlockTree::get_block_command_by_hash` or `get_block_command_by_height` instead of getting the entire list
/// of Data at once, which might be an unacceptable expensive I/O operation.
pub type Data = Vec<Datum>;

/// Arbitrary data provided by an App as part of the return value of `create_leaf`. 
pub type Datum = Vec<u8>;

pub const DATA_HASH_LEN: usize = 32;
pub type DataHash = [u8; DATA_HASH_LEN];

#[derive(Clone)]
pub struct Signature(pub DalekSignature);

impl BorshSerialize for Signature {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&self.0.to_bytes())
    }
}

impl BorshDeserialize for Signature {
    fn deserialize(buf: &mut &[u8]) -> std::io::Result<Self> {
        // BorshDeserialize requires that deserialize update the buffer to point at the remaining bytes.
        if buf.len() < SIGNATURE_LENGTH {
            *buf = &buf[buf.len()..];
            Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Buffer is too short to produce Signature"))
        } else {
            let sig_bytes = &buf[0..SIGNATURE_LENGTH];
            let dalek_signature = DalekSignature::from_bytes(sig_bytes)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Dalek rejects the Signature"))?;
            *buf = &buf[SIGNATURE_LENGTH..];
            Ok(Signature(dalek_signature))
        } 
    }
}

impl Deref for Signature {
    type Target = DalekSignature;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Messages sent between Participants on IPC channels to drive BlockTree construction forward.
#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub enum ConsensusMsg {
    /// Broadcasted by the Leader of a view to all other Replicas to propose a Block which extends the BlockTree.
    Propose(ViewNumber, Block),

    /// Sent by a Replica to the Leader of the next view to attest that the Replica has inserted the Block
    /// identified by BlockHash into its local BlockTree.
    Vote(ViewNumber, BlockHash, Signature),

    /// Sent by a Replica to the Leader of the next view to hand over the Replica's highest QuorumCertificate.
    /// By collecting a quorum of QCs and selecting the highest, the Leader of the next View will have a high
    /// enough QC that its proposal is guaranteed to pass the SafeBlock predicate and be accepted by the Replicas
    /// of the next round.
    NewView(ViewNumber, QuorumCertificate),
}

/// A bundle of data 'chained' onto another like bundle of data--referred to as its 'parent'--by containing a
/// cryptographic certificate that attests to the parent having been accepted and stored by a quorum of Participants.
#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct Block {
    pub app_id: AppID,

    pub hash: BlockHash,

    /// How many justify-links separate this Block from the Genesis Block.
    pub height: BlockHeight,

    pub justify: QuorumCertificate,

    pub data_hash: DataHash,
    pub data: Data,
}

impl Block {
    pub fn hash(height: BlockHeight, justify: &QuorumCertificate, data_hash: &DataHash) -> BlockHash {
        todo!()
    }
}

/// An aggregate of multiple `ConsensusMsg::Vote`s. A valid QuorumCertificate is proof that a quorum of
/// Participants has inserted the branch headed by the Block identified by the QuorumCertificate's `block
/// _hash` into its local BlockTree. The more QuorumCertificates 'justifies' a Block, the closer the Block
/// to being considered committed. If you are familiar with Bitcoin, 'justification' via QuorumCertificate
/// is similar to 'confirmation' by Blocks.
/// 
/// A QuorumCertificate is 'valid', i.e., it can safely be inserted into the BlockTree, if its is_quorum
/// and is_cryptographically_correct methods return true. 
#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct QuorumCertificate {
    /// The view number of the Votes that were combined to form this QuorumCertificate. 
    pub view_number: ViewNumber,

    /// The hash of the Block that the Votes that were combined to form 
    pub block_hash: BlockHash,
    
    // A collection of Signatures, which (if the QuorumCertificate was formed right) were formed over the
    // same view_number and block_hash.
    pub sigs: SignatureSet,
}

impl QuorumCertificate {
    /// Computes the size of a quorum given the number of participants in the ParticipantSet.
    pub fn quorum_size(num_participants: usize) -> usize {
        // '/' here is integer (floor) division. 
        (num_participants * 2) / 3 + 1
    }

    /// Returns whether this QuorumCertificate contains at least a quorum of signatures, correct or incorrect. 
    pub fn is_quorum(&self, num_participants: usize) -> bool {
        self.sigs.count() > Self::quorum_size(num_participants) 
    }

    /// Returns whether all Signatures in this QuorumCertificate were produced over (view_number ++ block_hash)
    /// by the SecretKey associated with a PublicAddr in the provided ParticipantSet.
    pub fn is_cryptographically_correct(&self, participant_set: &ParticipantSet) -> bool {
        todo!()
    }
}

/// Helps Leaders incrementally form QuorumCertificates by combining Signatures on some view_number and block_hash.
pub struct QuorumCertificateAggregator {
    view_number: ViewNumber,
    block_hash: BlockHash,
    participant_set: ParticipantSet,
    signature_set: SignatureSet,
}

impl QuorumCertificateAggregator {
    /// Creates a QuorumCertificateAggregator which attempts to form a QuorumCertificate that is valid for the given
    /// ViewNumber, BlockHash, and ParticipantSet.
    pub fn new(view_number: ViewNumber, block_hash: BlockHash, participant_set: ParticipantSet) -> QuorumCertificateAggregator {
        QuorumCertificateAggregator {
            view_number,
            block_hash,
            signature_set: SignatureSet::new(participant_set.len()),
            participant_set,
        }
    }
    
    /// Inserts a signature into this QuorumCertificateAggregator and checks whether the Aggregator now contains enough
    /// Signatures to form a valid QuorumCertificate, in which case it returns Ok(true). To turn an Aggregator into a 
    /// QuorumCertificate, use its implementation of Into<QuorumCertificate>.
    /// 
    /// Note that this DOES NOT check whether the provided Signature is cryptographically correct.
    /// 
    /// # Errors
    /// Read the documentation of QCAggregatorInsertError. If an error is returned, this function was a no-op.
    pub fn insert(&mut self, signature: Signature, of_public_addr: PublicAddr) -> Result<bool, QCAggregatorInsertError> {
        if self.signature_set.count() > QuorumCertificate::quorum_size(self.participant_set.len()) {
            return Err(QCAggregatorInsertError::AlreadyAQuorum)
        }
        if let Some(position) = self.participant_set.keys().position(|pk| *pk == of_public_addr) {
            if self.signature_set.contains(position) {
                return Err(QCAggregatorInsertError::PublicAddrAlreadyProvidedSignature);
            }
            let _ = self.signature_set.insert(position, signature);
    
            Ok(self.signature_set.count() > QuorumCertificate::quorum_size(self.participant_set.len()))
            } else {
                Err(QCAggregatorInsertError::PublicAddrNotInParticipantSet)
            }
        }
}

impl Into<QuorumCertificate> for QuorumCertificateAggregator {
    fn into(self) -> QuorumCertificate {
        QuorumCertificate { 
            view_number: self.view_number,
            block_hash: self.block_hash,
            sigs:  self.signature_set
        }
    }
}

/// Ways that QuorumCertificateAggregator::insert can fail.
pub enum QCAggregatorInsertError {
    /// This QuorumCertificateAggregator already held enough Signatures before the insert to form a QuorumCertificate.
    AlreadyAQuorum,

    /// A signature associated with the provided PublicAddr was already inserted into this QuorumCertificateAggregator.
    /// was a no-op.
    PublicAddrAlreadyProvidedSignature,

    /// The provided PublicAddr is not in the ParticipantSet provided during the construction of this
    /// QuorumCertificateAggregator.
    PublicAddrNotInParticipantSet,
}

/// A list of Signatures, ordered according to the arithmetic order of the PublicAddr associated with the SecretKey that
/// produced them. This special ordering makes it so that an explicit mapping between PublicAddr and Signature can be
/// omitted from SignatureSet's bytes-encoding, saving message and storage size.
#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct SignatureSet {
    pub signatures: Vec<Option<Signature>>,

    /// How many `Some`s are in signatures.
    pub count: usize,
} 

impl SignatureSet {
    pub fn new(length: usize) -> SignatureSet {
        let signatures = vec![None; length];
        SignatureSet {
            signatures,
            count: 0,
        }
    }

    /// The caller has the responsibility to ensure that Signatures in the SignatureSet are sorted in ascending order of the
    /// PublicAddr that produced them, i.e., the n-th item in SignatureSet, if Some, was produced by the SecretKey corresponding
    /// to the 'length - n' numerically largest Participant in a ParticipantSet. By imposing an order on SignatureSet, mappings
    /// between PublicAddr and Signature can be omitted from SignatureSet's bytes-encoding, saving message and storage size.
    pub fn insert(&mut self, index: usize, signature: Signature) -> Result<(), AlreadyInsertedError> {
        if self.signatures[index].is_some() {
            Err(AlreadyInsertedError)
        } else {
            self.signatures[index] = Some(signature);
            self.count += 1;
            Ok(())
        }
    }

    /// Returns whether the signature at the provided index is Some. 
    pub fn contains(&mut self, index: usize) -> bool {
        self.signatures[index].is_some()
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

pub struct AlreadyInsertedError;
