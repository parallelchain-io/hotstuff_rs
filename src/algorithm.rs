use std::convert::identity;
use std::time::{Instant, Duration};
use std::cmp::{min, max};
use std::thread;
use crate::msg_types::{Block as MsgBlock, ViewNumber, QuorumCertificate, ConsensusMsg, QuorumCertificateAggregator, BlockHash};
use crate::app::{App, Block as AppBlock, SpeculativeStorageReader, ExecuteError};
use crate::config::{AlgorithmConfig, NetworkingConfiguration, IdentityConfig};
use crate::block_tree::BlockTreeWriter;
use crate::identity::{PublicKeyBytes, ParticipantSet};
use crate::ipc::{Handle as IPCHandle, AbstractHandle, RecvFromError};
use crate::rest_api::{SyncModeClient, AbstractSyncModeClient, GetBlocksFromTailError};

pub(crate) struct Algorithm<A: App, I: AbstractHandle, S: AbstractSyncModeClient> {
    // # Mutable state variables.
    cur_view: ViewNumber,
    // top_qc is the cryptographically correct QC with the highest ViewNumber that this Participant is aware of.
    top_qc: QuorumCertificate, 
    block_tree: BlockTreeWriter,

    // # Pacemaker
    pacemaker: Pacemaker,

    // # World State transition function.
    app: A,

    // # Networking utilities.
    ipc_handle: I,
    sync_mode_client: S,
    round_robin_idx: usize,

    // # Configuration variables.
    networking_config: NetworkingConfiguration,
    identity_config: IdentityConfig,
    algorithm_config: AlgorithmConfig,
}

pub(crate) enum State {
    Leader,
    Replica,

    /// BlockHash is the Block that we expect to Justify in the next
    /// View, when we become Leader.
    NextLeader(BlockHash),
    NewView,
    Sync,
}

impl<A: App> Algorithm<A, IPCHandle, SyncModeClient> {
    pub(crate) fn initialize(
        block_tree: BlockTreeWriter,
        app: A,
        algorithm_config: AlgorithmConfig,
        identity_config: IdentityConfig,
        networking_config: NetworkingConfiguration
    ) -> Algorithm<A, IPCHandle, SyncModeClient> {
        let (cur_view, top_qc) = match block_tree.get_top_block() {
            Some(block) => (block.justify.view_number + 1, block.justify),
            None => (1, QuorumCertificate::new_genesis_qc(identity_config.static_participant_set.len())),
        };
        let pacemaker = Pacemaker::new(algorithm_config.target_block_time, identity_config.static_participant_set.clone(), networking_config.progress_mode.expected_worst_case_net_latency);
        let ipc_handle = IPCHandle::new(identity_config.static_participant_set.clone(), identity_config.my_public_key, networking_config.clone());
        let sync_mode_client = SyncModeClient::new(networking_config.sync_mode.clone());

        Algorithm {
            top_qc,
            cur_view,
            block_tree,
            pacemaker,
            ipc_handle,
            sync_mode_client,
            round_robin_idx: 0,
            networking_config,
            identity_config,
            algorithm_config,
            app
        }
    }

}

impl<A: App, I: AbstractHandle, S: AbstractSyncModeClient> Algorithm<A, I, S> {
    pub(crate) fn start(&mut self) {
        let mut next_state = self.do_sync();       

        loop {
            next_state = match next_state {
                State::Leader => self.do_leader(),
                State::Replica => self.do_replica(),
                State::NextLeader(block_hash) => self.do_next_leader(block_hash),
                State::NewView => self.do_new_view(),
                State::Sync => self.do_sync(),
            }
        }
    }

    fn do_leader(&mut self) -> State {

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

                let (data, data_hash, writes) = self.app.propose_block(
                    parent_state, 
                    Instant::now() + self.pacemaker.execute_timeout()
                );

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
        let next_leader = self.pacemaker.leader(self.cur_view + 1);
        self.ipc_handle.send_to(&self.pacemaker.leader(self.cur_view + 1), &vote);

        // Phase 3: Wait for Replicas to send a vote for our proposal to the next Leader.

        if next_leader == self.identity_config.my_public_key.to_bytes() {
            // If we are the next Leader, transition to State::NextLeader and do the waiting there.
            return State::NextLeader(leaf_hash)

        } else {
            thread::sleep(self.pacemaker.execute_timeout());

            self.cur_view += 1;
            return State::Replica
        }
    }

