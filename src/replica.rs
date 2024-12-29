/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Methods for building and running a replica as well as initializing its storage.
//!
//! HotStuff-rs works to safely replicate a state machine in multiple processes. In our terminology,
//! these processes are called 'replicas', and therefore the set of all replicas is called the
//! 'replica set'. Each replica is uniquely identified by an Ed25519
//! [`VerifyingKey`](crate::types::crypto_primitives::VerifyingKey).
//!
//! They key components of this module are:
//! - The builder-pattern interface to construct a [specification of the replica](ReplicaSpec) with:
//!   1. `ReplicaSpec::builder` to construct a `ReplicaSpecBuilder`,
//!   2. The setters of the `ReplicaSpecBuilder`, and
//!   3. The `ReplicaSpecBuilder::build` method to construct a [`ReplicaSpec`],
//! - The function to [start](ReplicaSpec::start) a [`Replica`] given its specification,
//! - The function to [initialize](Replica::initialize) the replica's [Block Tree](crate::block_tree),
//! - [The type](Replica) which keeps the replica alive.
//!
//! # Kinds of replicas
//!
//! At any given moment a Replica could either be a Validator, or a Listener, depending on whether it
//! is currently allowed to take part in consensus decisions:
//! - **Validators**: replicas that currently take part in consensus decisions.
//! - **Listeners**: replicas that currently do not take part in consensus decisions, but rather
//!  merely replicates the block tree.
//!
//! As the definition above implies, the **Validator Set** is dynamic, and will change as
//! [**validator set-updating**](crate::app#two-app-mutable-states-app-state-and-validator-set) blocks
//! are produced and committed.
//!
//! ## Becoming a Listener
//!
//! While HotStuff-rs keeps track of the current committed (and possibly the previous) validator set in
//! the persistent block tree, it does not keep track of a "Listener Set" anywhere, and nor can the
//! `App` ever specify "Listener Set Updates".
//!
//! This begs the question: "if HotStuff-rs does not know who the listeners are, how can the listeners
//! receive blocks and replicate the block tree?"
//!
//! In order for listeners to replicate the block tree, the library user should make sure that the
//! `broadcast` method of the [networking implementation](crate::networking) it provides sends messages to
//! **all** the peers the replica is connected to, and not only the validators. The library user is free
//! to implement their own mechanism or deciding which peers, besides those in the validator set, should
//! be connected to the network. This reduces the process of becoming a listener to the process of
//! becoming a peer in the user-provided network.
//! messages.
//!
//! # Starting a replica
//!
//! Below is an example that demonstrates how to build and start running a replica using the builder pattern:
//!
//! ```ignore
//! let replica =
//!     ReplicaSpec::builder()
//!     .app(app)
//!     .pacemaker(pacemaker)
//!     .configuration(configuration)
//!     .kv_store(kv_store)
//!     .network(network)
//!     .on_commit(commit_handler)
//!     .on_nudge(nudge_handler)
//!     .build()
//!     .start()
//! ```
//!
//! ## Required setters
//!
//! The required setters are for providing the trait implementations required to run a replica:
//! - `.app(...)`
//! - `.network(...)`
//! - `.kv_store(...)`
//! - `.pacemaker(...)`
//! - `.configuration(...)`
//!
//! ## Optional setters
//!
//! The optional setters are for registering user-defined event handlers for events from [crate::events]:
//! - `.on_insert_block(...)`
//! - `.on_commit_block(...)`
//! - `.on_prune_block(...)`
//! - `.on_update_highest_pc(...)`
//! - `.on_update_locked_view(...)`
//! - `.on_update_validator_set(...)`
//! - `.on_propose(...)`
//! - `.on_nudge(...)`
//! - `.on_phase_vote(...)`
//! - `.on_new_view(...)`
//! - `.on_receive_proposal(...)`
//! - `.on_receive_nudge(...)`
//! - `.on_receive_phase_vote(...)`
//! - `.on_receive_new_view(...)`
//! - `.on_start_view(...)`
//! - `.on_view_timeout(...)]`
//! - `.on_collect_pc(...)`
//! - `.on_start_sync(...)`
//! - `.on_end_sync(...)`
//! - `.on_receive_sync_request(...)`
//! - `.on_receive_sync_response(...)`
//!
//! The replica's [configuration](Configuration) can also be defined using the builder pattern, for example:
//!
//! ```ignore
//! let configuration =
//!     Configuration::builder()
//!     .me(keypair)
//!     .chain_id(0)
//!     .sync_request_limit(10)
//!     .sync_response_timeout(Duration::new(3, 0))
//!     .progress_msg_buffer_capacity(1024)
//!     .log_events(true)
//!     .build()
//! ```

