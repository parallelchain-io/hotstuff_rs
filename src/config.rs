use std::time::Duration;
use std::net::IpAddr;
use crate::identity::{SecretKey, PublicAddr, ParticipantSet};

/// Configuration as specified by the operator. This is split up into smaller, subsystem specific config structs
/// before being passed to components.
#[derive(Clone)]
pub struct Configuration {
    pub identity: IdentityConfig,
    pub node_tree: NodeTreeConfig,
    pub progress_mode: ProgressModeConfig,
    pub ipc: IPCConfig,
}

#[derive(Clone)]
pub struct IdentityConfig {
    pub my_secret_key: SecretKey,
    pub my_public_addr: PublicAddr,
    pub static_participant_set: ParticipantSet,
}

#[derive(Clone, Debug)]
pub struct NodeTreeConfig {
    pub db_path: String, 
}

pub struct NodeTreeApiConfig {
    pub listening_addr: IpAddr,
    pub listening_port: u16,
}

#[derive(Clone)]
pub struct ProgressModeConfig {
    pub target_node_time: Duration,
}

#[derive(Clone)]
pub struct IPCConfig {
    pub listening_addr: IpAddr,
    pub listening_port: u16,
    pub initiator_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub expected_worst_case_net_latency: Duration,
    pub reader_channel_buffer_len: usize,
    pub writer_channel_buffer_len: usize,
}
