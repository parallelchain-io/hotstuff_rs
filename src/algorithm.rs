/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Thread that drives the event-driven implementations of the [HotStuff](crate::hotstuff),
//! [Pacemaker](crate::pacemaker), and [BlockSync](crate::block_sync) subprotocols.

use std::{
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
};

use ed25519_dalek::VerifyingKey;

use crate::{
    app::App,
    block_sync::{
        client::{BlockSyncClient, BlockSyncClientConfiguration},
        messages::BlockSyncResponse,
    },
    block_tree::{accessors::internal::BlockTreeSingleton, pluggables::KVStore},
    events::*,
    hotstuff::implementation::{HotStuff, HotStuffConfiguration},
    networking::{
        messages::ProgressMessage,
        network::{Network, ValidatorSetUpdateHandle},
        receiving::{BlockSyncClientStub, ProgressMessageReceiveError, ProgressMessageStub},
        sending::SenderHandle,
    },
    pacemaker::implementation::{Pacemaker, PacemakerConfiguration},
    types::data_types::{BufferSize, ChainID, ViewNumber},
};

/// Instance of the algorithm thread.
///
/// This struct's `Drop` destructor gracefully shuts down the algorithm thread.
pub(crate) struct Algorithm<N: Network + 'static, K: KVStore, A: App<K> + 'static> {
    chain_id: ChainID,
    pm_stub: ProgressMessageStub,
    block_tree: BlockTreeSingleton<K>,
    app: A,
    hotstuff: HotStuff<N>,
    pacemaker: Pacemaker<N>,
    block_sync_client: BlockSyncClient<N>,
    shutdown_signal: Receiver<()>,
}

impl<N: Network + 'static, K: KVStore, A: App<K> + 'static> Algorithm<N, K, A> {
    /// Create an instance of the algorithm thread.
    pub(crate) fn new(
        chain_id: ChainID,
        hotstuff_config: HotStuffConfiguration,
        pacemaker_config: PacemakerConfiguration,
        block_sync_client_config: BlockSyncClientConfiguration,
        block_tree: BlockTreeSingleton<K>,
        app: A,
        network: N,
        progress_msg_receiver: Receiver<(VerifyingKey, ProgressMessage)>,
        progress_msg_buffer_capacity: BufferSize,
        block_sync_response_receiver: Receiver<(VerifyingKey, BlockSyncResponse)>,
        shutdown_signal: Receiver<()>,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        let pm_stub = ProgressMessageStub::new(progress_msg_receiver, progress_msg_buffer_capacity);
        let block_sync_client_stub = BlockSyncClientStub::new(block_sync_response_receiver);
        let msg_sender: SenderHandle<N> = SenderHandle::new(network.clone());
        let validator_set_update_handle = ValidatorSetUpdateHandle::new(network);

        let init_view = match block_tree
            .highest_view_with_progress()
            .expect("Cannot retrieve the highest view with progress!")
            .int()
        {
            0 => ViewNumber::new(0),
            v => ViewNumber::new(v + 1),
        };

        let pacemaker = Pacemaker::new(
            pacemaker_config,
            msg_sender.clone(),
            init_view,
            &block_tree
                .validator_set_state()
                .expect("Cannot retrieve the validator set state!"),
            event_publisher.clone(),
        )
        .expect("Failed to create a new Pacemaker!");

        let init_view_info = pacemaker.query();

        let hotstuff = HotStuff::new(
            hotstuff_config,
            init_view_info.clone(),
            msg_sender.clone(),
            validator_set_update_handle.clone(),
            block_tree
                .validator_set_state()
                .expect("Cannot retrieve the validator set state!")
                .clone(),
            event_publisher.clone(),
        );

        let block_sync_client = BlockSyncClient::new(
            block_sync_client_config,
            block_sync_client_stub,
            msg_sender,
            validator_set_update_handle,
            event_publisher.clone(),
        );

        Self {
            chain_id,
            pm_stub,
            block_tree,
            app,
            hotstuff,
            pacemaker,
            block_sync_client,
            shutdown_signal,
        }
    }

    /// Start an instance of the algorithm thread.
    pub(crate) fn start(self) -> JoinHandle<()> {
        thread::spawn(move || self.execute())
    }

    fn execute(mut self) {
        loop {
            // 1. Check whether the library user has issued a shutdown command. If so, break.
            match self.shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => {
                    panic!("Algorithm thread disconnected from main thread")
                }
            }

            // 2. Let the pacemaker update its internal state if needed.
            self.pacemaker
                .tick(&self.block_tree)
                .expect("Pacemaker failure!");

            // 3. Query the pacemaker for potential updates to the current view.
            let view_info = self.pacemaker.query();

            // 4. In case the view has been updated, update HotStuff's internal view and perform
            // the necessary protocol steps.
            if self.hotstuff.is_view_outdated(view_info) {
                self.hotstuff
                    .enter_view(view_info.clone(), &mut self.block_tree, &mut self.app)
                    .expect("HotStuff failure!")
            }

            // 5. Poll the network for incoming messages.
            match self
                .pm_stub
                .recv(self.chain_id, view_info.view, view_info.deadline)
            {
                Ok((origin, msg)) => match msg {
                    ProgressMessage::HotStuffMessage(msg) => self
                        .hotstuff
                        .on_receive_msg(msg, &origin, &mut self.block_tree, &mut self.app)
                        .expect("HotStuff failure!"),
                    ProgressMessage::PacemakerMessage(msg) => self
                        .pacemaker
                        .on_receive_msg(msg, &origin, &mut self.block_tree)
                        .expect("Pacemaker failure!"),
                    ProgressMessage::BlockSyncAdvertiseMessage(msg) => self
                        .block_sync_client
                        .on_receive_msg(msg, &origin, &mut self.block_tree, &mut self.app)
                        .expect("Block Sync Client failure!"),
                },
                Err(ProgressMessageReceiveError::Disconnected) => {
                    panic!("The poller has disconnected!")
                }
                Err(ProgressMessageReceiveError::Timeout) => {}
            }

            // 6. Let the block sync client update its internal state, and trigger sync if needed.
            self.block_sync_client
                .tick(&mut self.block_tree, &mut self.app)
                .expect("Block Sync Client failure!")
        }
    }
}
