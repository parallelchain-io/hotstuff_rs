use std::collections::HashMap;
use std::net::IpAddr;
use crate::msg_types::{Signature};

pub const MY_PUBLIC_ADDR: PublicAddr = todo!();
pub const MY_SECRET_KEY: Signature = todo!();
pub const STATIC_PARTICIPANT_SET: ParticipantSet = todo!();

pub type PublicAddr = [u8; 32];
pub type SecretKey = [u8; 32];
pub type ParticipantSet = HashMap<PublicAddr, IpAddr>;