use std::{thread::JoinHandle, time::Duration};

use ed25519_dalek::SigningKey;
use typed_builder::TypedBuilder;

use crate::{
    algorithm::Algorithm,
    app::App,
    block_sync::{
        client::BlockSyncClientConfiguration,
        server::{BlockSyncServer, BlockSyncServerConfiguration},
    },
    block_tree::{
        accessors::{internal::BlockTreeSingleton, public::BlockTreeCamera},
        pluggables::KVStore,
    },
    event_bus::*,
    events::*,
    hotstuff::implementation::HotStuffConfiguration,
    networking::{network::Network, receiving::start_polling},
    pacemaker::implementation::PacemakerConfiguration,
    types::{
        crypto_primitives::Keypair,
        data_types::{BufferSize, ChainID, EpochLength},
        update_sets::AppStateUpdates,
        validator_set::ValidatorSetState,
    },
};
use std::sync::mpsc::{self, Sender};

/// Stores the user-defined parameters required to start the replica, that is:
/// 1. The replica's [keypair](ed25519_dalek::SigningKey).
/// 2. The [chain ID](crate::types::data_types::ChainID) of the target blockchain.
/// 3. The sync request limit, which determines how many blocks should the replica request from its peer
///    when syncing.
/// 4. The sync response timeout (in seconds), which defines the maximum amount of time after which the
///    replica should wait for a sync response.
/// 5. The progress message buffer capacity, which defines the maximum allowed capacity of the progress
///    message buffer. If this capacity is about to be exceeded, some messages might be removed to make
///    space for new messages.
/// 6. The length of an "epoch", i.e., a sequence of views such that at the end of every such sequence
///    replica should try to synchronise views with others via an all-to-all broadcast.
/// 7. The maximum view time, which defines the duration after which the replica should timeout in the
///    current view and move to the next view (unless the replica is synchronising views with other
///    replicas at the end of an epoch).
/// 8. The "Log Events" flag, if set to "true" then logs should be printed.
///
/// ## Chain ID
///
/// Each HotStuff-rs blockchain should be identified by a [chain ID](crate::types::data_types::ChainID).
/// This is included in votes and other messages so that replicas don't mistake messages and blocks for
/// one HotStuff-rs blockchain does not get mistaken for those for another blockchain. In most cases,
/// having a chain ID that collides with another blockchain is harmless. But if your application is a
/// Proof of Stake public blockchain, this may cause a slashable offence if you operate validators in
/// two chains that use the same keypair. So ensure that you don't operate a validator in two
/// blockchains with the same keypair.
///
/// ## Sync response timeout
///
/// Durations stored in [`Configuration::block_sync_response_timeout`] must be "well below"
/// [`u64::MAX`] seconds. A good limit is to cap them at [u32::MAX].
///
/// In the most popular target platforms, Durations can only go up to [`u64::MAX`] seconds, so keeping
/// returned durations lower than [`u64::MAX`] avoids overflows in calling code, which may add to the
/// returned duration.
///
/// ## Log Events
///
/// HotStuff-rs logs using the [log](https://docs.rs/log/latest/log/) crate. To get these messages
/// printed onto a terminal or to a file, set up a [logging
/// implementation](https://docs.rs/log/latest/log/#available-logging-implementations).
#[derive(TypedBuilder)]
#[builder(builder_method(doc = "
    Create a builder for building a [Configuration]. On the builder call the following methods to construct a valid [Configuration].
        
    Required:
    - `.me(...)`
    - `.chain_id(...)`
    - `.sync_request_limit(...)`
    - `.sync_response_timeout(...)`
    - `.progress_msg_buffer_capacity(...)`
    - `.epoch_length(...)`
    - `.max_view_time(...)`
    - `.log_events(...)`
