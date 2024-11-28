/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implements the [`BlockSyncClient`], which helps a replica catch up with the head of the blockchain
//! by requesting blocks from the sync server of another replica.
//!
//! The client is responsible for:
//! 1. Triggering Block Sync when:
//!     1. A replica has not made progress for a configurable amount of time.
//!     2. Or, sees evidence that others are ahead.
//! 2. Managing the list of peers available as sync servers and a blacklist for sync servers that have
//!    provided incorrect information in the past.
//! 3. Selecting a peer to sync with from the list of available peers.
//! 4. Attempting to sync with a given peer.
//!
//! ## Triggering Block Sync
//!
//! HotStuff-rs replicas implement two complementary and configurable block sync trigger mechanisms:
//! 1. Event-based sync trigger: on receiving an [`AdvertisePC`] message with a correct PC from the
//!    future. By how many views the received PC must be ahead of the current view to trigger sync can
//!    be configured by setting `block_sync_trigger_min_view_difference` in the replica
//!    [configuration](crate::replica::Configuration).
//! 2. Timeout-based sync trigger: when no "progress" is made for a sufficiently long time. Here
//!    "progress" is understood as updating the Highest PC stored in the [block tree](BlockTree) - in the
//!    context of the HotStuff SMR, updating the Highest PC means that the validators are achieving
//!    consensus and extending the blockchain. The amount of time without progress or sync attempts
//!    required to trigger sync can be configured by setting `block_sync_trigger_timeout` in the replica
//!    configuration.
//!
//! The two sync trigger mechanims offer fallbacks for different liveness-threatening scenarios that a
//! replica may face:
//! 1. The event-based sync trigger can help a replica catch up with the head of the blockchain in case
//!    there is a quorum of validators known to the replica making progress ahead, but the replica has
//!    fallen behind them in terms of view number.
//! 2. The timeout-based sync trigger can help a replica catch up in case there is either no quorum
//!    ahead (e.g., if others have also fallen out of sync with each other), or the validator set making
//!    progress ahead is unknown to the replica because it doesn't know about the most recent validator
//!    set updates. Note that in the latter case, sync will only be succesful if some of the sync peers
//!    known to the replica are up-to-date with the head of the blockchain. Otherwise, a manual sync
//!    attempt may be required to recover from this situation.
//!
//! ## Block Sync procedure
//!
//! When sync is triggered, the sync client picks a random sync server from its list of available sync
//! servers, and iteratively sends it sync requests and processes the received sync responses until
//! the sync is terminated. Sync can be terminated if either:
//! 1. The sync client reaches timeout waiting for a response,
//! 2. The response contains incorrect blocks,
//! 3. The response contains no blocks.
//!
//! In the first two cases, the sync server may be blacklisted if the above-mentioned behaviour is
//! inconsistent with the server's promise to provide blocks up to a given height, as conveyed
//! through its earlier [`AdvertiseBlock`] message.
//!
//! ## Available sync servers
//!
//! In general, available sync servers are current and potential validators that:
//! 1. Are "in sync" or ahead of the replica in terms of highest committed block height,
//! 2. Notify the replica's sync client about their availability,
//! 3. Have not provided false information to the block sync client within a certain period of time
//!    specified by the blacklist expiry time. By "false information" we mean incorrect blocks or
//!    incorrect highest committed block height (used to determine 1).
//!
//! Keeping track of which peers can be considered available sync servers is done by maintaining a
//! hashmap of available sync servers, and a queue of blacklisted sync servers together with their
//! expiry times.

use std::{
    collections::{HashMap, VecDeque},
    sync::mpsc::Sender,
    time::{Duration, Instant, SystemTime},
};

use ed25519_dalek::VerifyingKey;
use rand::seq::IteratorRandom;

use crate::{
    app::{App, ValidateBlockRequest, ValidateBlockResponse},
    block_sync::messages::{
        AdvertiseBlock, AdvertisePC, BlockSyncAdvertiseMessage, BlockSyncRequest,
    },
    block_tree::{
        accessors::internal::{BlockTreeError, BlockTreeSingleton},
        invariants::{safe_block, safe_pc},
        pluggables::KVStore,
    },
    events::{EndSyncEvent, Event, InsertBlockEvent, StartSyncEvent},
    networking::{
        network::{Network, ValidatorSetUpdateHandle},
        receiving::{BlockSyncClientStub, BlockSyncResponseReceiveError},
        sending::SenderHandle,
    },
    types::{
        block::Block,
        data_types::{BlockHeight, ChainID, ViewNumber},
        signed_messages::{Certificate, SignedMessage},
        update_sets::ValidatorSetUpdates,
        validator_set::ValidatorSetUpdatesStatus,
    },
};

