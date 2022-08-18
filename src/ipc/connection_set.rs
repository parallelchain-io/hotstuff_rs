use std::sync::Arc;
use std::collections::HashMap;
use std::net::TcpStream;
use crate::msg_types::PublicAddress;
use crate::ipc::CRwLock;

pub(crate) type ConnectionSet = HashMap<PublicAddress, Arc<CRwLock<TcpStream>>>;
