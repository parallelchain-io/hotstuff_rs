use std::ops::Deref;
use std::io;
use std::slice;
use std::mem;
use borsh::{BorshSerialize, BorshDeserialize};
use ed25519_dalek::Signer;
use ed25519_dalek::Verifier;
use sha2::Digest;
use sha2::Sha256;
use ed25519_dalek::{Signature as DalekSignature, SIGNATURE_LENGTH};
use crate::identity::KeyPair;
use crate::identity::PublicKey;
use crate::identity::{PublicKeyBytes, ParticipantSet};

pub(crate) type ViewNumber = u64;

/// Documented as a field of [Block].
pub type AppID = u64;

/// Documented as a field of [Block].
pub type BlockHeight = u64;

/// Documented as a field of [Block].
pub type BlockHash = [u8; BLOCK_HASH_LEN];
pub const BLOCK_HASH_LEN: usize = 32;

/// Documented as a field of [Block].
pub type Data = Vec<Datum>;

/// Documented as a field of [Block].
pub type Datum = Vec<u8>;

/// Documented as a field of [Block].
pub type DataHash = [u8; DATA_HASH_LEN];
pub const DATA_HASH_LEN: usize = 32;

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

impl ConsensusMsg {
    pub(crate) fn new_vote(view_number: ViewNumber, block_hash: BlockHash, keypair: &KeyPair) -> ConsensusMsg {
        let signature = {
            let mut msg = Vec::with_capacity(mem::size_of::<ViewNumber>() + mem::size_of::<BlockHash>());
            msg.extend_from_slice(&view_number.to_le_bytes());
            msg.extend_from_slice(&block_hash);
            Signature(keypair.sign(&msg))
        };

        ConsensusMsg::Vote(view_number, block_hash, signature)
    }
}

/// A bundle of data 'chained' onto another like bundle of data--referred to as its 'parent'--by containing a
/// cryptographic certificate that attests to the parent having been accepted and stored by a quorum of Participants.
/// 
/// # [Data] vs [Datum]
/// [App](crate::app::App)s return a Data as part of the return value of their [propose_block](crate::app::App::propose_block)
/// method. These are included verbatim in the `data` field of Blocks, and can contain arbitrary sequence of bytes
/// with application-specific meaning.
///  
/// But Data can be arbitrarily large, and applications may want to get only small chunks of Data at a time from the
/// BlockTree. For example, a Blockchain App may want to get a single Transaction from a Block to serve an HTTP request
/// for that particular Transaction. 
/// 
/// To support this use case without having to load and deserialize an entire, arbitrarily large Data from the BlockTree,
/// Data is a `Vec<Datum>`. Applications can get *only* a particular Datum from the BlockTree using BlockTreeSnapshot's  
/// [get_block_datum](crate::block_tree::BlockTreeSnapshot::get_block_datum_by_hash) method.
/// 
/// # Why must App provide [DataHash]?
/// Some hash functions have properties that can be useful, sometimes even essential, for some applications. 
///
/// For example, Blockchains rely on [Merkle Tree](https://en.wikipedia.org/wiki/Merkle_tree) hashing to support succinct
/// cryptographic proofs that a Transaction is included in a Block *without* having to send an entire Block over the wire.
/// For the curious, the way this works is explained [here](https://en.bitcoinwiki.org/wiki/Simplified_Payment_Verification).
/// 
/// Having Apps return a DataHash as part of the return value of `propose_block` allows applications to choose the hash 
/// function or construction that fits their needs best.
#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct Block {
    /// A number which distinguishes between the Blocks of different HotStuff-rs networks.
    pub app_id: AppID,

    /// A SHA-256 Hash over a Block's (`app_id` ++ `height` ++ `justify` ++ `data_hash`).
    pub hash: BlockHash,

    /// The number of justify-links that separate this Block from the Genesis Block.
    pub height: BlockHeight,
    
    /// A cryptographic certificate which links a Block with its direct ancestor.
    pub justify: QuorumCertificate,

    /// A cryptographic hash over the Block's Data, provided by the App as part of the return value of its methods.
    pub data_hash: DataHash,

    /// A list of App-provided Datums.
    pub data: Data,
}

impl Block {
    pub const PARENT_OF_GENESIS_BLOCK_HASH: BlockHash = [0u8; BLOCK_HASH_LEN];
    pub const GENESIS_BLOCK_HEIGHT: BlockHeight = 0;

    pub fn new(app_id: AppID, height: BlockHeight, justify: QuorumCertificate, data_hash: DataHash, data: Data) -> Block {
        let hash = Self::hash(app_id, height, &justify, &data_hash);
        Block {
            app_id,
            hash,
            height,
            justify,
            data_hash,
            data,
        }
    }

    pub fn hash(app_id: AppID, height: BlockHeight, justify: &QuorumCertificate, data_hash: &DataHash) -> BlockHash {
        Sha256::new()
            .chain_update(app_id.try_to_vec().unwrap())
            .chain_update(height.try_to_vec().unwrap())
            .chain_update(justify.try_to_vec().unwrap())
            .chain_update(data_hash.try_to_vec().unwrap())
            .finalize()
            .into()
    }
}