"))]
pub struct Configuration {
    #[builder(setter(doc = "Set the replica's keypair, used to sign messages. Required."))]
    pub me: SigningKey,
    #[builder(setter(doc = "Set the chain ID of the blockchain. Required."))]
    pub chain_id: ChainID,
    #[builder(setter(
        doc = "Set the limit for the number of blocks that a replica can request from its peer when syncing. Required."
    ))]
    pub block_sync_request_limit: u32,
    #[builder(setter(
        doc = "Set how frequently the sync server should advertise its Highest Committed Block and Highest PC. Required."
    ))]
    pub block_sync_server_advertise_time: Duration,
    #[builder(setter(
        doc = "Set the timeout for receiving a sync response from a peer. Required."
    ))]
    pub block_sync_response_timeout: Duration,
    #[builder(setter(
        doc = "Set the time after which a block sync server blacklisting should expire. Required."
    ))]
    pub block_sync_blacklist_expiry_time: Duration,
    #[builder(setter(
        doc = "Set the number of views a PC received by the sync client must be ahead of the current view in order to trigger sync (event-based sync trigger). Required."
    ))]
    pub block_sync_trigger_min_view_difference: u64,
    #[builder(setter(
        doc = "Set the time that needs to pass without any progress (i.e., updating the Highest PC) or sync attempts in order to trigger sync (timeout-based sync trigger). Required."
    ))]
    pub block_sync_trigger_timeout: Duration,
    #[builder(setter(
        doc = "Set the maximum number of bytes that can be stored in the replica's message buffer at any given moment. Required."
    ))]
    pub progress_msg_buffer_capacity: BufferSize,
    #[builder(setter(
        doc = "Set the epoch length i.e., if epoch length is n, then replicas synchronize views via all-to-all broadcast 
    every n views. Required."
    ))]
    pub epoch_length: EpochLength,
    #[builder(setter(
        doc = "Set the maximum duration that should be allocated to each view. Required."
    ))]
    pub max_view_time: Duration,
    #[builder(setter(doc = "Enable logging? Required."))]
    pub log_events: bool,
}

impl
    Into<(
        HotStuffConfiguration,
        PacemakerConfiguration,
        BlockSyncClientConfiguration,
        BlockSyncServerConfiguration,
    )> for Configuration
{
    fn into(
        self,
    ) -> (
        HotStuffConfiguration,
        PacemakerConfiguration,
        BlockSyncClientConfiguration,
        BlockSyncServerConfiguration,
    ) {
        let keypair = Keypair::new(self.me);
        let hotstuff_config = HotStuffConfiguration {
            chain_id: self.chain_id,
            keypair: keypair.clone(),
        };
        let pacemaker_config = PacemakerConfiguration {
            chain_id: self.chain_id,
            keypair: keypair.clone(),
            epoch_length: self.epoch_length,
            max_view_time: self.max_view_time,
        };
        let block_sync_client_config = BlockSyncClientConfiguration {
            chain_id: self.chain_id,
            request_limit: self.block_sync_request_limit,
            response_timeout: self.block_sync_response_timeout,
            blacklist_expiry_time: self.block_sync_blacklist_expiry_time,
            block_sync_trigger_min_view_difference: self.block_sync_trigger_min_view_difference,
            block_sync_trigger_timeout: self.block_sync_trigger_timeout,
        };
        let block_sync_server_config = BlockSyncServerConfiguration {
            chain_id: self.chain_id,
            keypair: keypair.clone(),
            request_limit: self.block_sync_request_limit,
            advertise_time: self.block_sync_server_advertise_time,
        };
        (
            hotstuff_config,
            pacemaker_config,
            block_sync_client_config,
            block_sync_server_config,
        )
    }
}

/// Stores all necessary parameters and trait implementations required to run the [Replica].
#[derive(TypedBuilder)]
#[builder(builder_method(doc = "
    Create a builder for building a [ReplicaSpec]. On the builder call the following methods to construct a valid [ReplicaSpec].
    
    Required:
    - `.app(...)`
    - `.network(...)`
    - `.kv_store(...)`
    - `.configuration(...)`
    
    Optional:
    - `.on_insert_block(...)`
    - `.on_commit_block(...)`
    - `.on_prune_block(...)`
    - `.on_update_highest_pc(...)`
    - `.on_update_locked_pc(...)`
    - `.on_update_locked_tc(...)`
    - `.on_update_validator_set(...)`
    - `.on_propose(...)`
    - `.on_nudge(...)`
    - `.on_phase_vote(...)`
    - `.on_new_view(...)`
    - `.on_timeout_vote(...)`
    - `.on_advance_view(...)`
    - `.on_receive_proposal(...)`
    - `.on_receive_nudge(...)`
    - `.on_receive_phase_vote(...)`
    - `.on_receive_new_view(...)`
    - `.on_receive_timeout_vote(...)`
    - `.on_receive_advance_view(...)`
    - `.on_start_view(...)`
    - `.on_view_timeout(...)`
    - `.on_collect_pc(...)`
    - `.on_collect_tc(...)`
    - `.on_start_sync(...)`
    - `.on_end_sync(...)`
    - `.on_receive_sync_request(...)`
    - `.on_receive_sync_response(...)`
