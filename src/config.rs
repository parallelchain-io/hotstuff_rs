use std::time::Duration;
use std::net::IpAddr;
use crate::identity::{SecretKey, PublicAddr, ParticipantSet};

/// Configuration as specified by the operator. This is split up into smaller, subsystem specific Configuration
/// before being passed to components.
struct Configuration {
    identity: IdentityConfiguration,
    node_tree: NodeTreeConfiguration,
    progress_mode: ProgressModeConfiguration,
    ipc: IPCConfiguration,
}

struct IdentityConfiguration {
    my_secret_key: SecretKey,
    my_public_addr: PublicAddr,
    static_participant_set: ParticipantSet,
}

struct NodeTreeConfiguration {
    db_path: String, 
}

struct ProgressModeConfiguration {
    target_node_time: Duration,
}

struct IPCConfiguration {
    listening_addr: IpAddr,
    listening_port: u16,
    initiator_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
    expected_worst_case_net_latency: Duration,
    read_channel_buffer_len: usize,
    writer_channel_buffer_len: usize,
}