    fn do_replica(&mut self) -> State {
        let cur_leader = self.pacemaker.leader(self.cur_view);

        // Phase 1: Wait for a proposal.
        let deadline = Instant::now() + self.pacemaker.wait_timeout(self.cur_view, &self.top_qc);
        let proposed_block;
        loop { 
            if Instant::now() >= deadline {
                return State::NewView 
            }

            match self.ipc_handle.recv_from(&cur_leader, deadline - Instant::now()) {
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
                            // The Leader is Byzantine.
                            return State::NewView
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
        let next_leader = self.pacemaker.leader(self.cur_view + 1);
        let vote = ConsensusMsg::new_vote(self.cur_view, proposed_block.hash, &self.identity_config.my_keypair);
        self.ipc_handle.send_to(&next_leader, &vote);

        // Phase 4: Wait for the next Leader to finish collecting votes

        if next_leader == self.identity_config.my_public_key.to_bytes() {
            // If next leader == me, transition to State::NextLeader and do the waiting there.
            return State::NextLeader(proposed_block.hash)
        } else {
            thread::sleep(self.networking_config.progress_mode.expected_worst_case_net_latency);

            self.cur_view += 1;
            State::Replica
        }
    }

    fn do_next_leader(&mut self, pending_block_hash: BlockHash) -> State {
        let deadline = Instant::now() + self.pacemaker.wait_timeout(self.cur_view, &self.top_qc);

        // Read messages from every participant until deadline is reached or until a new QC is collected.
        let mut qc_builder = QuorumCertificateAggregator::new(
            self.cur_view, pending_block_hash, self.identity_config.static_participant_set.clone()
        );
        loop {
            // Deadline is reached.
            if Instant::now() >= deadline {
                break;
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
                            // The sender is Byzantine.
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
                            // A new QC is collected.
                            self.top_qc = qc_builder.into();
                            break;
                        }
                    }
                }, 
            }
        }

        self.cur_view += 1;
        State::Leader
    }

    fn do_new_view(&mut self) -> State {
        let next_leader = self.pacemaker.leader(self.cur_view+1);
        if &next_leader == self.identity_config.my_public_key.as_bytes() {
            // Send out a NEW-VIEW message containing cur_view and our Top QC to the next leader.
            let new_view = ConsensusMsg::NewView(self.cur_view, self.top_qc.clone());
            self.ipc_handle.send_to(&next_leader, &new_view);

            self.cur_view += 1;
            return State::Leader;
        } else {
            self.cur_view += 1;
            return State::Replica;
        }
    }

    fn do_sync(&mut self) -> State {
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
                let request_result = self.sync_mode_client.get_blocks_from_tail(
                    start_height, 
                    self.networking_config.sync_mode.request_jump_size,
                    participant_ip_addr,
                );
                let extension_chain = match request_result {
                    Ok(blocks) => blocks,
                    Err(GetBlocksFromTailError::TailBlockNotFound) => Vec::new(),
                    Err(_) => break,
                };
    
                // 3. Filter the extension chain so that it includes only the blocks that we do *not* have in the local BlockTree.
                let mut extension_chain = extension_chain.iter().filter(|block| self.block_tree.get_block(&block.hash).is_some()).peekable();
                if extension_chain.peek().is_none() {
                    // If, after the Filter, the chain has length 0, this suggests that we are *not* lagging behind, after all.

                    // Update top_qc.
                    if let Some(block) = self.block_tree.get_top_block() {
                        self.top_qc = block.justify;
                        self.cur_view = max(self.top_qc.view_number + 1, self.cur_view);
                    }

                    // Transition back to Progress Mode.
                    if &self.pacemaker.leader(self.cur_view) == self.identity_config.my_public_key.as_bytes() {
                        return State::Leader
                    } else {
                        return State::Replica
                    }
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

fn safe_block(block: &MsgBlock, block_tree: &BlockTreeWriter) -> bool {
    let locked_view = block_tree.get_locked_view();
    locked_view.is_none() || block.justify.view_number >= locked_view.unwrap()
}

struct Pacemaker {
    tbt: Duration,
    participant_set: ParticipantSet,
    net_latency: Duration,
}

impl Pacemaker {
    fn new(tbt: Duration, participant_set: ParticipantSet, net_latency: Duration) -> Pacemaker {
        Pacemaker { tbt, participant_set, net_latency }
    }

    fn leader(&self, cur_view: ViewNumber) -> PublicKeyBytes {
        let idx = cur_view as usize % self.participant_set.len();
        self.participant_set.keys().nth(idx).unwrap().clone()
    }

    fn execute_timeout(&self) -> Duration {
        (self.tbt - 2*self.net_latency)/2
    }

    fn wait_timeout(&self, cur_view: ViewNumber, top_qc: &QuorumCertificate) -> Duration {
        let exp = min(u32::MAX as u64, cur_view - top_qc.view_number) as u32;
        self.tbt + Duration::new(u64::checked_pow(2, exp).map_or(u64::MAX, identity), 0)
    }
}