"))]
pub struct ReplicaSpec<K: KVStore, A: App<K> + 'static, N: Network + 'static> {
    // Required parameters
    #[builder(setter(
        doc = "Set the application code to be run on the blockchain. The argument must implement the [`App`](crate::app::App) trait. Required."
    ))]
    app: A,
    #[builder(setter(
        doc = "Set the implementation of peer-to-peer networking. The argument must implement the [`Network`](crate::networking::network::Network) trait. Required."
    ))]
    network: N,
    #[builder(setter(
        doc = "Set the implementation of the replica's Key-Value store. The argument must implement the [`KVStore`](crate::block_tree::pluggables::KVStore) trait. Required."
    ))]
    kv_store: K,
    #[builder(setter(
        doc = "Set the [configuration](Configuration), which contains the necessary parameters to run a replica. Required."
    ))]
    configuration: Configuration,
    // Optional parameters
    #[builder(default, setter(transform = |handler: impl Fn(&InsertBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<InsertBlockEvent>),
    doc = "Register a handler closure to be invoked after a block is inserted to the replica's block tree. Optional."))]
    on_insert_block: Option<HandlerPtr<InsertBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&CommitBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<CommitBlockEvent>),
    doc = "Register a handler closure to be invoked after a block is committed. Optional."))]
    on_commit_block: Option<HandlerPtr<CommitBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&PruneBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<PruneBlockEvent>),
    doc = "Register a handler closure to be invoked after a block is pruned, i.e., its siblings are removed from the replica's block tree. Optional."))]
    on_prune_block: Option<HandlerPtr<PruneBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateHighestPCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateHighestPCEvent>),
    doc = "Register a handler closure to be invoked after the replica updates its highest PC. Optional."))]
    on_update_highest_pc: Option<HandlerPtr<UpdateHighestPCEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateLockedPCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateLockedPCEvent>),
    doc = "Register a handler closure to be invoked after the replica updates its locked PC. Optional."))]
    on_update_locked_pc: Option<HandlerPtr<UpdateLockedPCEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateHighestTCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateHighestTCEvent>),
    doc = "Register a handler closure to be invoked after the replica updates its highest TC. Optional."))]
    on_update_highest_tc: Option<HandlerPtr<UpdateHighestTCEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateValidatorSetEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateValidatorSetEvent>),
    doc = "Register a handler closure to be invoked after the replica updates its validator set. Optional."))]
    on_update_validator_set: Option<HandlerPtr<UpdateValidatorSetEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ProposeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ProposeEvent>),
    doc = "Register a handler closure to be invoked after the replica broadcasts a proposal for a block. Optional."))]
    on_propose: Option<HandlerPtr<ProposeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&NudgeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<NudgeEvent>),
    doc = "Register a handler closure to be invoked after the replica broadcasts a nudge for a block. Optional."))]
    on_nudge: Option<HandlerPtr<NudgeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&PhaseVoteEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<PhaseVoteEvent>),
    doc = "Register a handler closure to be invoked after the replica sends a `PhaseVote`. Optional."))]
    on_phase_vote: Option<HandlerPtr<PhaseVoteEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&NewViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<NewViewEvent>),
    doc = "Register a handler closure to be invoked after the replica sends a new view message to the next leader. Optional."))]
    on_new_view: Option<HandlerPtr<NewViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&TimeoutVoteEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<TimeoutVoteEvent>),
    doc = "Register a handler closure to be invoked after the replica sends a `TimeoutVote`. Optional."))]
    on_timeout_vote: Option<HandlerPtr<TimeoutVoteEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&AdvanceViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<AdvanceViewEvent>),
    doc = "Register a handler closure to be invoked after the replica sends an advance view message. Optional."))]
    on_advance_view: Option<HandlerPtr<AdvanceViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveProposalEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveProposalEvent>),
    doc = "Register a handler closure to be invoked after the replica receives a proposal for a block. Optional."))]
    on_receive_proposal: Option<HandlerPtr<ReceiveProposalEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveNudgeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveNudgeEvent>),
    doc = "Register a handler closure to be invoked after the replica receives a nudge for a block. Optional."))]
    on_receive_nudge: Option<HandlerPtr<ReceiveNudgeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceivePhaseVoteEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceivePhaseVoteEvent>),
    doc = "Register a handler closure to be invoked after the replica receives a `PhaseVote`. Optional."))]
    on_receive_phase_vote: Option<HandlerPtr<ReceivePhaseVoteEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveNewViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveNewViewEvent>),
    doc = "Register a handler closure to be invoked after the replica receives a new view message. Optional."))]
    on_receive_new_view: Option<HandlerPtr<ReceiveNewViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveTimeoutVoteEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveTimeoutVoteEvent>),
    doc = "Register a handler closure to be invoked after the replica receives a `TimeoutVote`. Optional."))]
    on_receive_timeout_vote: Option<HandlerPtr<ReceiveTimeoutVoteEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveAdvanceViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveAdvanceViewEvent>),
    doc = "Register a handler closure to be invoked after the replica receives an advance view message. Optional."))]
    on_receive_advance_view: Option<HandlerPtr<ReceiveAdvanceViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&StartViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<StartViewEvent>),
    doc = "Register a handler closure to be invoked after the replica enters a new view. Optional."))]
    on_start_view: Option<HandlerPtr<StartViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ViewTimeoutEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ViewTimeoutEvent>),
    doc = "Register a handler closure to be invoked after the replica's view times out. Optional."))]
    on_view_timeout: Option<HandlerPtr<ViewTimeoutEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&CollectPCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<CollectPCEvent>),
    doc = "Register a handler closure to be invoked after the replica collects a new `PhaseCertificate`. Optional."))]
    on_collect_pc: Option<HandlerPtr<CollectPCEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&CollectTCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<CollectTCEvent>),
    doc = "Register a handler closure to be invoked after the replica collects a new timeout certificate. Optional."))]
    on_collect_tc: Option<HandlerPtr<CollectTCEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&StartSyncEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<StartSyncEvent>),
    doc = "Register a handler closure to be invoked after the replica starts syncing, exiting the progress mode. Optional."))]
    on_start_sync: Option<HandlerPtr<StartSyncEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&EndSyncEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<EndSyncEvent>),
    doc = "Register a handler closure to be invoked after the replica finishes syncing, returning to progress mode. Optional."))]
    on_end_sync: Option<HandlerPtr<EndSyncEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveSyncRequestEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveSyncRequestEvent>),
    doc = "Register a handler closure to be invoked after the replica receives a sync request from a peer. Optional."))]
    on_receive_sync_request: Option<HandlerPtr<ReceiveSyncRequestEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&SendSyncResponseEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<SendSyncResponseEvent>),
    doc = "Register a handler closure to be invoked after the replica sends a sync response to a peer. Optional."))]
    on_send_sync_response: Option<HandlerPtr<SendSyncResponseEvent>>,
}

