use std::collections::BTreeMap;
use std::net::IpAddr;
use ed25519_dalek::{Keypair as DalekKeypair, PublicKey as DalekPublicKey};

/// A set of (PublicAddr, IpAddr) pairs, indexable by PublicAddr. It is a BTreeMap instead of, say, a HashMap, so that we can
/// we can iterate through it in ascending order of PublicAddr. This implifies SignatureSet verification.
pub type ParticipantSet = BTreeMap<PublicKeyBytes, IpAddr>;

/// An Ed25519 Keypair. This is used by the Participant to sign Vote messages.
pub type KeyPair = DalekKeypair;

/// An Ed25519 Public Key. This uniquely identifies Participants.
pub type PublicKey = DalekPublicKey;

/// The backing byte-sequence behind PublicKey. This is defined separately because PublicKey does not implement Hash, which we
/// need to use it as a key in the [IndexMaps](indexmap::IndexMap) used in IPC.
pub type PublicKeyBytes = [u8; 32];
