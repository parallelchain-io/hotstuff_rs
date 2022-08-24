use std::time::{Instant, Duration};
use std::cmp;
use crate::node_tree::NodeTree;
use crate::msg_types::{ViewNumber, QuorumCertificate, self};
use crate::progress_mode::{self, view, ipc};
use crate::identity::{MY_PUBLIC_ADDR, MY_SECRET_KEY, STATIC_PARTICIPANT_SET};
use crate::App;
const TARGET_NODE_TIME: Duration = todo!();

struct StateMachine<A: App> {
    cur_view: ViewNumber,
    generic_qc: QuorumCertificate,
    node_tree: NodeTree,
    ipc_handle: ipc::Handle,
    app: A,
}

impl<A: App> StateMachine<A> {
    // Implements the Initialize state as it is described in the top-level README.
    pub fn initialize(node_tree: NodeTree, app: A) -> StateMachine<A> {
        let generic_qc = node_tree.get_generic_qc();
        let view_number = generic_qc.view_number;
        let ipc_handle = ipc::Handle::

        StateMachine {
            cur_view: view_number,
            generic_qc,
            node_tree,
            app
        }
    }

    pub fn enter(&mut self, state: State, node_tree: NodeTree) {
        let mut next_state = state;
        loop {
            next_state = match next_state {
                State::BeginView => self.do_begin_view(),
                State::Leader(deadline) => self.do_leader(deadline),
                State::Replica(deadline) => self.do_replica(deadline),
                State::NextLeader(deadline) => self.do_next_leader(deadline),
            }
        }
    }

    fn do_begin_view(&mut self) -> progress_mode::State {
        self.cur_view = cmp::max(self.cur_view, self.generic_qc.view_number + 1);
        let timeout = view::timeout(TARGET_NODE_TIME, self.cur_view, self.generic_qc);
        let deadline = Instant::now() + timeout;

        if view::leader(self.cur_view, STATIC_PARTICIPANT_SET) == MY_PUBLIC_ADDR {
            State::Leader(deadline)
        } else {
            State::Replica(deadline)
        }

    }

    fn do_leader(&mut self, deadline: Instant) -> progress_mode::State {
        let start = Instant::now();

        // Phase 1: Produce a new Node.

        // 1. Extend the branch containing Generic QC with a new leaf Node.
        let (leaf, writes) = {
            let parent_node = self.node_tree
                .get_node(&self.generic_qc.node_hash)
                .expect("Programming error: generic_qc.node_hash is not in DB.");
            let (command, state) = self.app.create_leaf(parent_node);
            let node = msg_types::Node {
                command,
                justify: self.generic_qc.clone(),
            };

            (node, state.get_writes())
        };
        
        // 2. Write new leaf into NodeTree.
        self.node_tree
            .try_insert_node(leaf, writes)
            .expect("Programming error: tried to insert App-produced Leaf node but parent is not in DB.");

        // Phase 2: Propose the new Node.

        // 1. Broadcast a PROPOSAL message containing the Node to every participant.
        self. 


    }

    fn do_replica(&mut self, deadline: Instant) -> progress_mode::State {
        todo!()
    }

    fn do_next_leader(&mut self, deadline: Instant) -> progress_mode::State {
        todo!()
    }
}

pub enum State {
    BeginView,
    Leader(Instant),
    Replica(Instant),
    NextLeader(Instant),
}
