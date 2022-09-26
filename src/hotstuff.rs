use std::{thread, fs};
use crate::app::App;
use crate::config::Configuration;
use crate::msg_types::{Data, DataHash, Block as MsgBlock, QuorumCertificate, BlockHeight};
use crate::stored_types::WriteSet;
use crate::block_tree::{self, BlockTreeWriter, BlockTreeSnapshotFactory};
use crate::algorithm::{State, Algorithm};
use crate::rest_api;

pub struct HotStuff {
    block_tree_snapshot_factory: BlockTreeSnapshotFactory,
    _rest_api_server: rest_api::Server,
    _engine_thread: thread::JoinHandle<()>,
}

impl HotStuff {
    /// Starts the HotStuff Protocol State Machine and the Block Tree REST API in the background. HotStuff-rs immediately 
    /// begins building the BlockTree through consensus using the provided parameters. Make sure that you have called
    /// `initialize` at least once or that you have an initialized BlockTree at `configuration.block_tree_storage.db_path`.
    pub fn start(app: impl App, configuration: Configuration) -> HotStuff {
        let (
            block_tree_writer, 
            block_tree_snapshot_factory
        ) = block_tree::open(&configuration.block_tree_storage);


        HotStuff {
            block_tree_snapshot_factory: block_tree_snapshot_factory.clone(),
            _rest_api_server: rest_api::Server::start(block_tree_snapshot_factory.clone(), configuration.block_tree_api.clone()),
            _engine_thread: Self::start_state_machine_thread(app, block_tree_writer, configuration),
        }
    } 

    /// *Deletes* all persistent storage maintained by HotStuff-rs, then inserts a Genesis Block with the provided parameters.
    pub fn initialize(
        configuration: Configuration,
        genesis_block_data_hash: DataHash,
        genesis_block_data: Data,
        genesis_block_write_set: WriteSet
    ) {
        const GENESIS_BLOCK_HEIGHT: BlockHeight = 0;

        // Clear BlockTree database if it exists.
        if configuration.block_tree_storage.db_path.is_dir() {
            fs::remove_dir_all(&configuration.block_tree_storage.db_path)
                .expect("Configuration error: fail to delete Block Tree DB directory.")
        }

        // Form genesis Block.
        let genesis_qc = QuorumCertificate::new_genesis_qc(configuration.identity.static_participant_set.len());
        let genesis_block = MsgBlock::new(
            configuration.algorithm.app_id,
            GENESIS_BLOCK_HEIGHT, 
            genesis_qc, 
            genesis_block_data_hash, 
            genesis_block_data
        );

        // Insert genesis Block.
        let (block_tree_writer, _) = block_tree::open(&configuration.block_tree_storage);
        block_tree_writer.initialize(&genesis_block, &genesis_block_write_set);
    }

    /// Get a factory for `BlockTreeSnapshot`s. This can be used in user-written code to get snapshotted read-access into the
    /// Block Tree. 
    pub fn get_block_tree_snapshot_factory(&self) -> &BlockTreeSnapshotFactory {
        &self.block_tree_snapshot_factory
    } 

    fn start_state_machine_thread(app: impl App, block_tree_writer: BlockTreeWriter, configuration: Configuration) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut state_machine = Algorithm::initialize(
                block_tree_writer, 
                app, 
                configuration.algorithm, 
                configuration.identity,
                configuration.networking
            );
            state_machine.enter(State::Sync);
        })
    }
}