use std::thread;
use crate::App;
use crate::config::Configuration;
use crate::msg_types::{Node, NodeHash};
use crate::NodeTree;
use crate::engine::{State, StateMachine};

struct HotStuff {
    node_tree: NodeTree,
    engine_thread: thread::JoinHandle<()>,
}

impl HotStuff {
    pub fn start(app: impl App, configuration: Configuration) -> HotStuff {
        let node_tree = NodeTree::open(configuration.node_tree.clone());
        HotStuff {
            node_tree: node_tree.clone(),
            engine_thread: Self::start_engine_thread(app, node_tree, configuration),
        }
    }

    pub fn get_node(&self, hash: &NodeHash) -> Option<Node> {
        self.node_tree.get_node(hash)
    }

    fn start_engine_thread(app: impl App, mut node_tree: NodeTree, configuration: Configuration) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let state_machine = StateMachine::initialize(node_tree, app, configuration.progress_mode, configuration.identity, configuration.ipc);
            state_machine.enter(State::Sync);
        })
    }
}