impl<K: KVStore, A: App<K> + 'static, N: Network + 'static> ReplicaSpec<K, A, N> {
    /// Starts all threads and channels associated with running a replica, and returns the handles to them in a [Replica] struct.
    pub fn start(mut self) -> Replica<K> {
        let block_tree = BlockTreeSingleton::new(self.kv_store.clone());
        self.network.init_validator_set(
            block_tree
                .committed_validator_set()
                .expect("No validator set is committed!")
                .clone(),
        );

        let chain_id = self.configuration.chain_id;
        let progress_msg_buffer_capacity = self.configuration.progress_msg_buffer_capacity;
        let log_events = self.configuration.log_events;
        let (hotstuff_config, pacemaker_config, block_sync_client_config, block_sync_server_config) =
            self.configuration.into();

        let (poller_shutdown, poller_shutdown_receiver) = mpsc::channel();
        let (poller, progress_msgs, block_sync_requests, block_sync_responses) =
            start_polling(self.network.clone(), poller_shutdown_receiver);

        let event_handlers = EventHandlers::new(
            log_events,
            self.on_insert_block,
            self.on_commit_block,
            self.on_prune_block,
            self.on_update_highest_pc,
            self.on_update_locked_pc,
            self.on_update_highest_tc,
            self.on_update_validator_set,
            self.on_propose,
            self.on_nudge,
            self.on_phase_vote,
            self.on_new_view,
            self.on_timeout_vote,
            self.on_advance_view,
            self.on_receive_proposal,
            self.on_receive_nudge,
            self.on_receive_phase_vote,
            self.on_receive_new_view,
            self.on_receive_timeout_vote,
            self.on_receive_advance_view,
            self.on_start_view,
            self.on_view_timeout,
            self.on_collect_pc,
            self.on_collect_tc,
            self.on_start_sync,
            self.on_end_sync,
            self.on_receive_sync_request,
            self.on_send_sync_response,
        );

        let (event_publisher, event_subscriber) = if !event_handlers.is_empty() {
            Some(mpsc::channel()).unzip()
        } else {
            (None, None)
        };

        let (block_sync_server_shutdown, block_sync_server_shutdown_receiver) = mpsc::channel();
        let block_sync_server = BlockSyncServer::new(
            block_sync_server_config,
            BlockTreeCamera::new(self.kv_store.clone()),
            block_sync_requests,
            self.network.clone(),
            block_sync_server_shutdown_receiver,
            event_publisher.clone(),
        );
        let block_sync_server = block_sync_server.start();

        let (algorithm_shutdown, algorithm_shutdown_receiver) = mpsc::channel();
        let algorithm = Algorithm::new(
            chain_id,
            hotstuff_config,
            pacemaker_config,
            block_sync_client_config,
            block_tree,
            self.app,
            self.network.clone(),
            progress_msgs,
            progress_msg_buffer_capacity,
            block_sync_responses,
            algorithm_shutdown_receiver,
            event_publisher,
        );
        let algorithm = algorithm.start();

        let (event_bus_shutdown, event_bus_shutdown_receiver) = if !event_handlers.is_empty() {
            Some(mpsc::channel()).unzip()
        } else {
            (None, None)
        };

        let event_bus = if !event_handlers.is_empty() {
            Some(start_event_bus(
                event_handlers,
                event_subscriber.unwrap(), // Safety: should be Some(...).
                event_bus_shutdown_receiver.unwrap(), // Safety: should be Some(...).
            ))
        } else {
            None
        };

        Replica {
            block_tree_camera: BlockTreeCamera::new(self.kv_store),
            poller: Some(poller),
            poller_shutdown,
            algorithm: Some(algorithm),
            algorithm_shutdown,
            block_sync_server: Some(block_sync_server),
            block_sync_server_shutdown,
            event_bus,
            event_bus_shutdown,
        }
    }
}

