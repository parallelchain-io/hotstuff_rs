use std::thread;
use crate::app::App;
use crate::config::Configuration;
use crate::node_tree::{self, NodeTreeWriter, NodeTreeSnapshotFactory};
use crate::state_machine::{State, StateMachine};

struct HotStuff {
    node_tree_snapshot_factory: NodeTreeSnapshotFactory,
    engine_thread: thread::JoinHandle<()>,
}

impl HotStuff {
    /// Starts the HotStuff Protocol State Machine and the Node Tree REST API in the background.
    pub fn start(app: impl App, configuration: Configuration) -> HotStuff {
        let (node_tree_writer, node_tree_snapshot_factory) = node_tree::open(&configuration.node_tree);

        HotStuff {
            node_tree_snapshot_factory,
            engine_thread: Self::start_state_machine_thread(app, node_tree_writer, configuration),
        }
    }

    /// Get a factory for NodeTreeSnapshots. This can be used in user-written code to get snapshotted read-access into the
    /// Node Tree. 
    pub fn get_node_tree_snapshot_factory(&self) -> &NodeTreeSnapshotFactory {
        &self.node_tree_snapshot_factory
    }

    fn start_state_machine_thread(app: impl App, mut node_tree_writer: NodeTreeWriter, configuration: Configuration) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut state_machine = StateMachine::initialize(node_tree_writer, app, configuration.state_machine, configuration.identity, configuration.networking);
            state_machine.enter(State::Sync);
        })
    }
}