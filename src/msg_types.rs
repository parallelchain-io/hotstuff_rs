use std::array;
use std::mem;
use sha2::Digest;
use sha2::Sha256;
use ed25519_dalek::Signature as DalekSignature;
use crate::identity::{PublicAddr, ParticipantSet};

/// Messages sent between Participants on IPC channels to drive BlockTree construction forward.
#[derive(Clone)]
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

/// The first byte of the wire-encoding of ConsensusMsg. Disambiguates whether the following message should be 
/// deserialized into a Proposal, a Vote, or a NewView.
type VariantPrefix = [u8; 1];

impl ConsensusMsg {
    pub const PREFIX_PROPOSE: VariantPrefix = [0u8];
    pub const PREFIX_VOTE: VariantPrefix = [1u8];
    pub const PREFIX_NEW_VIEW: VariantPrefix = [2u8]; 
}

impl SerDe for ConsensusMsg {
    // # Encodings
    //
    // ## Propose
    // PREFIX_PROPOSE 
    // ++ vn.to_le_bytes()
    // ++ block.serialize()
    // 
    // ## Vote
    // PREFIX_VOTE
    // ++ vn.to_le_bytes()
    // ++ block_hash
    // ++ signature
    //
    // ## NewView
    // PREFIX_NEW_VIEW
    // ++ vn.to_le_bytes()
    // ++ qc.serialize()
    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::Propose(vn, block) => {
                buf.extend_from_slice(&Self::PREFIX_PROPOSE);
                buf.extend_from_slice(&vn.to_le_bytes());
                buf.extend_from_slice(&block.serialize());
            },
            Self::Vote(vn, block_hash, signature) => {
                buf.extend_from_slice(&Self::PREFIX_VOTE);
                buf.extend_from_slice(&vn.to_le_bytes());
                buf.extend_from_slice(block_hash);
                buf.extend_from_slice(&signature.to_bytes());
            },
            Self::NewView(vn, qc) => {
                buf.extend_from_slice(&Self::PREFIX_NEW_VIEW);
                buf.extend_from_slice(&vn.to_le_bytes());
                buf.extend_from_slice(&qc.serialize());
            }
        }

        return buf
    }

    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError> {
        let mut cursor = 0usize;

        let variant_prefix = bs[cursor..mem::size_of::<VariantPrefix>()].try_into()?;
        cursor += mem::size_of::<VariantPrefix>();

        let vn = u64::from_le_bytes(bs[cursor..mem::size_of::<ViewNumber>()].try_into()?); 
        cursor += mem::size_of::<ViewNumber>();
        match variant_prefix {
            Self::PREFIX_PROPOSE => {
                let (bytes_read, block) = Block::deserialize(&bs[cursor..])?;
                cursor += bytes_read;
                Ok((cursor, Self::Propose(vn, block)))
            },
            Self::PREFIX_VOTE => {
                let block_hash = bs[cursor..mem::size_of::<BlockHash>()].try_into()?;
                cursor += mem::size_of::<BlockHash>();

                let signature = <[u8; 64]>::try_from(&bs[cursor..64]).unwrap().into();
                cursor += mem::size_of::<Signature>();
                Ok((cursor, Self::Vote(vn, block_hash, signature)))
            },
            Self::PREFIX_NEW_VIEW => {
                let (bytes_read, qc) = QuorumCertificate::deserialize(&bs[cursor..])?;
                cursor += bytes_read;
                Ok((cursor, Self::NewView(vn, qc)))
            },
            _ => Err(DeserializationError) 
        }


    }
}

/// A bundle of data 'chained' onto another like bundle of data--referred to as its 'parent'--by containing a
/// cryptographic certificate that attests to the parent having been accepted and stored by a quorum of Participants.
#[derive(Clone)]
pub struct Block {
    /// A cryptographic hash over (height ++ justify ++ commands). Used as a unique identifier of a Block that is short
    /// and therefore to cheap to send over the wire. 
    pub hash: BlockHash,

    /// How many justify-links separate this Block from the Genesis Block.
    pub height: BlockHeight,

    pub justify: QuorumCertificate,
    pub commands: CommandList,
}

impl Block {
    pub fn hash(height: BlockHeight, commands: &Vec<Vec<u8>>, justify: &QuorumCertificate) -> BlockHash {
        Sha256::new()
            .chain_update(height.to_le_bytes())
            .chain_update(commands.serialize())
            .chain_update(justify.serialize())
            .finalize()
            .into()
    }
}

impl SerDe for Block {
    fn serialize(&self) -> Vec<u8> {
        let mut bs = Vec::new();
        bs.extend_from_slice(&self.hash);
        bs.extend_from_slice(&self.height.to_le_bytes());
        bs.extend_from_slice(&self.commands.serialize());
        bs.extend_from_slice(&self.justify.serialize());
        bs
    }

    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError> {
        todo!()
    }
}

pub type ViewNumber = u64;

pub type BlockHeight = u64;

pub const NODE_HASH_LEN: usize = 32;
pub type BlockHash = [u8; NODE_HASH_LEN];

/// A list of App-provided Commands. Commands are stored in Blocks as a delineated, indexable list so that
/// applications can quickly get the Command sitting a particular index in a Block using 
/// `BlockTree::get_block_command_by_hash` or `get_block_command_by_height` instead of getting the entire list
/// of Commands at once, which might be an unacceptable expensive I/O operation.
pub type CommandList = Vec<Command>;

