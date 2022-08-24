use std::time::{Instant, Duration};
use std::cmp;
use std::thread;
use crate::app::InvalidNodeError;
use crate::node_tree::{NodeTree, self};
use crate::msg_types::{ViewNumber, QuorumCertificate, self, ConsensusMsg};
use crate::identity::{MY_PUBLIC_ADDR, MY_SECRET_KEY, STATIC_PARTICIPANT_SET};
use crate::App;
use crate::progress_mode::{self, view, ipc};
use crate::sync_mode;

use super::ipc::handle::RecvFromError;
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
        let ipc_handle = ipc::Handle::new(STATIC_PARTICIPANT_SET);

        StateMachine {
            cur_view: view_number,
            generic_qc,
            node_tree,
            ipc_handle,
            app
        }
    }

    pub fn enter(&mut self, state: State) {
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
        let timeout = view::timeout(TARGET_NODE_TIME, self.cur_view, &self.generic_qc);
        let deadline = Instant::now() + timeout;

        if view::leader(self.cur_view, &STATIC_PARTICIPANT_SET) == MY_PUBLIC_ADDR {
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
            .try_insert_node(&leaf, &writes)
            .expect("Programming error: tried to insert App-produced Leaf node but parent is not in DB.");

        // Phase 2: Propose the new Node.

        // 1. Broadcast a PROPOSE message containing the Node to every participant.
        let leaf_hash = leaf.hash();
        let proposal = ConsensusMsg::Propose(self.cur_view, leaf);
        self.ipc_handle.broadcast(&proposal);

        // 2. Send a VOTE for our own proposal to the next leader.
        let vote = ConsensusMsg::Vote(self.cur_view, leaf_hash, MY_SECRET_KEY.sign(&leaf_hash));
        let next_leader = view::leader(self.cur_view + 1, &STATIC_PARTICIPANT_SET);
        self.ipc_handle.send_to(&next_leader, &vote);

        // 3. If next_leader == me, change to State::NextLeader.
        if next_leader == MY_PUBLIC_ADDR {
            return State::NextLeader(deadline)
        }

        // Phase 3: Wait for Replicas to send vote for proposal to the next leader.
        let sleep_duration = cmp::min(2 * ipc::NET_LATENCY, deadline - Instant::now());
        thread::sleep(sleep_duration);

        // Increment view number.
        self.cur_view += 1;

        State::BeginView
    }

    fn do_replica(&mut self, deadline: Instant) -> progress_mode::State {
        let leader = view::leader(self.cur_view, &STATIC_PARTICIPANT_SET);

        // Phase 1: Wait for a proposal.
        let proposal;
        loop { 
            if Instant::now() >= deadline {
                return State::NewView
            }

            match self.ipc_handle.recv_from(&leader, deadline - Instant::now()) {
                Ok(ConsensusMsg::Propose(vn, node)) => {
                    if vn < self.cur_view {
                        continue
                    } else if self.node_tree.get_node(&node.justify.node_hash).is_none() {
                        return sync_mode::enter(&mut self.node_tree)
                    } else if vn > self.cur_view {
                        // Note: the protocol requires that the condition `vn > self.cur_view` be considered only if the second
                        // condition is false.
                        continue
                    } else {
                        // (if vn == cur_view):
                        proposal = (vn, node);
                        break
                    } 
                },
                Ok(ConsensusMsg::NewView(vn, qc)) => {
                    if vn < self.cur_view - 1 {
                        continue
                    } else if qc.view_number > self.generic_qc.view_number {
                        return sync_mode::enter(&mut self.node_tree)
                    } else {
                        continue
                    }
                },
                Ok(ConsensusMsg::Vote(_, _, _)) => continue,
                Err(RecvFromError::Timeout) => continue,
                Err(RecvFromError::NotConnected) => continue,
            } 
        }

        // Phase 2: Validate the proposed Node.

        // 1. Execute Node.
        let node = self.node_tree.make_speculative_node(proposal.1.clone());
        let node_hash = node.hash();
        let writes = match self.app.try_execute(node) {
            Ok(writes) => writes.get_writes(),
            Err(InvalidNodeError) => return State::NewView,
        };

        // 2. Write validated Node into NodeTree.
        self.node_tree.try_insert_node(&proposal.1, &writes);

        // Phase 3: Vote for the proposal.

        // 1. Send a VOTE message to the next leader.
        let next_leader = view::leader(self.cur_view + 1, &STATIC_PARTICIPANT_SET);
        let vote = ConsensusMsg::Vote(self.cur_view, node_hash, MY_SECRET_KEY.sign(&node_hash));
        self.ipc_handle.send_to(&next_leader, &vote);

        // 2. If next leader == me, change to State::NextLeader.
        if next_leader == MY_PUBLIC_ADDR {
            return State::NextLeader(deadline)
        }

        // Phase 4: Wait for the next leader to finish collecting votes
        let sleep_duration = cmp::min(ipc::NET_LATENCY, deadline - Instant::now());
        thread::sleep(sleep_duration);

        // Increment view number.
        self.cur_view += 1;

        State::BeginView
    }

    fn do_next_leader(&mut self, deadline: Instant) -> progress_mode::State {
        // 1. Read messages from every participant until deadline is reached or until a new QC is collected.
        let qc;
        loop {
            match self.ipc_handle.recv_from_any(Instant::now() - deadline) {

            }

        }
    }

    fn do_new_view(&mut self) -> progress_mode::State {
        todo!()
    }
}

pub enum State {
    BeginView,
    Leader(Instant),
    Replica(Instant),
    NextLeader(Instant),
    NewView
}
