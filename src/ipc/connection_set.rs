use std::collections::HashMap;
use std::net::TcpStream;
use crate::msg_types::PublicAddress;

pub type ConnectionSet = HashMap<PublicAddress, TcpStream>;
