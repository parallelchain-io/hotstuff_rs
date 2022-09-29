use std::convert::identity;
use std::time::{Instant, Duration};
use std::cmp::{min, max};
use std::thread;
use crate::msg_types::{Block as MsgBlock, ViewNumber, QuorumCertificate, ConsensusMsg, QuorumCertificateAggregator, BlockHash, Signature};
use crate::app::{App, Block as AppBlock, SpeculativeStorageReader, ExecuteError};
use crate::config::{AlgorithmConfig, NetworkingConfiguration, IdentityConfig};
use crate::block_tree::BlockTreeWriter;
use crate::identity::{PublicKeyBytes, ParticipantSet};
use crate::ipc::{Handle as IPCHandle, RecvFromError};
use crate::rest_api::SyncModeClient;

pub(crate) struct Algorithm<A: App> {
    // # Mutable state variables.
    cur_view: ViewNumber,
    // top_qc is the cryptographically correct QC with the highest ViewNumber that this Participant is aware of.
    top_qc: QuorumCertificate, 
    block_tree: BlockTreeWriter,

    // # World State transition function.
    app: A,

    // # Networking utilities.
    ipc_handle: IPCHandle,
    round_robin_idx: usize,

    // # Configuration variables.
    networking_config: NetworkingConfiguration,
    identity_config: IdentityConfig,
    algorithm_config: AlgorithmConfig,
}

pub(crate) enum State {
    BeginView,
    Leader(Instant),
    Replica(Instant),

    /// BlockHash (the second item in the tuple) is the Block that we expect to Justify in the next
    /// View, when we become Leader.
    NextLeader(Instant, BlockHash),
    NewView,
    Sync,
}

impl<A: App> Algorithm<A> {
    pub(crate) fn initialize(
        block_tree: BlockTreeWriter,
        app: A,
        progress_mode_config: AlgorithmConfig,
        identity_config: IdentityConfig,
        ipc_config: NetworkingConfiguration
    ) -> Algorithm<A> {
        let (cur_view, top_qc) = match block_tree.get_top_block() {
            Some(block) => (block.justify.view_number + 1, block.justify),
            None => (0, QuorumCertificate::new_genesis_qc(identity_config.static_participant_set.len())),
        };
        let ipc_handle = IPCHandle::new(identity_config.static_participant_set.clone(), identity_config.my_public_key, ipc_config.clone());

        Algorithm {
            top_qc,
            cur_view,
            block_tree,
            ipc_handle,
            round_robin_idx: 0,
            networking_config: ipc_config,
            identity_config,
            algorithm_config: progress_mode_config,
            app
        }
    }

    pub(crate) fn enter(&mut self, state: State) {
        let mut next_state = state;
        loop {
            next_state = match next_state {
                State::BeginView => self.do_begin_view(),
                State::Leader(deadline) => self.do_leader(deadline),
                State::Replica(deadline) => self.do_replica(deadline),
                State::NextLeader(deadline, block_hash) => self.do_next_leader(deadline, block_hash),
                State::NewView => self.do_new_view(),
                State::Sync => self.do_sync(),
            }
        }
    }

    fn do_begin_view(&mut self) -> State {
        self.cur_view = max(self.cur_view, self.top_qc.view_number + 1);
        let timeout = view_timeout(self.algorithm_config.target_block_time, self.cur_view, &self.top_qc);
        let deadline = Instant::now() + timeout;

        if view_leader(self.cur_view, &self.identity_config.static_participant_set) == self.identity_config.my_public_key.to_bytes() {
            State::Leader(deadline)
        } else {
            State::Replica(deadline)
        }
    }

