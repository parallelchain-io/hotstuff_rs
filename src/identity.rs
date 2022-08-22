use std::collections::HashMap;
use std::net::IpAddr;
use crate::msg_types::{PublicAddr, Signature};

pub const MY_PUBLIC_ADDR: PublicAddr = todo!();
pub const MY_SIGNATURE: Signature = todo!();

pub type ParticipantSet = HashMap<PublicAddr, IpAddr>;