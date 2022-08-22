use std::collections::HashMap;
use std::net::IpAddr;
use crate::msg_types::PublicAddr;

pub type ParticipantSet = HashMap<PublicAddr, IpAddr>;