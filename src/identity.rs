use std::collections::BTreeMap;
use std::net::IpAddr;
use crate::msg_types::Signature;

pub const MY_PUBLIC_ADDR: PublicAddr = [0u8; 32]; // TODO.
pub const MY_SECRET_KEY: SecretKey = SecretKey([0u8; 32]); // TODO.
pub static STATIC_PARTICIPANT_SET: ParticipantSet = todo!();

pub type PublicAddr = [u8; 32];

/// ParticipantSet is a one-to-many mapping between PublicAddr and IpAddr in ascending order of PublicAddr: `get_index(n)` returns
/// the (PublicAddr, IpAddr) pair with the 'length - n'-th numerically-largest PublicAddr. This property simplifies SignatureSet
/// verification.
pub type ParticipantSet = BTreeMap<PublicAddr, IpAddr>;

pub struct SecretKey([u8; 32]);

impl SecretKey {
    pub fn sign(&self, bs: &[u8]) -> Signature {
        todo!()
    }
}