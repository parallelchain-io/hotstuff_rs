use std::{thread, fs};
use crate::app::App;
use crate::config::Configuration;
use crate::algorithm::{State, Algorithm};
use crate::block_tree::{self, BlockTreeWriter, BlockTreeSnapshotFactory};
use crate::rest_api;

pub struct HotStuff {
    block_tree_snapshot_factory: BlockTreeSnapshotFactory,
    _rest_api_server: rest_api::Server,
    _engine_thread: thread::JoinHandle<()>,
}

impl HotStuff {
    /// Starts the HotStuff Protocol State Machine and the Block Tree REST API in the background. HotStuff-rs immediately 
    /// begins building the BlockTree through consensus using the provided parameters. If start_from_scratch is provided, then
    /// this function clears the BlockTree before proceeding.
    pub fn start(app: impl App, configuration: Configuration, start_from_scratch: bool) -> HotStuff {
        if start_from_scratch {
            // Clear BlockTree database if it exists.
            if configuration.block_tree_storage.db_path.is_dir() {
                fs::remove_dir_all(&configuration.block_tree_storage.db_path)
                    .expect("Configuration error: fail to delete Block Tree DB directory.")
            }
        }

        let (
            block_tree_writer, 
            block_tree_snapshot_factory
        ) = block_tree::open(&configuration.block_tree_storage);

        HotStuff {
            block_tree_snapshot_factory: block_tree_snapshot_factory.clone(),
            _rest_api_server: rest_api::Server::start(block_tree_snapshot_factory.clone(), configuration.networking.sync_mode.clone()),
            _engine_thread: Self::start_state_machine_thread(app, block_tree_writer, configuration),
        }
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