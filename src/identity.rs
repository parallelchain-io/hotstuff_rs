use std::collections::HashMap;
use std::net::IpAddr;
use crate::msg_types::Signature;

pub const MY_PUBLIC_ADDR: PublicAddr = todo!();
pub const MY_SECRET_KEY: SecretKey = todo!();
pub const STATIC_PARTICIPANT_SET: ParticipantSet = todo!();

pub type PublicAddr = [u8; 32];
pub type ParticipantSet = HashMap<PublicAddr, IpAddr>;

pub struct SecretKey([u8; 32]);

impl SecretKey {
    pub fn sign(&self, bs: &[u8]) -> Signature {
        todo!()
    }
}