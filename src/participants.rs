use std::collections::HashMap;
use std::net::SocketAddr;
use crate::msg_types::PublicAddr;

pub type ParticipantSet = HashMap<PublicAddr, SocketAddr>;