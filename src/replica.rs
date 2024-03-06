/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Methods to build, run, and initialize the storage of a replica.
//!
//! HotStuff-rs works to safely replicate a state machine in multiple processes. In our terminology, these processes are
//! called 'replicas', and therefore the set of all replicas is called the 'replica set'. Each replica is uniquely identified
//! by an [Ed25519 public key](ed25519_dalek::VerifyingKey).
//! 
//! They key components of this module are:
//! - The builder-pattern interface to construct a [specification of the replica](ReplicaSpec) with:
//!   1. `ReplicaSpec::builder` to construct a `ReplicaSpecBuilder`, 
//!   2. The setters of the `ReplicaSpecBuilder`, and 
//!   3. The `ReplicaSpecBuilder::build` method to construct a [ReplicaSpec],
//! - The function to [start](ReplicaSpec::start) a [Replica] given its specification,
//! - The function to [initialize](Replica::initialize) the replica's [Block Tree](crate::state::BlockTree), 
//! - [The type](Replica) which keeps the replica alive.
//! 
//! ## Validators and Listeners
//!
//! Not every replica has to vote in consensus. Some operators may want to run replicas that merely keep up with consensus
//! decisions, without having weight in them. We call these replicas 'listeners', and call the replicas that vote in
//! consensus 'validators'.
//!
//! HotStuff-rs needs to know the full 'validator set' at all times to collect votes, but does not need to know the identity
//! of listeners. But for listeners to keep up with consensus decisions, they also need to receive progress messages.
//!
//! Concretely, this requires that the library user's [networking provider's](crate::networking) broadcast method send progress
//! messages to all peers it is connected to, and not only the validators. The library user is free to design and implement
//! their own mechanism for deciding which peers, besides those in the validator set, should be connected to the network.
//! 
//! ## Starting a replica
//! 
//! Here is an example that demonstrates how to build and start running a replica using the builder pattern:
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
//! ### Required setters
//! 
//! The required setters are for providing the trait implementations required to run a replica:
//! - `.app(...)`
//! - `.network(...)`
//! - `.kv_store(...)`
//! - `.pacemaker(...)`
//! - `.configuration(...)`
//! 
//! ### Optional setters
//! 
//! The optional setters are for registering user-defined event handlers for events from [crate::events]:
//! - `.on_insert_block(...)`
//! - `.on_commit_block(...)`
//! - `.on_prune_block(...)`
//! - `.on_update_highest_qc(...)`
//! - `.on_update_locked_view(...)`
//! - `.on_update_validator_set(...)`
//! - `.on_propose(...)`
//! - `.on_nudge(...)`
//! - `.on_vote(...)`
//! - `.on_new_view(...)`
//! - `.on_receive_proposal(...)`
//! - `.on_receive_nudge(...)`
//! - `.on_receive_vote(...)`
//! - `.on_receive_new_view(...)`
//! - `.on_start_view(...)`
//! - `.on_view_timeout(...)]`
//! - `.on_collect_qc(...)`
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

use std::thread::JoinHandle;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use typed_builder::TypedBuilder;

use crate::algorithm::Algorithm;
use crate::app::App;
use crate::block_sync::client::BlockSyncClientConfiguration;
use crate::block_sync::server::BlockSyncServer;
use crate::block_sync::server::BlockSyncServerConfiguration;
use crate::event_bus::*;
use crate::events::*;
use crate::hotstuff::protocol::HotStuffConfiguration;
use crate::networking::{start_polling, Network};
use crate::pacemaker::protocol::PacemakerConfiguration;
use crate::state::{BlockTree, BlockTreeCamera, KVStore};
use crate::types::basic::AppStateUpdates;
use crate::types::basic::BufferSize;
use crate::types::basic::ChainID;
use crate::types::basic::EpochLength;
use crate::types::keypair::Keypair;
use crate::types::validators::ValidatorSetUpdates;
use std::sync::mpsc::{self, Sender};

