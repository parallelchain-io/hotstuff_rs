use std::collections::BTreeMap;
use std::net::IpAddr;
use ed25519_dalek::{
    Keypair as DalekKeypair,
    SecretKey as DalekSecretKey,
};

/// the (PublicAddr, IpAddr) pair with the 'length - n'-th numerically-largest PublicAddr. This property simplifies SignatureSet
/// verification.
pub type ParticipantSet = BTreeMap<PublicAddr, IpAddr>;

pub type KeyPair = DalekKeypair;

pub type PublicAddr = [u8; 32];

pub type SecretKey = DalekSecretKey;