/// Arbitrary data provided by an App as part of the return value of `create_leaf`. 
pub type Command = Vec<u8>;

/// An aggregate of multiple `ConsensusMsg::Vote`s. A valid QuorumCertificate is proof that a quorum of
/// Participants has inserted the branch headed by the Block identified by the QuorumCertificate's `block
/// _hash` into its local BlockTree. The more QuorumCertificates 'justifies' a Block, the closer the Block
/// to being considered committed. If you are familiar with Bitcoin, 'justification' via QuorumCertificate
/// is similar to 'confirmation' by Blocks.
/// 
/// A QuorumCertificate is 'valid', i.e., it can safely be inserted into the BlockTree, if its is_quorum
/// and is_cryptographically_correct methods return true. 
#[derive(Clone)]
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

impl SerDe for QuorumCertificate {
    fn serialize(&self) -> Vec<u8> {
        let mut bs = Vec::new();
        bs.extend_from_slice(&self.view_number.to_le_bytes());
        bs.extend_from_slice(&self.block_hash);
        bs.extend_from_slice(&self.sigs.serialize());

        return bs
    }

    fn deserialize(bs: &[u8]) -> Result<(BytesRead, QuorumCertificate), DeserializationError> {
        let mut cursor = 0;

        let vn = u64::from_le_bytes(bs[cursor..mem::size_of::<u64>()].try_into()?);
        cursor += mem::size_of::<u64>();

        let block_hash = bs[cursor..mem::size_of::<BlockHash>()].try_into()?;
        cursor += mem::size_of::<BlockHash>();

        let (bytes_read, sigs) = SignatureSet::deserialize(&bs[40..])?;
        cursor += bytes_read;

        let qc = QuorumCertificate {
            view_number: vn,
            block_hash,
            sigs,
        };

        Ok((cursor, qc))
    }
}

impl SerDe for CommandList {
    fn serialize(&self) -> Vec<u8> {
        todo!() 
    }

    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError> {
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
#[derive(Clone)]
pub struct SignatureSet {
    pub signatures: Vec<Option<Signature>>,

    /// How many `Some`s are in signatures.
    pub count: usize,
} 

impl SignatureSet {
    pub const SOME_PREFIX: u8 = 1;
    pub const NONE_PREFIX: u8 = 0; 

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

impl SerDe for SignatureSet {
    // Encoding:
    // `participant_set.len()` as u64
    // ++
    // for each signature in signatures:
    //     if Some:
    //         1 ++ signature
    //     if None: 
    //         0 
    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Number counts both Some and None signatures.
        let num_sigs = self.signatures.len().to_le_bytes();
        buf.extend_from_slice(&num_sigs);

        for signature in &self.signatures {
            match signature {
                Some(sig) => { 
                    buf.push(Self::SOME_PREFIX);
                    buf.extend_from_slice(&sig.to_bytes());
                },
                None => {
                    buf.push(Self::NONE_PREFIX);
                }
            }
        }
        buf
    }

    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError> {
        let mut signatures = Vec::new();

        let mut cursor = 0usize;
        let num_sigs = u64::from_le_bytes(bs[0..mem::size_of::<u64>()].try_into().unwrap());
        cursor += mem::size_of::<u64>();

        let mut count = 0;
        for _ in 0..num_sigs {
            let variant_prefix = u8::from_le_bytes(bs[cursor..mem::size_of::<u8>()].try_into().unwrap());
            cursor += mem::size_of::<u8>();
            match variant_prefix {
                Self::SOME_PREFIX => {
                    let sig = <Signature>::try_from(&bs[cursor..64]).unwrap().into();
                    cursor += mem::size_of::<Signature>();
                    signatures.push(Some(sig));
                    count += 1;
                }, 
                Self::NONE_PREFIX => {
                    signatures.push(None);
                },
                _ => return Err(DeserializationError)
            }
        }
        
        let sig_set = SignatureSet {
            signatures,
            count,
        };

        Ok((cursor, sig_set))
    }
}

pub type Signature = DalekSignature;

/// Methods that convert to and from bytes-encodings.
pub trait SerDe: Sized {
    fn serialize(&self) -> Vec<u8>;


    /// Reads from the beginning of the provided buffer to try and produce an instance of the implementor type.
    /// 
    /// The fact that deserialize returns the number of bytes read from the provided buffer to produce an instance of the implementor
    /// simplifies SerDe implementation on types that are products of types that also implement SerDe.
    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError>;
}

pub type BytesRead = usize;

#[derive(Debug)]
pub struct DeserializationError;

impl From<array::TryFromSliceError> for DeserializationError {
    fn from(_: array::TryFromSliceError) -> Self {
        DeserializationError
    }
}

impl<T: SerDe> SerDe for Vec<T> {
    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError> {
        let mut cursor = 0;

        let num_elems = u64::from_le_bytes(bs[cursor..mem::size_of::<u64>()].try_into()?);
        let mut res = Vec::with_capacity(num_elems as usize);

        for _ in 0..num_elems {
            let (bytes_read, elem) = T::deserialize(&bs[cursor..])?;  
            cursor += bytes_read;
            res.push(elem);
        }

        Ok((cursor, res))
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        let num_elems = self.len() as u64;
        buf.extend_from_slice(&num_elems.to_le_bytes());

        for elem in self {
            buf.append(&mut elem.serialize())
        }

        buf
    }
}
