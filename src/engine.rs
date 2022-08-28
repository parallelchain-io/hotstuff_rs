use std::time::{Instant, Duration};
use std::cmp;
use std::thread;
use crate::app;
use crate::config::{ProgressModeConfig, IPCConfig, IdentityConfig};
use crate::node_tree::{WorldState, NodeTree};
use crate::msg_types::{ViewNumber, QuorumCertificate, self, ConsensusMsg, QuorumCertificateBuilder, NodeHash};
use crate::identity::{PublicAddr, ParticipantSet};
use crate::App;
use crate::ipc::{self, RecvFromError};

pub(crate) struct StateMachine<A: App> {
    cur_view: ViewNumber,
    generic_qc: QuorumCertificate,
    node_tree: NodeTree,
    ipc_handle: ipc::Handle,
    ipc_config: IPCConfig,
    identity_config: IdentityConfig,
    progress_mode_config: ProgressModeConfig,
    app: A,
}

impl<A: App> StateMachine<A> {
    // Implements the Initialize state as it is described in the top-level README.
    pub fn initialize(
        node_tree: NodeTree,
        app: A,
        progress_mode_config: ProgressModeConfig,
        identity_config: IdentityConfig,
        ipc_config: IPCConfig
    ) -> StateMachine<A> {
        let generic_qc = node_tree.get_node_with_generic_qc().justify;
        let view_number = generic_qc.view_number;
        let ipc_handle = ipc::Handle::new(identity_config.clone(), ipc_config.clone());

        StateMachine {
            cur_view: view_number,
            generic_qc,
            node_tree,
            ipc_handle,
            ipc_config,
            identity_config,
            progress_mode_config,
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
                State::NextLeader(deadline, node_hash) => self.do_next_leader(deadline, node_hash),
                State::NewView => self.do_new_view(),
                State::Sync => self.do_sync(),
            }
        }
    }

    fn do_begin_view(&mut self) -> State {
        self.cur_view = cmp::max(self.cur_view, self.generic_qc.view_number + 1);
        let timeout = view_timeout(self.progress_mode_config.target_node_time, self.cur_view, &self.generic_qc);
        let deadline = Instant::now() + timeout;

        if view_leader(self.cur_view, &self.identity_config.static_participant_set) == self.identity_config.my_public_addr {
            State::Leader(deadline)
        } else {
            State::Replica(deadline)
        }

    }

    fn do_leader(&mut self, deadline: Instant) -> State {
        // Phase 1: Produce a new Node.

        // 1. Extend the branch containing Generic QC with a new leaf Node.
        let (leaf, writes) = {
            let parent_node =  self.node_tree
                .get_node(&self.generic_qc.node_hash)
                .expect("Programming error: generic_qc.node_hash is not in DB.");
            let app_parent_node = app::Node {
                inner: parent_node,
                node_tree: self.node_tree, 
            };
            let state = WorldState::open(&self.node_tree, &parent_node);
            let deadline = deadline - 2 * self.ipc_config.expected_worst_case_net_latency;
            let (command, state) = self.app.create_leaf(&app_parent_node, state, deadline);

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
        let vote = ConsensusMsg::Vote(self.cur_view, leaf_hash, self.identity_config.my_secret_key.sign(&leaf_hash));
        let next_leader = view_leader(self.cur_view + 1, &self.identity_config.static_participant_set);
        self.ipc_handle.send_to(&next_leader, &vote);

        // 3. If next_leader == me, change to State::NextLeader.
        if next_leader == self.identity_config.my_public_addr {
            return State::NextLeader(deadline, leaf_hash)
        }

        // Phase 3: Wait for Replicas to send vote for proposal to the next leader.
        let sleep_duration = cmp::min(2 * self.ipc_config.expected_worst_case_net_latency, deadline - Instant::now());
        thread::sleep(sleep_duration);

        // Increment view number.
        self.cur_view += 1;

        State::BeginView
    }

    fn do_replica(&mut self, deadline: Instant) -> State {
        let leader = view_leader(self.cur_view, &self.identity_config.static_participant_set);

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
                        return State::Sync
                    } else if vn > self.cur_view {
                        continue
                    } else { // (if vn == cur_view):
                        proposal = (vn, node);
                        break
                    } 
                },
                Ok(ConsensusMsg::NewView(vn, qc)) => {
                    if vn < self.cur_view - 1 {
                        continue
                    } else if qc.view_number > self.generic_qc.view_number {
                        return State::Sync
                    } else {
                        continue
                    }
                },
                Ok(ConsensusMsg::Vote(_, _, _)) => continue,
                Err(RecvFromError::Timeout) => continue,
                Err(RecvFromError::NotConnected) => continue,
            } 
        }
        let proposed_node = proposal.1;
        let proposed_node_hash = proposed_node.hash();

        // Phase 2: Validate the proposed Node.

        // 1. Execute Node. 
        let app_node = app::Node {
            inner: proposed_node,
            node_tree: self.node_tree.clone(),
        };
        let world_state = {
            let parent_node = self.node_tree.get_parent(&proposed_node_hash)
                .expect("Programming error: proposed Node accepted to Phase 2.1 of the Replica state even though its parent is not in the NodeTree.");
            WorldState::open(&self.node_tree, &parent_node)   
        };
        let deadline = deadline - self.ipc_config.expected_worst_case_net_latency; 
        let writes = match self.app.execute(&app_node, world_state, deadline) {
            Ok(writes) => writes.get_writes(),
            Err(_) => return State::NewView,
        };

        // 2. Write validated Node into NodeTree.
        self.node_tree.try_insert_node(&proposed_node, &writes)
            .expect("Programming error: proposed Node accepted to Phase 2.1 of the Replica state even though its parent is not in the NodeTree.");

        // Phase 3: Vote for the proposal.

        // 1. Send a VOTE message to the next leader.
        let next_leader = view_leader(self.cur_view + 1, &self.identity_config.static_participant_set);
        let vote = ConsensusMsg::Vote(self.cur_view, proposed_node_hash, self.identity_config.my_secret_key.sign(&proposed_node_hash));
        self.ipc_handle.send_to(&next_leader, &vote);

        // 2. If next leader == me, change to State::NextLeader.
        if next_leader == self.identity_config.my_public_addr {
            return State::NextLeader(deadline, proposed_node_hash)
        }

        // Phase 4: Wait for the next leader to finish collecting votes
        let sleep_duration = cmp::min(self.ipc_config.expected_worst_case_net_latency, deadline - Instant::now());
        thread::sleep(sleep_duration);

        // Increment view number.
        self.cur_view += 1;

        State::BeginView
    }

    fn do_next_leader(&mut self, deadline: Instant, node_hash: NodeHash) -> State {
        // 1. Read messages from every participant until deadline is reached or until a new QC is collected.
        let mut qc_builder = QuorumCertificateBuilder::new(self.cur_view, node_hash, self.identity_config.static_participant_set.clone());
        loop {
            if Instant::now() >= deadline {
                return State::NewView
            }

            match self.ipc_handle.recv_from_any(Instant::now() - deadline) {
                Ok((public_addr, ConsensusMsg::Vote(vn, node_hash, sig))) => {
                    if vn < self.cur_view {
                        continue
                    } else if self.node_tree.get_node(&node_hash).is_none() {
                        return State::Sync
                    } else if vn > self.cur_view {
                        continue
                    } else {
                        // if vote.view_number == cur_view
                        if let Ok(true) = qc_builder.insert(sig, public_addr) {
                            self.generic_qc = qc_builder.into_qc();
                            self.cur_view += 1;
                            return State::BeginView
                        }
                    }
                },
                Ok((_, ConsensusMsg::NewView(vn, qc))) => {
                    if vn < self.cur_view - 1 {
                        continue
                    } else if qc.view_number > self.generic_qc.view_number {
                        return State::Sync
                    } else {
                        continue
                    }
                },
                Ok((_, ConsensusMsg::Propose(_, _))) => continue,
                Err(RecvFromError::Timeout) => continue,
                Err(RecvFromError::NotConnected) => continue,
            }
        }
    }

    fn do_new_view(&mut self) -> State {
        // 1. Send out a NEW-VIEW message containing cur_view and our generic_qc.
        let new_view = ConsensusMsg::NewView(self.cur_view, self.generic_qc.clone());
        self.ipc_handle.broadcast(&new_view);

        self.cur_view += 1;
        State::BeginView
    }

    fn do_sync(&mut self) -> State {
        todo!()
    }
}

pub enum State {
    BeginView,
    Leader(Instant),
    Replica(Instant),
    /// NodeHash (the second item in the tuple) is the Node that we expect to Justify in the next
    /// View, when we become Leader.
    NextLeader(Instant, NodeHash),
    NewView,
    Sync,
}

pub fn view_leader(cur_view: ViewNumber, participant_set: &ParticipantSet) -> PublicAddr {
    todo!()
}

pub fn view_timeout(tnt: Duration, cur_view: ViewNumber, generic_qc: &QuorumCertificate) -> Duration {
    todo!()
}