    fn do_leader(&mut self, deadline: Instant) -> State {

        // Phase 1: Produce a new Block.

        // 1. Call App to produce a new leaf Block.
        let (new_leaf, writes) = {
            let parent_state = if self.top_qc.block_hash == MsgBlock::PARENT_OF_GENESIS_BLOCK_HASH {
                // parent_block and storage are None if the BlockTree does not have a genesis Block.
                None
            } else {
                let msg_block = self.block_tree.get_block(&self.top_qc.block_hash).unwrap();
                let storage = SpeculativeStorageReader::open(self.block_tree.clone(), &msg_block.hash);
                let app_block = AppBlock::new(msg_block, self.block_tree.clone());
                Some((app_block, storage))
            };
            let (block_height, data, data_hash, writes) = {
                let block_height = match &parent_state {
                    Some(parent_state) => parent_state.0.height + 1,
                    None => MsgBlock::GENESIS_BLOCK_HEIGHT
                };
                let (data, data_hash, writes) = self.app.propose_block(parent_state, deadline);
                

                (block_height, data, data_hash, writes)
            };

            let block = MsgBlock::new(self.algorithm_config.app_id, block_height, self.top_qc.clone(), data_hash, data);

            (block, writes)
        };
        
        // 2. Write new leaf into BlockTree.
        self.block_tree.insert_block(&new_leaf, &writes);

        // Phase 2: Propose the new Block.

        // 1. Broadcast a PROPOSE message containing the Block to every participant.
        let leaf_hash = new_leaf.hash;
        let proposal = ConsensusMsg::Propose(self.cur_view, new_leaf);
        self.ipc_handle.broadcast(&proposal);

        // 2. Send a VOTE for our own proposal to the next leader.
        let vote = ConsensusMsg::new_vote(self.cur_view, leaf_hash,&self.identity_config.my_keypair);
        let next_leader = view_leader(self.cur_view + 1, &self.identity_config.static_participant_set);
        self.ipc_handle.send_to(&next_leader, &vote);

        // 3. If next_leader == me, change to State::NextLeader.
        if next_leader == self.identity_config.my_public_key.to_bytes() {
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
        let proposed_block;
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
                        if !qc.is_valid(&self.identity_config.static_participant_set) {
                            // TODO: Slash
                            return State::Sync
                        } else {
                            return State::Sync
                        }
                    } else {
                        continue
                    }
                },
                Ok(ConsensusMsg::Propose(vn, block)) => {
                    if vn < self.cur_view {
                        continue
                    } else if self.block_tree.get_block(&block.justify.block_hash).is_none() {
                        return State::Sync
                    } else if vn > self.cur_view {
                        continue
                    } else { // (if vn == cur_view);
                        // 1. Validate the proposed Block's QuorumCertificate
                        if !block.justify.is_valid(&self.identity_config.static_participant_set) {
                            // TODO: Slash
                            return State::NewView // if the Block's QC is invalid, change to NewView.
                        }

                        // 2. Check if the proposed Block satisfies the SafeBlock predicate.
                        if !safe_block(&block, &self.block_tree) {
                            continue
                        }

                        proposed_block = block;
                        self.top_qc = proposed_block.justify.clone();
                        break
                    } 
                },     
            } 
        }

        // Phase 2: Validate the proposed Block. 

        // 1. Call App to execute the proposed Block.
        let app_block = AppBlock::new(proposed_block.clone(), self.block_tree.clone());
        let storage = if app_block.justify.block_hash == MsgBlock::PARENT_OF_GENESIS_BLOCK_HASH {
            None
        } else {
            Some(SpeculativeStorageReader::open(self.block_tree.clone(), &proposed_block.justify.block_hash))
        };
        let deadline = deadline - self.networking_config.progress_mode.expected_worst_case_net_latency; 
 
        // 2. If App accepts the Block, write it and its writes into the BlockTree.
        let writes = match self.app.validate_block(app_block, storage, deadline) {
            Ok(writes) => writes.into(),
            // If the App rejects the Block, change to NewView.
            Err(ExecuteError::RanOutOfTime) => return State::NewView, 
            Err(ExecuteError::InvalidBlock) => return State::NewView,
        };
        self.block_tree.insert_block(&proposed_block, &writes);

        // Phase 3: Vote for the proposal.

        // 1. Send a VOTE message to the next leader.
        let next_leader = view_leader(self.cur_view + 1, &self.identity_config.static_participant_set);
        let vote = ConsensusMsg::new_vote(self.cur_view, proposed_block.hash, &self.identity_config.my_keypair);
        self.ipc_handle.send_to(&next_leader, &vote);

        // 2. If next leader == me, change to State::NextLeader.
        if next_leader == self.identity_config.my_public_key.to_bytes() {
            return State::NextLeader(deadline, proposed_block.hash)
        }

        // Phase 4: Wait for the next leader to finish collecting votes
        let sleep_duration = min(self.networking_config.progress_mode.expected_worst_case_net_latency, deadline - Instant::now());
        thread::sleep(sleep_duration);

        // Begin the next View.
        self.cur_view += 1;
        State::BeginView
    }

    fn do_next_leader(&mut self, deadline: Instant, pending_block_hash: BlockHash) -> State {
        // 1. Read messages from every participant until deadline is reached or until a new QC is collected.
        let mut qc_builder = QuorumCertificateAggregator::new(
            self.cur_view, pending_block_hash, self.identity_config.static_participant_set.clone()
        );
        loop {
            if Instant::now() >= deadline {
                return State::NewView
            }

            match self.ipc_handle.recv_from_any(deadline - Instant::now()) {
                Err(RecvFromError::Timeout) => continue,
                Err(RecvFromError::NotConnected) => continue,
                Ok((_, ConsensusMsg::Propose(_, _))) => continue,
                Ok((_, ConsensusMsg::NewView(vn, qc))) => {
                    if vn < self.cur_view - 1 {
                        continue
                    } else if qc.view_number > self.top_qc.view_number {
                        if !qc.is_valid(&self.identity_config.static_participant_set) {
                            // TODO: Slash
                            return State::Sync
                        } else {
                            return State::Sync
                        }
                    } else {
                        continue
                    }
                },
                Ok((public_addr, ConsensusMsg::Vote(vn, block_hash, sig))) => {
                    if vn < self.cur_view {
                        continue
                    } else if self.block_tree.get_block(&block_hash).is_none() {
                        return State::Sync
                    } else if vn > self.cur_view {
                        continue
                    } else { // if vote.view_number == cur_view
                        if let Ok(true) = qc_builder.insert(sig, public_addr) {
                            self.top_qc = qc_builder.into();
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
        let client = SyncModeClient::new(self.networking_config.sync_mode.clone());
        loop {
            // 1. Pick an arbitrary participant in the ParticipantSet by round-robin.
            let participant_idx = { 
                self.round_robin_idx += 1; 
                if self.round_robin_idx >= self.identity_config.static_participant_set.len() {
                    self.round_robin_idx = 0;
                }
                self.round_robin_idx
            };
            let participant_ip_addr = self.identity_config.static_participant_set.values().nth(participant_idx).unwrap();

            loop {
                // 2. Hit the participant’s GET /blocks endpoint for a chain of `request_jump_size` Blocks starting from our highest committed Block,
                // or the Genesis Block, if the former is still None.
                let start_height = match self.block_tree.get_highest_committed_block() {
                    Some(block) => block.height,
                    None => 0,
                };
                let request_result = client.get_blocks_from_tail(
                    start_height, 
                    self.networking_config.sync_mode.request_jump_size,
                    participant_ip_addr,
                );
                let extension_chain = match request_result {
                    Ok(blocks) => blocks,
                    Err(crate::rest_api::GetBlocksFromTailError::TailBlockNotFound) => Vec::new(),
                    Err(_) => continue,
                };
    
                // 3. Filter the extension chain so that it includes only the blocks that we do *not* have in the local BlockTree.
                let mut extension_chain = extension_chain.iter().filter(|block| self.block_tree.get_block(&block.hash).is_some()).peekable();
                if extension_chain.peek().is_none() {
                    // If, after the Filter, the chain has length 0, this suggests that we are *not* lagging behind, after all.

                    // Update top_qc.
                    if let Some(block) = self.block_tree.get_top_block() {
                        self.top_qc = block.justify;
                    }

                    return State::BeginView
                }
    
                for block in extension_chain {
                    // 4. Structurally validate Block. 
                    if !block.justify.is_valid(&self.identity_config.static_participant_set) {
                        break
                    }

                    // 5. Call App to validate block.
                    let app_block = AppBlock::new(block.clone(), self.block_tree.clone());
                    let storage = if block.justify.block_hash == MsgBlock::PARENT_OF_GENESIS_BLOCK_HASH {
                        None
                    } else {
                        Some(SpeculativeStorageReader::open(self.block_tree.clone(), &block.justify.block_hash))
                    };
                    let deadline = Instant::now() + self.algorithm_config.sync_mode_execution_timeout;
                    let execution_result = self.app.validate_block(app_block, storage, deadline);

                    // 6. If App accepts the Block, write it and its WriteSet into the BlockTree.
                    let writes = match execution_result {
                        Ok(writes) => writes.into(),
                        Err(ExecuteError::RanOutOfTime) => panic!("Configuration Error: ran out of time executing a Block during Sync"), 
                        Err(ExecuteError::InvalidBlock) => panic!("Possible Byzantine Fault: a quorum voted on an App-invalid Block."),
                    };
                    self.block_tree.insert_block(&block, &writes);
                }
            }
        } 
    }
}

fn view_leader(cur_view: ViewNumber, participant_set: &ParticipantSet) -> PublicKeyBytes {
    let idx = cur_view as usize % participant_set.len();
    participant_set.keys().nth(idx).unwrap().clone()
}

fn view_timeout(tnt: Duration, cur_view: ViewNumber, top_qc: &QuorumCertificate) -> Duration {
    let exp = min(u32::MAX as u64, cur_view - top_qc.view_number) as u32;
    tnt + Duration::new(u64::checked_pow(2, exp).map_or(u64::MAX, identity), 0)
}

fn safe_block(block: &MsgBlock, block_tree: &BlockTreeWriter) -> bool {
    let locked_view = block_tree.get_locked_view();
    locked_view.is_none() || block.justify.view_number >= locked_view.unwrap()
}