/// A handle to the background threads of a HotStuff-rs replica. When this value is dropped, all
/// background threads are gracefully shut down.
pub struct Replica<K: KVStore> {
    block_tree_camera: BlockTreeCamera<K>,
    poller: Option<JoinHandle<()>>,
    poller_shutdown: Sender<()>,
    algorithm: Option<JoinHandle<()>>,
    algorithm_shutdown: Sender<()>,
    block_sync_server: Option<JoinHandle<()>>,
    block_sync_server_shutdown: Sender<()>,
    event_bus: Option<JoinHandle<()>>,
    event_bus_shutdown: Option<Sender<()>>,
}

impl<K: KVStore> Replica<K> {
    /// Initializes the replica's [block tree](crate::block_tree) with the intial
    /// [app state updates](crate::types::update_sets::AppStateUpdates) and
    /// [validator set updates](crate::types::update_sets::ValidatorSetUpdates).
    pub fn initialize(
        kv_store: K,
        initial_app_state: AppStateUpdates,
        initial_validator_set_state: ValidatorSetState,
    ) {
        let mut block_tree = BlockTreeSingleton::new(kv_store);
        block_tree
            .initialize(&initial_app_state, &initial_validator_set_state)
            .expect("Block Tree initialization failed!")
    }

    /// Get a [`BlockTreeCamera`].
    pub fn block_tree_camera(&self) -> &BlockTreeCamera<K> {
        &self.block_tree_camera
    }
}

impl<K: KVStore> Drop for Replica<K> {
    fn drop(&mut self) {
        // Safety: the order of thread shutdown in this function is important, as the threads make assumptions
        // about the validity of their channels based on this. In particular, the algorithm and block sync
        // server threads receive messages from the poller, and assumes that the poller will live longer than
        // them.

        self.event_bus_shutdown
            .iter()
            .for_each(|shutdown| shutdown.send(()).unwrap());
        if self.event_bus.is_some() {
            self.event_bus.take().unwrap().join().unwrap();
        }

        self.algorithm_shutdown.send(()).unwrap();
        self.algorithm.take().unwrap().join().unwrap();

        self.block_sync_server_shutdown.send(()).unwrap();
        self.block_sync_server.take().unwrap().join().unwrap();

        self.poller_shutdown.send(()).unwrap();
        self.poller.take().unwrap().join().unwrap();
    }
}