pub(crate) struct BlockSyncClient<N: Network> {
    config: BlockSyncClientConfiguration,
    receiver: BlockSyncClientStub,
    sender: SenderHandle<N>,
    validator_set_update_handle: ValidatorSetUpdateHandle<N>,
    block_sync_client_state: BlockSyncClientState,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network> BlockSyncClient<N> {
    /// Create a new instance of the [BlockSyncClient].
    pub(crate) fn new(
        config: BlockSyncClientConfiguration,
        receiver: BlockSyncClientStub,
        sender: SenderHandle<N>,
        validator_set_update_handle: ValidatorSetUpdateHandle<N>,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        Self {
            config,
            receiver,
            sender,
            validator_set_update_handle,
            block_sync_client_state: BlockSyncClientState::initialize(),
            event_publisher,
        }
    }

    /// Process a received [`BlockSyncAdvertiseMessage`].
    pub(crate) fn on_receive_msg<K: KVStore>(
        &mut self,
        msg: BlockSyncAdvertiseMessage,
        origin: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
        app: &mut impl App<K>,
    ) -> Result<(), BlockSyncClientError> {
        match msg {
            BlockSyncAdvertiseMessage::AdvertiseBlock(advertise_block) => {
                self.on_receive_advertise_block(advertise_block, origin, block_tree)
            }
            BlockSyncAdvertiseMessage::AdvertisePC(advertise_pc) => {
                self.on_receive_advertise_pc(advertise_pc, origin, block_tree, app)
            }
        }
    }

    /// Update the [`BlockSyncClient`]'s internal state, and possibly trigger sync on reaching sync trigger
    /// timeout.
    pub(crate) fn tick<K: KVStore>(
        &mut self,
        block_tree: &mut BlockTreeSingleton<K>,
        app: &mut impl App<K>,
    ) -> Result<(), BlockSyncClientError> {
        // 1. Check if any blacklistings have expired, and if so remove the expired blacklistings
        //    from the blacklist.
        self.block_sync_client_state
            .remove_expired_blacklisted_servers();

        // 2. Update highest_pc_view if needed.
        let highest_pc_view = block_tree.highest_pc()?.view;
        if highest_pc_view > self.block_sync_client_state.highest_pc_view {
            self.block_sync_client_state.highest_pc_view = highest_pc_view;
            self.block_sync_client_state.last_progress_or_sync_time = Instant::now();
        }

        // 3. Check if sync should be triggered due to timeout, and if yes then trigger sync.
        if Instant::now() - self.block_sync_client_state.last_progress_or_sync_time
            >= self.config.block_sync_trigger_timeout
        {
            self.sync(block_tree, app)?;
            self.block_sync_client_state.last_progress_or_sync_time = Instant::now();
        };

        Ok(())
    }

    /// Process an [`AdvertiseBlock`] message. This can lead to registering the sender as an available sync
    /// server and storing information on the server's claimed highest committed block height.
    fn on_receive_advertise_block<K: KVStore>(
        &mut self,
        advertise_block: AdvertiseBlock,
        origin: &VerifyingKey,
        block_tree: &BlockTreeSingleton<K>,
    ) -> Result<(), BlockSyncClientError> {
        // 1. Check if the advertise block message has the correct chain id, and is correctly signed.
        if advertise_block.chain_id != self.config.chain_id || !advertise_block.is_correct(origin) {
            return Ok(());
        }

        // 2. Check if the sender address is a valid sync server address.
        if !is_sync_server_address(origin, block_tree)? {
            return Ok(());
        }

        // 3. Check if the sender is not blacklisted.
        if self
            .block_sync_client_state
            .blacklist_contains_server_address(origin)
        {
            return Ok(());
        }

        // 4. Register the sender as an available sync server, committed to sending blocks at least up to
        // the provided block height.
        self.block_sync_client_state.register_or_update_sync_server(
            *origin,
            advertise_block.highest_committed_block_height,
        );

        Ok(())
    }

    /// Process an [`AdvertisePC`] message. This can lead to triggering sync if the criteria for event-based
    /// sync trigger are met.
    fn on_receive_advertise_pc<K: KVStore>(
        &mut self,
        advertise_pc: AdvertisePC,
        origin: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
        app: &mut impl App<K>,
    ) -> Result<(), BlockSyncClientError> {
        let highest_view_entered = block_tree.highest_view_entered()?;

        // If the sender is blacklisted, ignore the message.
        if self
            .block_sync_client_state
            .blacklist_contains_server_address(origin)
        {
            return Ok(());
        }

        // If the advertised PC has smaller view number than the highest view entered,
        // then it cannot trigger sync.
        if advertise_pc.highest_pc.view < highest_view_entered {
            return Ok(());
        }

        // If the received PC is correct *and* the difference between its view and the highest view
        // entered is sufficiently big, then trigger sync.
        let view_difference = (advertise_pc.highest_pc.view - highest_view_entered) as u64;
        if view_difference >= self.config.block_sync_trigger_min_view_difference
            && advertise_pc.highest_pc.is_correct(block_tree)?
        {
            self.sync(block_tree, app)?;
            self.block_sync_client_state.last_progress_or_sync_time = Instant::now();
        };

        Ok(())
    }

    /// Sync with a randomly selected peer.
    fn sync<K: KVStore>(
        &mut self,
        block_tree: &mut BlockTreeSingleton<K>,
        app: &mut impl App<K>,
    ) -> Result<(), BlockSyncClientError> {
        let highest_committed_block_height = block_tree.highest_committed_block_height()?;
        if let Some(peer) = self
            .block_sync_client_state
            .random_sync_server(&highest_committed_block_height)
        {
            self.sync_with(&peer, block_tree, app)
        } else {
            Ok(())
        }
    }

    /// Sync with a given peer. This involves possibly multiple iterations of:
    /// 1. Sending a sync request to the peer for a given number of blocks,
    /// 2. Waiting for a response from the peer,
    /// 3. Processing the response: validating blocks, inserting into the block tree, performing related
    ///    block tree and app state updates.
    ///
    /// As part of this process, a sync peer can be blacklisted if:
    /// 1. It sends an incorrect or unsafe block,
    /// 2. It sends a block that is not validated by the [App],
    /// 3. It fails to provide the minimum number of blocks it committed to providing through [AdvertiseBlock].
    fn sync_with<K: KVStore>(
        &mut self,
        peer: &VerifyingKey,
        block_tree: &mut BlockTreeSingleton<K>,
        app: &mut impl App<K>,
    ) -> Result<(), BlockSyncClientError> {
        Event::StartSync(StartSyncEvent {
            timestamp: SystemTime::now(),
            peer: peer.clone(),
        })
        .publish(&self.event_publisher);

        let mut blocks_synced = 0;
        let init_highest_committed_block_height =
            match block_tree.highest_committed_block_height()? {
                Some(height) => height,
                None => BlockHeight::new(0),
            };

        loop {
            let request = BlockSyncRequest {
                chain_id: self.config.chain_id,
                start_height: if let Some(height) = block_tree.highest_committed_block_height()? {
                    height + 1
                } else {
                    BlockHeight::new(0)
                },
                limit: self.config.request_limit,
            };
            self.sender.send(*peer, request);

            match self
                .receiver
                .recv_response(*peer, Instant::now() + self.config.response_timeout)
            {
                Ok(response) => {
                    let new_blocks: Vec<Block> = response
                        .blocks
                        .into_iter()
                        .skip_while(|block| block_tree.contains(&block.hash))
                        .collect();
                    if new_blocks.is_empty() {
                        // Check if the server's commitment to providing blocks at least up to a given height was observed.
                        // If not, blacklist the server.
                        let min_blocks_expected = *self
                            .block_sync_client_state
                            .available_sync_servers
                            .get(peer)
                            .unwrap()
                            - init_highest_committed_block_height;

                        if blocks_synced < min_blocks_expected {
                            self.block_sync_client_state.blacklist_sync_server(
                                peer.clone(),
                                self.config.blacklist_expiry_time,
                            )
                        }

                        Event::EndSync(EndSyncEvent {
                            timestamp: SystemTime::now(),
                            peer: *peer,
                            blocks_synced,
                        })
                        .publish(&self.event_publisher);
                        return Ok(());
                    }

                    for block in new_blocks {
                        if !block.is_correct(block_tree)?
                            || !safe_block(&block, block_tree, self.config.chain_id)?
                        {
                            // Blacklist the sync server.
                            self.block_sync_client_state.blacklist_sync_server(
                                peer.clone(),
                                self.config.blacklist_expiry_time,
                            );
                            Event::EndSync(EndSyncEvent {
                                timestamp: SystemTime::now(),
                                peer: *peer,
                                blocks_synced,
                            })
                            .publish(&self.event_publisher);
                            return Ok(());
                        }

                        let parent_block = if block.justify.is_genesis_pc() {
                            None
                        } else {
                            Some(&block.justify.block)
                        };

                        let validate_block_request =
                            ValidateBlockRequest::new(&block, block_tree.app_view(parent_block)?);

                        if let ValidateBlockResponse::Valid {
                            app_state_updates,
                            validator_set_updates,
                        } = app.validate_block_for_sync(validate_block_request)
                        {
                            // 1. Insert the block into the block tree.
                            block_tree.insert(
                                &block,
                                app_state_updates.as_ref(),
                                validator_set_updates.as_ref(),
                            )?;
                            Event::InsertBlock(InsertBlockEvent {
                                timestamp: SystemTime::now(),
                                block: block.clone(),
                            })
                            .publish(&self.event_publisher);

                            // 2. Trigger block tree updates: update highest PC, lock, commit.
                            let committed_validator_set_updates =
                                block_tree.update(&block.justify, &self.event_publisher)?;

                            if let Some(vs_updates) = committed_validator_set_updates {
                                self.validator_set_update_handle
                                    .update_validator_set(vs_updates)
                            }

                            blocks_synced += 1;
                        } else {
                            // Blacklist the sync server.
                            self.block_sync_client_state.blacklist_sync_server(
                                peer.clone(),
                                self.config.blacklist_expiry_time,
                            );
                            Event::EndSync(EndSyncEvent {
                                timestamp: SystemTime::now(),
                                peer: *peer,
                                blocks_synced,
                            })
                            .publish(&self.event_publisher);
                            return Ok(());
                        }

                        if response.highest_pc.is_correct(block_tree)?
                            && safe_pc(&response.highest_pc, block_tree, self.config.chain_id)?
                        {
                            block_tree.update(&response.highest_pc, &self.event_publisher)?;
                        }
                    }
                }
                Err(BlockSyncResponseReceiveError::Disconnected)
                | Err(BlockSyncResponseReceiveError::Timeout) => {
                    // Check if the server's commitment to providing blocks at least up to a given height was observed.
                    // If not, blacklist the server.
                    let min_blocks_expected = *self
                        .block_sync_client_state
                        .available_sync_servers
                        .get(peer)
                        .unwrap()
                        - init_highest_committed_block_height;

                    if blocks_synced < min_blocks_expected {
                        self.block_sync_client_state
                            .blacklist_sync_server(peer.clone(), self.config.blacklist_expiry_time)
                    }

                    Event::EndSync(EndSyncEvent {
                        timestamp: SystemTime::now(),
                        peer: *peer,
                        blocks_synced,
                    })
                    .publish(&self.event_publisher);
                    return Ok(());
                }
            }
        }
    }
}

/// Configuration parameters that define the behaviour of the [`BlockSyncClient`]. These should not
/// change after the block sync client starts.
pub(crate) struct BlockSyncClientConfiguration {
    /// Chain ID of the target blockchain. The block sync client will only process advertise messages whose
    /// Chain ID matches the configured value.
    pub(crate) chain_id: ChainID,

