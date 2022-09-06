use std::thread;
use crate::app::App;
use crate::config::Configuration;
use crate::block_tree::{self, BlockTreeWriter, BlockTreeSnapshotFactory};
use crate::state_machine::{State, StateMachine};

struct HotStuff {
    block_tree_snapshot_factory: BlockTreeSnapshotFactory,
    engine_thread: thread::JoinHandle<()>,
}

impl HotStuff {
    /// Starts the HotStuff Protocol State Machine and the Block Tree REST API in the background.
    pub fn start(app: impl App, configuration: Configuration) -> HotStuff {
        let (block_tree_writer, block_tree_snapshot_factory) = block_tree::open(&configuration.block_tree);

        HotStuff {
            block_tree_snapshot_factory,
            engine_thread: Self::start_state_machine_thread(app, block_tree_writer, configuration),
        }
    }

    /// Get a factory for BlockTreeSnapshots. This can be used in user-written code to get snapshotted read-access into the
    /// Block Tree. 
    pub fn get_block_tree_snapshot_factory(&self) -> &BlockTreeSnapshotFactory {
        &self.block_tree_snapshot_factory
    }

    fn start_state_machine_thread(app: impl App, mut block_tree_writer: BlockTreeWriter, configuration: Configuration) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut state_machine = StateMachine::initialize(block_tree_writer, app, configuration.state_machine, configuration.identity, configuration.networking);
            state_machine.enter(State::Sync);
        })
    }
}