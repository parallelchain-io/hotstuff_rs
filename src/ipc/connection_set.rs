use std::sync::Arc;
use std::collections::HashMap;
use crate::msg_types::PublicAddress;
use crate::ipc::RwTcpStream;

pub(crate) type ConnectionSet = HashMap<PublicAddress, Arc<RwTcpStream>>;