/// Stores the user-defined parameters required to start the replica, that is:
/// 1. The replica's [keypair](ed25519_dalek::SigningKey).
/// 2. The [chain ID](crate::types::basic::ChainID) of the target blockchain.
/// 3. The sync request limit, which determines how many blocks should the replica request from its peer when syncing.
/// 4. The sync response timeout (in seconds), which defines the maximum amount of time after which the replica should wait for a sync response.
/// 5. The progress message buffer capacity, which defines the maximum allowed capacity of the progress message buffer. If this capacity
///    is about to be exceeded, some messages might be removed to make space for new messages.
/// 6. The length of an "epoch", i.e., a sequence of views such that at the end of every such sequence replica should try to synchronise views with others
///    via an all-to-all broadcast.
/// 7. The maximum view time, which defines the duration after which the replica should timeout in the current view and move to the next view 
///    (unless the replica is synchronising views with other replicas at the end of an epoch).
/// 8. The "Log Events" flag, if set to "true" then logs should be printed. 
/// 
/// ## Chain ID
/// 
/// Each HotStuff-rs blockchain should be identified by a [chain ID](crate::types::basic::ChainID). This
/// is included in votes and other messages so that replicas don't mistake messages and blocks for
/// one HotStuff-rs blockchain does not get mistaken for those for another blockchain.
/// In most cases, having a chain ID that collides with another blockchain is harmless. But
/// if your application is a Proof of Stake public blockchain, this may cause a slashable offence
/// if you operate validators in two chains that use the same keypair. So ensure that you don't
/// operate a validator in two blockchains with the same keypair.
///
/// ## Sync response timeout
/// 
/// Durations stored in [Configuration::block_sync_response_timeout] must be "well below"
/// [u64::MAX] seconds. A good limit is to cap them at [u32::MAX].
/// 
/// In the most popular target platforms, Durations can only go up to [u64::MAX] seconds, so keeping returned
/// durations lower than [u64::MAX] avoids overflows in calling code, which may add to the returned duration.
/// 
/// ## Log Events
/// 
/// HotStuff-rs logs using the [log](https://docs.rs/log/latest/log/) crate. To get these messages
/// printed onto a terminal or to a file, set up a [logging
/// implementation](https://docs.rs/log/latest/log/#available-logging-implementations).
#[derive(TypedBuilder)]
#[builder(builder_method(doc = 
    "
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
"
))]
pub struct Configuration {
    #[builder(setter(doc = "Set the replica's keypair, used to sign messages. Required."))]
    pub me: SigningKey,
    #[builder(setter(doc = "Set the chain ID of the blockchain. Required."))]
    pub chain_id: ChainID,
    #[builder(setter(doc = "Set the limit for the number of blocks that a replica can request from its peer when syncing. Required."))]
    pub block_sync_request_limit: u32,
    #[builder(setter(doc = "Set the timeout for receiving a sync response from a peer (in seconds). Required."))]
    pub block_sync_response_timeout: Duration,
    #[builder(setter(doc = "Set the maximum number of bytes that can be stored in the replica's message buffer at any given moment. Required."))]
    pub progress_msg_buffer_capacity: BufferSize,
    #[builder(setter(doc = "Set the epoch length i.e., if epoch length is n, then replicas synchronize views via all-to-all broadcast 
    every n views. Required."))]
    pub epoch_length: EpochLength,
    #[builder(setter(doc = "Set the maximum duration that should be allocated to each view. Required."))]
    pub max_view_time: Duration,
    #[builder(setter(doc = "Enable logging? Required."))]
    pub log_events: bool,
}

impl Into<(HotStuffConfiguration, PacemakerConfiguration, BlockSyncClientConfiguration, BlockSyncServerConfiguration)> for Configuration {
    fn into(self) -> (HotStuffConfiguration, PacemakerConfiguration, BlockSyncClientConfiguration, BlockSyncServerConfiguration) {
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
        };
        let block_sync_server_config = BlockSyncServerConfiguration {
            chain_id: self.chain_id,
            keypair: keypair.clone(),
            request_limit: self.block_sync_request_limit,
        };
        (
            hotstuff_config,
            pacemaker_config,
            block_sync_client_config,
            block_sync_server_config
        )
    }
}

/// Stores all necessary parameters and trait implementations required to run the [Replica].
#[derive(TypedBuilder)]
#[builder(builder_method(doc = 
    "
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
    - `.on_update_highest_qc(...)`
    - `.on_update_locked_view(...)`
    - `.on_update_validator_set(...)`
    - `.on_propose(...)`
    - `.on_nudge(...)`
    - `.on_vote(...)`
    - `.on_new_view(...)`
    - `.on_receive_proposal(...)`
    - `.on_receive_nudge(...)`
    - `.on_receive_vote(...)`
    - `.on_receive_new_view(...)`
    - `.on_start_view(...)`
    - `.on_view_timeout(...)`
    - `.on_collect_qc(...)`
    - `.on_start_sync(...)`
    - `.on_end_sync(...)`
    - `.on_receive_sync_request(...)`
    - `.on_receive_sync_response(...)`