    /// The maximum number of blocks requested with every block sync request.
    pub(crate) request_limit: u32,

    /// Timeout for waiting for a single block sync response.
    pub(crate) response_timeout: Duration,

    /// Time after which a blacklisted sync server should be removed from the block sync blacklist.
    pub(crate) blacklist_expiry_time: Duration,

    /// By how many views a PC received via [`AdvertisePC`] must be ahead of the current view in order to
    /// trigger sync (via the event-based sync trigger).
    pub(crate) block_sync_trigger_min_view_difference: u64,

    /// How much time needs to pass without any progress (i.e., updating the highest PC) or sync attempts in
    /// order to trigger sync (via the timeout-based sync trigger).
    pub(crate) block_sync_trigger_timeout: Duration,
}

struct BlockSyncClientState {
    /// Replicas that are currently available to be sync servers,
    available_sync_servers: HashMap<VerifyingKey, BlockHeight>,

    /// A list of replicas (identified by their public addresses) that will be ignored by the block sync
    /// client until the paired instant. "Ignored" here means that:
    /// - These replicas will not be selected as sync servers.
    /// - Advertise messages received from these replicas will be ignored.
    blacklist: VecDeque<(VerifyingKey, Instant)>,

    /// The most recent instant in time when either:
    /// 1. Progress was made (the highest PC was updated).
    /// 2. The block sync procedure was completed (not necessarily successfully).
    last_progress_or_sync_time: Instant,

