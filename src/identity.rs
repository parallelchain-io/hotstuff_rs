use std::collections::BTreeMap;
use std::net::IpAddr;
use ed25519_dalek::Keypair as DalekKeypair;

/// A set of (PublicAddr, IpAddr) pairs, indexable by PublicAddr. It is a BTreeMap instead of, say, a HashMap, so that we can
/// we can iterate through it in ascending order of PublicAddr. This implifies SignatureSet verification.
pub type ParticipantSet = BTreeMap<PublicAddr, IpAddr>;

/// An Ed25519 Keypair. This is used by the Participant to sign Vote messages.
pub type KeyPair = DalekKeypair;

/// An Ed25519 Public Address. This uniquely identifies Participants.
pub type PublicAddr = [u8; 32];