"
))]
pub struct ReplicaSpec<K: KVStore, A: App<K> + 'static, N: Network + 'static> {
    // Required parameters
    #[builder(setter(doc = "Set the application code to be run on the blockchain. The argument must implement the [App](crate::app::App) trait. Required."))]
    app: A,
    #[builder(setter(doc = "Set the implementation of peer-to-peer networking. The argument must implement the [Network](crate::network::Network) trait. Required."))]
    network: N,
    #[builder(setter(doc = "Set the implementation of the replica's Key-Value store. The argument must implement the [KVStore](crate::state::KVStore) trait. Required."))]
    kv_store: K,
    #[builder(setter(doc = "Set the [configuration](Configuration), which contains the necessary parameters to run a replica. Required."))]
    configuration: Configuration,
    // Optional parameters
    #[builder(default, setter(transform = |handler: impl Fn(&InsertBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<InsertBlockEvent>),
    doc = "Register a handler closure to be invoked after a block is inserted to the replica's [Block Tree](crate::state::BlockTree). Optional."))]
    on_insert_block: Option<HandlerPtr<InsertBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&CommitBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<CommitBlockEvent>),
    doc = "Register a handler closure to be invoked after a block is committed. Optional."))]
    on_commit_block: Option<HandlerPtr<CommitBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&PruneBlockEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<PruneBlockEvent>),
    doc = "Register a handler closure to be invoked after a block is pruned, i.e., its siblings are removed from the replica's [Block Tree](crate::state::BlockTree). Optional."))]
    on_prune_block: Option<HandlerPtr<PruneBlockEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateHighestQCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateHighestQCEvent>),
    doc = "Register a handler closure to be invoked after the replica updates its highest QC. Optional."))]
    on_update_highest_qc: Option<HandlerPtr<UpdateHighestQCEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateLockedViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateLockedViewEvent>),
    doc = "Register a handler closure to be invoked after the replica updates its locked view. Optional."))]
    on_update_locked_view: Option<HandlerPtr<UpdateLockedViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&UpdateValidatorSetEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<UpdateValidatorSetEvent>),
    doc = "Register a handler closure to be invoked after the replica updates its validator set. Optional."))]
    on_update_validator_set: Option<HandlerPtr<UpdateValidatorSetEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ProposeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ProposeEvent>),
    doc = "Register a handler closure to be invoked after the replica broadcasts a proposal for a block. Optional."))]
    on_propose: Option<HandlerPtr<ProposeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&NudgeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<NudgeEvent>),
    doc = "Register a handler closure to be invoked after the replica broadcasts a nudge for a block. Optional."))]
    on_nudge: Option<HandlerPtr<NudgeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&VoteEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<VoteEvent>),
    doc = "Register a handler closure to be invoked after the replica sends a vote. Optional."))]
    on_vote: Option<HandlerPtr<VoteEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&NewViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<NewViewEvent>),
    doc = "Register a handler closure to be invoked after the replica sends a new view message to the next leader. Optional."))]
    on_new_view: Option<HandlerPtr<NewViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveProposalEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveProposalEvent>),
    doc = "Register a handler closure to be invoked after the replica receives a proposal for a block. Optional."))]
    on_receive_proposal: Option<HandlerPtr<ReceiveProposalEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveNudgeEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveNudgeEvent>),
    doc = "Register a handler closure to be invoked after the replica receives a nudge for a block. Optional."))]
    on_receive_nudge: Option<HandlerPtr<ReceiveNudgeEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveVoteEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveVoteEvent>),
    doc = "Register a handler closure to be invoked after the replica receives a vote. Optional."))]
    on_receive_vote: Option<HandlerPtr<ReceiveVoteEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ReceiveNewViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ReceiveNewViewEvent>),
    doc = "Register a handler closure to be invoked after the replica receives a new view message. Optional."))]
    on_receive_new_view: Option<HandlerPtr<ReceiveNewViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&StartViewEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<StartViewEvent>),
    doc = "Register a handler closure to be invoked after the replica enters a new view. Optional."))]
    on_start_view: Option<HandlerPtr<StartViewEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&ViewTimeoutEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<ViewTimeoutEvent>),
    doc = "Register a handler closure to be invoked after the replica's view times out. Optional."))]
    on_view_timeout: Option<HandlerPtr<ViewTimeoutEvent>>,
    #[builder(default, setter(transform = |handler: impl Fn(&CollectQCEvent) + Send + 'static| Some(Box::new(handler) as HandlerPtr<CollectQCEvent>),
    doc = "Register a handler closure to be invoked after the replica collects a new quorum certificate. Optional."))]
    on_collect_qc: Option<HandlerPtr<CollectQCEvent>>,
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
        let block_tree = BlockTree::new(self.kv_store.clone());
        self.network.init_validator_set(block_tree.committed_validator_set().clone());

        let chain_id = self.configuration.chain_id;
        let progress_msg_buffer_capacity = self.configuration.progress_msg_buffer_capacity;
        let log_events = self.configuration.log_events;
        let (hotstuff_config,
            pacemaker_config, 
            block_sync_client_config, 
            block_sync_server_config
        ) = self.configuration.into();

        let (poller_shutdown, poller_shutdown_receiver) = mpsc::channel();
        let (poller, progress_msgs, block_sync_requests, block_sync_responses) =
            start_polling(self.network.clone(), poller_shutdown_receiver);

        let event_handlers = EventHandlers::new(
            log_events,
            self.on_insert_block,
            self.on_commit_block,
            self.on_prune_block,
            self.on_update_highest_qc,
            self.on_update_locked_view,
            self.on_update_validator_set,
            self.on_propose,
            self.on_nudge,
            self.on_vote,
            self.on_new_view,
            self.on_receive_proposal,
            self.on_receive_nudge,
            self.on_receive_vote,
            self.on_receive_new_view,
            self.on_start_view,
            self.on_view_timeout,
            self.on_collect_qc,
            self.on_start_sync,
            self.on_end_sync,
            self.on_receive_sync_request,
            self.on_send_sync_response
        );

        let (event_publisher, event_subscriber) = 
            if !event_handlers.is_empty() {
                Some(mpsc::channel()).unzip()
            } else { (None, None) };

        let (block_sync_server_shutdown, block_sync_server_shutdown_receiver) = mpsc::channel();
        let mut block_sync_server = BlockSyncServer::new(
            block_sync_server_config, 
            BlockTreeCamera::new(self.kv_store.clone()), 
            block_sync_requests, 
            self.network.clone(),
            block_sync_server_shutdown_receiver,
            event_publisher.clone()
        );
        let block_sync_server = block_sync_server.start();

        let (algorithm_shutdown, algorithm_shutdown_receiver) = mpsc::channel();
        let mut algorithm = Algorithm::new(
            chain_id,
            hotstuff_config,
            pacemaker_config,
            block_sync_client_config,
            block_tree,
            self.app,
            self.network.clone(), 
            progress_msgs, 
            block_sync_responses, 
            algorithm_shutdown_receiver, 
            event_publisher, 
            progress_msg_buffer_capacity, 
        );
        let algorithm = algorithm.start();
        
        let (event_bus_shutdown, event_bus_shutdown_receiver) =
            if !event_handlers.is_empty() {
                Some(mpsc::channel()).unzip()
            } else { (None, None) };

        let event_bus = if !event_handlers.is_empty() {
            Some(
                start_event_bus(
                    event_handlers,
                    event_subscriber.unwrap(), // Safety: should be Some(...).
                    event_bus_shutdown_receiver.unwrap(), // Safety: should be Some(...).
                )
            )
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

/// A handle to the background threads of a HotStuff-rs replica. When this value is dropped, all background threads are
/// gracefully shut down.
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
    /// Initializes the replica's [Block Tree](crate::state::BlockTree) with the intial
    /// [app state updates](crate::types::basic::AppStateUpdates) and [validator set updates](crate::types::validators::ValidatorSetUpdates).
    pub fn initialize(
        kv_store: K,
        initial_app_state: AppStateUpdates,
        initial_validator_set: ValidatorSetUpdates,
    ) {
        let mut block_tree = BlockTree::new(kv_store);
        block_tree.initialize(&initial_app_state, &initial_validator_set);
    }

    /// Returns a [Block Tree Camera](crate::state::BlockTreeCamera) which can be used to peek into
    /// the [Block Tree](crate::state::BlockTree).
    pub fn block_tree_camera(&self) -> &BlockTreeCamera<K> {
        &self.block_tree_camera
    }
}

impl<K: KVStore> Drop for Replica<K> {
    fn drop(&mut self) {
        // Safety: the order of thread shutdown in this function is important, as the threads make assumptions about
        // the validity of their channels based on this. The algorithm and sync server threads receive messages from
        // the poller, and assumes that the poller will live longer than them.

        self.event_bus_shutdown.iter().for_each(|shutdown| shutdown.send(()).unwrap());
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
