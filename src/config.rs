use std::{time::Duration, path::PathBuf};
use std::net::IpAddr;
use hotstuff_rs_types::identity::{PublicKey, KeyPair, ParticipantSet};
use hotstuff_rs_types::messages::AppID;

/// Configuration as specified by the operator. This is split up into smaller, subsystem specific config structs
/// before being passed to components.
pub struct Configuration {
    pub identity: IdentityConfig,
    pub block_tree_storage: BlockTreeStorageConfig,
    pub algorithm: AlgorithmConfig,
    pub networking: NetworkingConfiguration,
}

/// Configuration related to the cryptographic identities of consensus Participants.
#[derive(Debug)]
pub struct IdentityConfig {
    pub my_keypair: KeyPair,
    /// my_public_addr must be the equivalent to the PublicKey component of my_keypair.
    pub my_public_key: PublicKey,
    pub static_participant_set: ParticipantSet,
}

/// Configuration related to the storage of the local BlockTree.
#[derive(Clone)]
pub struct BlockTreeStorageConfig {
    /// The path, local to the binary's working directory, where BlockTree will store its persistent database files.
    pub db_path: PathBuf, 
}

/// Configuration related to the Algorithm State Machine.
#[derive(Clone)]
pub struct AlgorithmConfig {
    /// A number which uniquely identifies the specific HotStuff-rs network that this Participant operates in.
    pub app_id: AppID,

    /// How long should a view be at most. This has to be at least 2 times the worst case expected network latency.
    pub target_block_time: Duration,

    /// How long should App execute and validate Blocks in Sync Mode.
    pub sync_mode_execution_timeout: Duration,
}

/// Configuration related to networking in the Protocol State Machine.
#[derive(Clone)]
pub struct NetworkingConfiguration {
    pub progress_mode: ProgressModeNetworkingConfig,
    pub sync_mode: SyncModeNetworkingConfig,
}

/// Configuration related to networking in the Progress Mode of the Algorithm State Machine.
#[derive(Clone)]
pub struct ProgressModeNetworkingConfig {
    /// The IP address that Progress Mode's IPC will wait on for new TCP connections initiated by other Participants.
    pub listening_addr: IpAddr,

    /// The port that Progress Mode's IPC will wait on for new TCP connectiosn initiated by other Participants.
    pub listening_port: u16,

    /// How long much time should Progress Mode IPC expend to form a (single) TCP connection?
    pub initiator_timeout: Duration,

    /// How long should Progress Mode IPC wait for new bytes to come in from a TCP connection before deciding that it has 
    /// probably failed and dropping it?
    pub read_timeout: Duration,

    /// How long should Progress Mode IPC wait to get its bytes acknowledged by the other side of a TCP connection before
    /// deciding that it has probably failed and dropping it?
    pub write_timeout: Duration,

    /// Expected worst case network latency. This is used to set execution deadlines in Progress Mode. If expected worst
    /// case network latency is high, then less time will be allocated for execution, in order to give messages more time
    /// to arrive in their destination.
    pub expected_worst_case_net_latency: Duration,

    pub reader_channel_buffer_len: usize,
    pub writer_channel_buffer_len: usize,
}

/// Configuration related to networking in the Sync Mode of the Algorithm State Machine.
#[derive(Clone)]
pub struct SyncModeNetworkingConfig {
    pub request_jump_size: usize,
    pub request_timeout: Duration,

    /// The IP address that the BlockTree HTTP API will listen on.
    pub block_tree_api_listening_addr: IpAddr,

    /// The port that the BlockTree HTTP API will listen on.
    pub block_tree_api_listening_port: u16,
}
