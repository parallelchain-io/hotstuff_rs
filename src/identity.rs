use std::collections::BTreeMap;
use std::net::IpAddr;
use ed25519_dalek::{
    Keypair as DalekKeypair,
    PublicKey as DalekPublicKey,
    SecretKey as DalekSecretKey,
};

/// the (PublicKey, IpAddr) pair with the 'length - n'-th numerically-largest PublicKey. This property simplifies SignatureSet
/// verification.
pub type ParticipantSet = BTreeMap<PublicKey, IpAddr>;

pub type KeyPair = DalekKeypair;

pub type PublicKey = DalekPublicKey;

pub type SecretKey = DalekSecretKey;