/// An aggregate of multiple [ConsensusMsg::Vote]s. A valid QuorumCertificate is proof that a quorum of
/// Participants has inserted the branch headed by the Block identified by the QuorumCertificate's `block
/// _hash` into its local BlockTree. The more QuorumCertificates 'justifies' a Block, the closer the Block
/// to being considered committed. If you are familiar with Bitcoin, 'justification' via QuorumCertificate
/// is similar to 'confirmation' by Blocks.
/// 
/// A QuorumCertificate is 'valid', i.e., it can safely be inserted into the BlockTree, if its \
/// [is_correct_len](QuorumCertificate::is_correct_len), is_quorum, and is_cryptographically_correct methods return true. 
#[derive(BorshSerialize, BorshDeserialize, Clone, PartialEq, Eq)]
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
    pub(crate) fn quorum_size(num_participants: usize) -> usize {
        // '/' here is integer (floor) division. 
        (num_participants * 2) / 3 + 1
    }

    pub(crate) fn is_valid(&self, participant_set: &ParticipantSet) -> bool {
        if self.view_number == 0 {
            self == &Self::genesis_qc(participant_set.len())
        } else {
            self.is_correct_len(participant_set.len())
                && self.is_quorum(participant_set.len())
                && self.is_cryptographically_correct(participant_set)
        }
    }

    pub(crate) fn genesis_qc(num_participants: usize) -> QuorumCertificate {
        QuorumCertificate {
            view_number: 0,
            block_hash: Block::PARENT_OF_GENESIS_BLOCK_HASH,
            sigs: SignatureSet::new(num_participants),
        }
    }

    pub(crate) fn is_correct_len(&self, num_participants: usize) -> bool {
        self.sigs.len() > num_participants
    }

    /// Returns whether this QuorumCertificate contains at least a quorum of signatures, correct or incorrect. 
    fn is_quorum(&self, num_participants: usize) -> bool {
        self.sigs.count_some() >= Self::quorum_size(num_participants) 
    }

    /// Returns whether all Signatures in this QuorumCertificate were produced over (view_number ++ block_hash)
    /// by the SecretKey associated with a PublicAddr in the provided ParticipantSet.
    fn is_cryptographically_correct(&self, participant_set: &ParticipantSet) -> bool {
        let msg = {
            let mut buf = Vec::with_capacity(mem::size_of::<ViewNumber>() + mem::size_of::<BlockHash>()); 
            buf.extend_from_slice(&self.view_number.to_le_bytes());
            buf.extend_from_slice(&self.block_hash);
            buf
        };

        self.sigs.iter()
            .zip(participant_set.iter())
            .all(|(signature, (public_key_bs, _))| match signature {
                Some(signature) => {
                        let public_key = PublicKey::from_bytes(public_key_bs)
                            .expect("Configuration error: static ParticipantSet contains invalid PublicKey");
                        public_key.verify(&msg, signature).is_ok()
                    },
                    None => true,
            })
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
    /// ## Errors
    /// Read the documentation of QCAggregatorInsertError. If an error is returned, this function was a no-op.
    pub fn insert(&mut self, signature: Signature, of_public_addr: PublicKeyBytes) -> Result<bool, QCAggregatorInsertError> {
        if self.signature_set.count_some() >= QuorumCertificate::quorum_size(self.participant_set.len()) {
            return Err(QCAggregatorInsertError::AlreadyAQuorum)
        }
        if let Some(position) = self.participant_set.keys().position(|pk| *pk == of_public_addr) {
            if self.signature_set.contains(position) {
                return Err(QCAggregatorInsertError::PublicAddrAlreadyProvidedSignature);
            }
            let _ = self.signature_set.insert(position, signature);
    
            Ok(self.signature_set.count_some() >= QuorumCertificate::quorum_size(self.participant_set.len()))
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
    PublicAddrAlreadyProvidedSignature,

    /// The provided PublicAddr is not in the ParticipantSet provided during the construction of this
    /// QuorumCertificateAggregator.
    PublicAddrNotInParticipantSet,
}

/// A list of Signatures, ordered according to the arithmetic order of the PublicAddr associated with the SecretKey that
/// produced them. This special ordering makes it so that an explicit mapping between PublicAddr and Signature can be
/// omitted from SignatureSet's bytes-encoding, saving message and storage size.
#[derive(BorshSerialize, BorshDeserialize, Clone, PartialEq, Eq)]
pub struct SignatureSet {
    pub signatures: Vec<Option<Signature>>,

    /// How many `Some`s are in signatures.
    pub count_some: usize,
} 

impl SignatureSet {
    pub fn new(len: usize) -> SignatureSet {
        let signatures = vec![None; len];
        SignatureSet {
            signatures,
            count_some: 0,
        }
    }

    /// The caller has the responsibility to ensure that Signatures in the SignatureSet are sorted in ascending order of the
    /// PublicAddr that produced them.
    pub fn insert(&mut self, index: usize, signature: Signature) -> Result<(), AlreadyInsertedError> {
        if self.signatures[index].is_some() {
            Err(AlreadyInsertedError)
        } else {
            self.signatures[index] = Some(signature);
            self.count_some += 1;
            Ok(())
        }
    }

    /// Get an iterator through this SignatureSet's `signatures` vector.
    pub fn iter(&self) -> slice::Iter<Option<Signature>> {
        self.signatures.iter()     
    }

    /// Returns the length of this SignatureSet's `signatures` vector. 
    pub fn len(&self) -> usize {
        self.signatures.len()
    }

    /// Returns how many `Some(Signature)` are in this SignatureSet's `signatures` vector.
    pub fn count_some(&self) -> usize {
        self.count_some
    }

    /// Returns whether the signature at the provided index is Some. 
    pub fn contains(&mut self, index: usize) -> bool {
        self.signatures[index].is_some()
    }
}

pub struct AlreadyInsertedError;

#[derive(Clone, PartialEq, Eq)]
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
