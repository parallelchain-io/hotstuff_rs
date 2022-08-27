use std::thread;
use crate::App;
use crate::config::Configuration;
use crate::msg_types::{Node, NodeHash};
use crate::node_tree::NodeTree;
use crate::sync_mode;
use crate::progress_mode;

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
            // 1. Sync.
            sync_mode::enter(&mut node_tree, &configuration.identity.static_participant_set);

            // 2. Initialize Progress Mode state machine.
            progress_mode::StateMachine::initialize(node_tree, app, configuration.progress_mode, configuration.identity, configuration.ipc);
        })
    }
}