    /// Cached value of `block_tree.highest_pc().view`.
    highest_pc_view: ViewNumber,
}

impl BlockSyncClientState {
    /// Initialize the internal state of the block sync client.
    fn initialize() -> Self {
        Self {
            available_sync_servers: HashMap::new(),
            blacklist: VecDeque::new(),
            last_progress_or_sync_time: Instant::now(),
            highest_pc_view: ViewNumber::new(0),
        }
    }

    /// Check if a given server address is in the blacklist.
    fn blacklist_contains_server_address(&self, sync_server: &VerifyingKey) -> bool {
        self.blacklist
            .iter()
            .find(|(vk, _)| vk == sync_server)
            .is_some()
    }

    // Register a sync server
    fn register_or_update_sync_server(
        &mut self,
        sync_server: VerifyingKey,
        highest_committed_block_height: BlockHeight,
    ) {
        let _ = self
            .available_sync_servers
            .insert(sync_server, highest_committed_block_height);
    }

    /// Blacklist a given sync server by:
    /// 1. Removing it from available sync servers,
    /// 2. Adding it to the blacklist.
    fn blacklist_sync_server(
        &mut self,
        sync_server: VerifyingKey,
        blacklist_expiry_time: Duration,
    ) {
        // Remove any entry corresponding to this server address from the available servers list.
        let _ = self.available_sync_servers.remove(&sync_server);

        // Push a new blacklist entry to the end of the blacklist queue.
        self.blacklist
            .push_back((sync_server, Instant::now() + blacklist_expiry_time))
    }

