use std::convert::identity;
use std::time::{Instant, Duration};
use std::cmp::{min, max};
use std::thread;
use ed25519_dalek::Signer;
use crate::msg_types::{Node as MsgNode, ViewNumber, QuorumCertificate, ConsensusMsg, QuorumCertificateBuilder, NodeHash};
use crate::app::{App, Node as AppNode, WorldStateHandle};
use crate::config::{StateMachineConfig, NetworkingConfiguration, IdentityConfig};
use crate::node_tree::NodeTreeWriter;
use crate::identity::{PublicAddr, ParticipantSet};
use crate::ipc::{Handle as IPCHandle, RecvFromError};
use crate::rest_api::SyncModeClient;

pub(crate) struct StateMachine<A: App> {
    // # Mutable state variables.
    cur_view: ViewNumber,
    top_qc: QuorumCertificate, // The cryptographically correct QC with the highest ViewNumber that this Participant is aware of.
    node_tree: NodeTreeWriter,

    // # World State transition function.
    app: A,

    // # Networking utilities.
    ipc_handle: IPCHandle,
    round_robin_idx: usize,

    // # Configuration variables.
    networking_config: NetworkingConfiguration,
    identity_config: IdentityConfig,
    state_machine_config: StateMachineConfig,
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

impl<A: App> StateMachine<A> {
    // Implements the Initialize state as it is described in the top-level README.
    pub fn initialize(
        node_tree: NodeTreeWriter,
        app: A,
        progress_mode_config: StateMachineConfig,
        identity_config: IdentityConfig,
        ipc_config: NetworkingConfiguration
    ) -> StateMachine<A> {
        let top_node = node_tree.get_top_node();
        let top_qc = top_node.justify.clone();
        let cur_view = top_node.justify.view_number;
        let ipc_handle = IPCHandle::new(identity_config.static_participant_set.clone(), identity_config.my_public_addr, ipc_config.clone());

        StateMachine {
            top_qc,
            cur_view,
            node_tree,
            ipc_handle,
            networking_config: ipc_config,
            identity_config,
            state_machine_config: progress_mode_config,
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
        self.cur_view = max(self.cur_view, self.top_qc.view_number + 1);
        let timeout = view_timeout(self.state_machine_config.target_node_time, self.cur_view, &self.top_qc);
        let deadline = Instant::now() + timeout;

        if view_leader(self.cur_view, &self.identity_config.static_participant_set) == self.identity_config.my_public_addr {
            State::Leader(deadline)
        } else {
            State::Replica(deadline)
        }
    }

    fn do_leader(&mut self, deadline: Instant) -> State {

        // Phase 1: Produce a new Node.

        // 1. Call App to produce a new leaf Node.
        let (leaf, writes) = {
            let parent_node = self.node_tree.get_node(&self.top_qc.node_hash).unwrap();
            let (command, state) = {
                let app_node = AppNode::new(parent_node.clone(), &self.node_tree);
                let world_state = WorldStateHandle::open(&self.node_tree, &parent_node.hash);
                self.app.create_leaf(&app_node, world_state, deadline)
            };
            let node = MsgNode {
                hash: MsgNode::hash(parent_node.height, &command, &self.top_qc),
                height: parent_node.height + 1,
                command,
                justify: self.top_qc.clone(),
            };

            (node, state.into())
        };
        
        // 2. Write new leaf into NodeTree.
        self.node_tree.insert_node(&leaf, &writes);

        // Phase 2: Propose the new Node.

        // 1. Broadcast a PROPOSE message containing the Node to every participant.
        let leaf_hash = leaf.hash;
        let proposal = ConsensusMsg::Propose(self.cur_view, leaf);
        self.ipc_handle.broadcast(&proposal);

        // 2. Send a VOTE for our own proposal to the next leader.
        let vote = ConsensusMsg::Vote(self.cur_view, leaf_hash, self.identity_config.my_keypair.sign(&leaf_hash));
        let next_leader = view_leader(self.cur_view + 1, &self.identity_config.static_participant_set);
        self.ipc_handle.send_to(&next_leader, &vote);

        // 3. If next_leader == me, change to State::NextLeader.
        if next_leader == self.identity_config.my_public_addr {
            return State::NextLeader(deadline, leaf_hash)
        }

        // Phase 3: Wait for Replicas to send vote for proposal to the next leader.

        let sleep_duration = min(2 * self.networking_config.progress_mode.expected_worst_case_net_latency, deadline - Instant::now());
        thread::sleep(sleep_duration);

        // Begin the next View.
        self.cur_view += 1;
        State::BeginView
    }

    fn do_replica(&mut self, deadline: Instant) -> State {
        let leader = view_leader(self.cur_view, &self.identity_config.static_participant_set);

        // Phase 1: Wait for a proposal.
        let proposed_node;
        loop { 
            if Instant::now() >= deadline {
                return State::NewView
            }

            match self.ipc_handle.recv_from(&leader, deadline - Instant::now()) {
                Err(RecvFromError::Timeout) => continue,
                Err(RecvFromError::NotConnected) => continue,
                Ok(ConsensusMsg::Vote(_, _, _)) => continue,
                Ok(ConsensusMsg::NewView(vn, qc)) => {
                    if vn < self.cur_view - 1 {
                        continue
                    } else if qc.view_number > self.top_qc.view_number {
                        return State::Sync
                    } else {
                        continue
                    }
                },
                Ok(ConsensusMsg::Propose(vn, node)) => {
                    if vn < self.cur_view {
                        continue
                    } else if self.node_tree.get_node(&node.justify.node_hash).is_none() {
                        return State::Sync
                    } else if vn > self.cur_view {
                        continue
                    } else { // (if vn == cur_view):
                        proposed_node = node;
                        self.top_qc = proposed_node.justify.clone();
                        break
                    } 
                },     
            } 
        }

        // Phase 2: Validate the proposed Node.

        // 1. Call App to execute the proposed Node.
        let app_node = AppNode::new(proposed_node.clone(), &self.node_tree);
        let world_state = WorldStateHandle::open(&self.node_tree, &proposed_node.justify.node_hash);   
        let deadline = deadline - self.networking_config.progress_mode.expected_worst_case_net_latency; 
        let writes = match self.app.execute(&app_node, world_state, deadline) {
            Ok(writes) => writes.into(),
            Err(_) => return State::NewView, // If the App rejects the Node, change to NewView.
        };

        // 2. Write validated Node into NodeTree.
        self.node_tree.insert_node(&proposed_node, &writes);

        // Phase 3: Vote for the proposal.

        // 1. Send a VOTE message to the next leader.
        let next_leader = view_leader(self.cur_view + 1, &self.identity_config.static_participant_set);
        let vote = ConsensusMsg::Vote(self.cur_view, proposed_node.hash, self.identity_config.my_keypair.sign(&proposed_node.hash));
        self.ipc_handle.send_to(&next_leader, &vote);

        // 2. If next leader == me, change to State::NextLeader.
        if next_leader == self.identity_config.my_public_addr {
            return State::NextLeader(deadline, proposed_node.hash)
        }

        // Phase 4: Wait for the next leader to finish collecting votes
        let sleep_duration = min(self.networking_config.progress_mode.expected_worst_case_net_latency, deadline - Instant::now());
        thread::sleep(sleep_duration);

        // Begin the next View.
        self.cur_view += 1;
        State::BeginView
    }

    fn do_next_leader(&mut self, deadline: Instant, pending_node_hash: NodeHash) -> State {
        // 1. Read messages from every participant until deadline is reached or until a new QC is collected.
        let mut qc_builder = QuorumCertificateBuilder::new(
            self.cur_view, pending_node_hash, self.identity_config.static_participant_set.clone()
        );
        loop {
            if Instant::now() >= deadline {
                return State::NewView
            }

            match self.ipc_handle.recv_from_any(Instant::now() - deadline) {
                Err(RecvFromError::Timeout) => continue,
                Err(RecvFromError::NotConnected) => continue,
                Ok((_, ConsensusMsg::Propose(_, _))) => continue,
                Ok((_, ConsensusMsg::NewView(vn, qc))) => {
                    if vn < self.cur_view - 1 {
                        continue
                    } else if qc.view_number > self.top_qc.view_number {
                        return State::Sync
                    } else {
                        continue
                    }
                },
                Ok((public_addr, ConsensusMsg::Vote(vn, node_hash, sig))) => {
                    if vn < self.cur_view {
                        continue
                    } else if self.node_tree.get_node(&node_hash).is_none() {
                        return State::Sync
                    } else if vn > self.cur_view {
                        continue
                    } else { // if vote.view_number == cur_view
                        if let Ok(true) = qc_builder.insert(sig, public_addr) {
                            self.top_qc = qc_builder.into_qc();
                            self.cur_view += 1;
                            return State::BeginView
                        }
                    }
                }, 
            }
        }
    }

    fn do_new_view(&mut self) -> State {
        // Send out a NEW-VIEW message containing cur_view and our Top QC to the next leader.
        let next_leader = view_leader(self.cur_view + 1, &self.identity_config.static_participant_set);
        let new_view = ConsensusMsg::NewView(self.cur_view, self.top_qc.clone());
        self.ipc_handle.send_to(&next_leader, &new_view);

        self.cur_view += 1;
        State::BeginView
    }

    fn do_sync(&mut self) -> State {
        let client = SyncModeClient::new(self.networking_config.sync_mode.request_timeout);
        loop {
            // 1. Pick an arbitrary participant in the ParticipantSet by round-robin.
            let participant_idx = { 
                self.round_robin_idx += 1; 
                if self.identity_config.static_participant_set.len() > self.round_robin_idx {
                    self.round_robin_idx = 0;
                }
                self.round_robin_idx
            };
            let participant_ip_addr = self.identity_config.static_participant_set.values().nth(participant_idx).unwrap();

            loop {
                // 2. Hit the participant’s GET /nodes endpoint for a chain of `request_jump_size` Nodes starting from our highest committed Node.
                let highest_committed_node = self.node_tree.get_highest_committed_node();
                let request_result = client.get_nodes_from_tail(
                    &highest_committed_node.hash, 
                    self.networking_config.sync_mode.request_jump_size,
                    participant_ip_addr,
                );
                let extension_chain = match request_result {
                    Ok(nodes) => nodes,
                    Err(_) => continue,
                };
    
                // 3. Filter the extension chain so that it includes only the nodes that we do *not* have in the local NodeTree.
                let extension_chain = extension_chain.iter().filter(|node| self.node_tree.get_node(&node.hash).is_some());
                if extension_chain.peekable().peek().is_none() {
                    // 3.1. If, after the Filter, the chain has length 0, change to BeginView (this suggests that we are *not* lagging behind, after all).
                    return State::BeginView
                }
    
                for node in extension_chain {
                    // 4. Validate node cryptographically. 
                    if !node.justify.is_quorum(self.identity_config.static_participant_set.len())
                        || !node.justify.is_cryptographically_correct() {
                            // Jump back to 1.: pick another participant to sync with.
                            break
                        }

                    // 5. Call App to validate node.

                }
    
    
    
    
    
            }
        }
    }
}

pub fn view_leader(cur_view: ViewNumber, participant_set: &ParticipantSet) -> PublicAddr {
    let idx = cur_view as usize % participant_set.len();
    participant_set.keys().nth(idx).unwrap().clone()
}

pub fn view_timeout(tnt: Duration, cur_view: ViewNumber, top_qc: &QuorumCertificate) -> Duration {
    let exp = min(u32::MAX as u64, cur_view - top_qc.view_number) as u32;
    tnt + Duration::new(u64::checked_pow(2, exp).map_or(u64::MAX, identity), 0)
}

pub fn safe_node(node: &MsgNode, node_tree: &NodeTreeWriter) -> bool {
    let locked_view = node_tree.get_locked_view();
    node.justify.view_number >= locked_view
}