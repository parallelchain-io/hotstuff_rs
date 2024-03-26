/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of the hotstuff-rs SMR protocol, which invokes the following sub-protocols:
//! 1. [HotStuff]: for blockchain consensus on a per-view basis,
//! 2. [Pacemaker]: for synchronizing views among the peers,
//! 3. [BlockSyncClient]: for triggering and handling the block sync procedure when needed.

use std::cmp::max;
use std::sync::mpsc::{Sender, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};

use ed25519_dalek::VerifyingKey;

use crate::app::App;
use crate::block_sync::client::{BlockSyncClient, BlockSyncClientConfiguration};
use crate::block_sync::messages::BlockSyncResponse;
use crate::messages::ProgressMessage;
use crate::events::*;
use crate::hotstuff::protocol::{HotStuff, HotStuffConfiguration};
use crate::networking::*;
use crate::pacemaker::protocol::{Pacemaker, PacemakerConfiguration, ViewInfo};
use crate::state::*;
use crate::types::basic::{BufferSize, ChainID, ViewNumber};

pub(crate) struct Algorithm<N: Network + 'static, K: KVStore, A: App<K> + 'static> {
    chain_id: ChainID,
    pm_stub: ProgressMessageStub<N>,
    block_tree: BlockTree<K>,
    app: A,
    hotstuff: HotStuff<N>,
    pacemaker: Pacemaker<N>,
    block_sync_client: BlockSyncClient<N>,
    shutdown_signal: Receiver<()>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network + 'static, K: KVStore, A: App<K> + 'static> Algorithm<N, K, A> {

    pub(crate) fn new(
        chain_id: ChainID,
        hotstuff_config: HotStuffConfiguration,
        pacemaker_config: PacemakerConfiguration,
        block_sync_client_config: BlockSyncClientConfiguration,
        block_tree: BlockTree<K>,
        app: A,
        network: N,
        progress_msg_receiver: Receiver<(VerifyingKey, ProgressMessage)>,
        block_sync_response_receiver: Receiver<(VerifyingKey, BlockSyncResponse)>,
        shutdown_signal: Receiver<()>,
        event_publisher: Option<Sender<Event>>,
        progress_msg_buffer_capacity: BufferSize,
    ) -> Self {

        let pm_stub = ProgressMessageStub::new(network.clone(), progress_msg_receiver, progress_msg_buffer_capacity);
        let block_sync_client_stub = BlockSyncClientStub::new(network.clone(), block_sync_response_receiver);
        let msg_sender: SenderHandle<N> = SenderHandle::new(network.clone());
        let validator_set_update_handle = ValidatorSetUpdateHandle::new(network.clone());

        let init_view = 
            match block_tree.highest_view_with_progress().get_int() {
                0 => ViewNumber::new(0),
                v => ViewNumber::new(v+1)
            };

        let pacemaker = Pacemaker::new(
            pacemaker_config,
            msg_sender.clone(),
            init_view,
            &block_tree.committed_validator_set(),
            event_publisher.clone()
        ).expect("Failed to create a new Pacemaker!");

        let init_view_info = pacemaker.view_info();

        let hotstuff = HotStuff::new(
            hotstuff_config,
            init_view_info.clone(),
            msg_sender.clone(),
            validator_set_update_handle.clone(),
            block_tree.committed_validator_set().clone(),
            event_publisher.clone()
        );

        let block_sync_client = BlockSyncClient::new(
            block_sync_client_config,
            block_sync_client_stub,
            msg_sender, 
            validator_set_update_handle,
            event_publisher.clone()
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
            event_publisher
        }
    }

    pub(crate) fn start(self) -> JoinHandle<()> {

        thread::spawn(move || {

            self.execute()

        })
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
            self.pacemaker.tick(&self.block_tree).expect("Pacemaker failure!");

            // 3. Query the pacemaker for potential updates to the current view.
            let view_info = self.pacemaker.view_info();

            // 4. In case the view has been updated, update HotStuff's internal view and perform
            // the necessary protocol steps.
            self.hotstuff.on_receive_view_info(view_info.clone(), &mut self.block_tree, &mut self.app);

            // 5. Poll the network for incoming messages.
            //let deadline = self.pacemaker.view_deadline(self.cur_view).expect("The deadline for this view has not been set!");
            match self.pm_stub.recv(self.chain_id, view_info.view, view_info.deadline) {
                Ok((origin, msg)) => {
                    match msg {
                        ProgressMessage::HotStuffMessage(msg) => self.hotstuff.on_receive_msg(msg, &origin, &mut self.block_tree, &mut self.app),
                        ProgressMessage::PacemakerMessage(msg) => self.pacemaker.on_receive_msg(msg, &origin, &mut self.block_tree).expect("Pacemaker failure!"),
                        ProgressMessage::BlockSyncTriggerMessage(msg) => self.block_sync_client.on_receive_msg(msg, &origin, &self.block_tree),
                    }
                },
                Err(_) => {},
            }

        }
    }
}