    /// Remove all sync servers whose blacklisting has expired from the blacklist.
    fn remove_expired_blacklisted_servers(&mut self) {
        let now = Instant::now();
        while self
            .blacklist
            .front()
            .is_some_and(|(_, expiry)| expiry >= &now)
        {
            let _ = self.blacklist.pop_front();
        }
    }

    /// Select a random sync server from available sync servers, with the condition that the sync server
    /// must have advertised a block higher than `highest_committed_block_height`.
    fn random_sync_server(
        &self,
        min_highest_committed_block_height: &Option<BlockHeight>,
    ) -> Option<VerifyingKey> {
        match min_highest_committed_block_height {
            None => self
                .available_sync_servers
                .keys()
                .choose(&mut rand::thread_rng())
                .copied(),
            Some(min_height) => self
                .available_sync_servers
                .keys()
                .filter(|vk| {
                    self.available_sync_servers
                        .get(vk)
                        .is_some_and(|height| height >= min_height)
                })
                .choose(&mut rand::thread_rng())
                .copied(),
        }
    }
}

/// The block sync client may fail if there is an error when trying to read from or write to the
/// [block tree][BlockTree].
#[derive(Debug)]
pub enum BlockSyncClientError {
    BlockTreeError(BlockTreeError),
}

impl From<BlockTreeError> for BlockSyncClientError {
    fn from(value: BlockTreeError) -> Self {
        BlockSyncClientError::BlockTreeError(value)
    }
}

/// Returns whether a given [verifying key](VerifyingKey) is recognised as a valid sync server address.
///
/// A replica is allowed to act as a sync server if either:
/// 1. It is a member of the current committed validator set, or
/// 2. One of the current speculative blocks proposes to add the replica to the validator set.
///
/// Recognising only committed and candidate validators as potential sync servers is an effective,
/// though rather conservative solution to the problem of sybil attacks.
fn is_sync_server_address<K: KVStore>(
    verifying_key: &VerifyingKey,
    block_tree: &BlockTreeSingleton<K>,
) -> Result<bool, BlockSyncClientError> {
    let committed_validator_set = block_tree.committed_validator_set()?;
    if committed_validator_set.contains(verifying_key) {
        return Ok(true);
    }

    match block_tree.highest_committed_block()? {
        Some(block) => {
            let mut speculative_vs_updates = block_tree
                .blocks_in_branch(block)
                .filter(|block| {
                    block_tree
                        .validator_set_updates_status(block)
                        .is_ok_and(|vsu_status| vsu_status.is_pending())
                })
                .map(|block| {
                    if let Ok(ValidatorSetUpdatesStatus::Pending(vs_updates)) =
                        block_tree.validator_set_updates_status(&block)
                    {
                        vs_updates
                    } else {
                        ValidatorSetUpdates::new()
                    }
                });

            Ok(speculative_vs_updates
                .find(|vs_updates| vs_updates.get_insert(verifying_key).is_some())
                .is_some())
        }
        None => Ok(false),
    }
}
