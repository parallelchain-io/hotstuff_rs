use std::convert::identity;
use std::time::{Instant, Duration};
use std::cmp::{min, max};
use std::thread;
use hotstuff_rs_types::messages::{Block as MsgBlock, ViewNumber, QuorumCertificate, ConsensusMsg, QuorumCertificateAggregator, BlockHash};
use hotstuff_rs_types::identity::{PublicKeyBytes, ParticipantSet};
use crate::app::{App, Block as AppBlock, SpeculativeStorageReader, ExecuteError};
use crate::config::{AlgorithmConfig, NetworkingConfiguration, IdentityConfig};
use crate::block_tree::BlockTreeWriter;
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
            None => (1, QuorumCertificate::genesis_qc(identity_config.static_participant_set.len())),
        };
        let pacemaker = Pacemaker::new(algorithm_config.target_block_time, identity_config.static_participant_set.clone(), networking_config.progress_mode.expected_worst_case_net_latency);
        let ipc_handle = IPCHandle::new(identity_config.static_participant_set.clone(), identity_config.my_public_key, networking_config.clone());
        let sync_mode_client = SyncModeClient::new(networking_config.sync_mode.clone(), identity_config.static_participant_set.clone());

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
                            // The Leader is Byzantine.
                            return State::Sync
                        } else {
                            return State::Sync
                        }
                    } else {
                        continue
                    }
                },
                Ok(ConsensusMsg::Propose(vn, block)) => {
                    // The proposal comes from the past.
                    if vn < self.cur_view {
                        continue
                    // The proposal refers to a branch that our BlockTree does not contain.
                    } else if self.block_tree.get_block(&block.justify.block_hash).is_none() 
                        && block.justify.block_hash != MsgBlock::PARENT_OF_GENESIS_BLOCK_HASH {
                        return State::Sync
                    // The proposal comes from the future but refers to a branch that we know.
                    } else if vn > self.cur_view {
                        continue
                    // The proposal is for the present and refers to a branch that we know.
                    } else {
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
 
        // 2. If App accepts the Block, write it and its writes into the BlockTree.
        let writes = match self.app.validate_block(app_block, storage, Instant::now() + self.pacemaker.execute_timeout()) {
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
        let _ = self.ipc_handle.send_to(&next_leader, &vote);

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
                        // CRITICAL TODO [Alice]: check if signature is cryptographically correct.

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
            self.cur_view += 1;
            return State::Leader;
        } else {
            // Send out a NEW-VIEW message containing cur_view and our Top QC to the next leader.
            let new_view = ConsensusMsg::NewView(self.cur_view, self.top_qc.clone());
            let _ = self.ipc_handle.send_to(&next_leader, &new_view);

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
            let participant = self.identity_config.static_participant_set.keys().nth(participant_idx).unwrap();

            loop {
                // 2. Hit the participant’s GET /blocks endpoint for a chain of `request_jump_size` Blocks starting from our highest committed Block,
                // or the Genesis Block, if the former is still None.
                let start_height = match self.block_tree.get_highest_committed_block() {
                    Some(block) => block.height,
                    None => 0,
                };
                let request_result = self.sync_mode_client.get_blocks_from_tail(
                    *participant, 
                    start_height, 
                    self.networking_config.sync_mode.request_jump_size,
                );
                let extension_chain = match request_result {
                    Ok(blocks) => blocks,
                    Err(GetBlocksFromTailError::TailBlockNotFound) => Vec::new(),
                    Err(_) => break,
                };
    
                // 3. Filter the extension chain so that it includes only the blocks that we do *not* have in the local BlockTree.
                let mut extension_chain = extension_chain.iter().filter(|block| 
                    self.block_tree.get_block(&block.hash).is_none()
                ).peekable();
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

pub(crate) struct Pacemaker {
    tbt: Duration,
    participant_set: ParticipantSet,
    net_latency: Duration,
}

impl Pacemaker {
    pub(crate) fn new(tbt: Duration, participant_set: ParticipantSet, net_latency: Duration) -> Pacemaker {
        Pacemaker { tbt, participant_set, net_latency }
    }

    pub(crate) fn leader(&self, cur_view: ViewNumber) -> PublicKeyBytes {
        let idx = cur_view as usize % self.participant_set.len();
        self.participant_set.keys().nth(idx).unwrap().clone()
    }

    pub(crate) fn execute_timeout(&self) -> Duration {
        (self.tbt - 2*self.net_latency)/2
    }

    pub(crate) fn wait_timeout(&self, cur_view: ViewNumber, top_qc: &QuorumCertificate) -> Duration {
        let exp = min(u32::MAX as u64, cur_view - top_qc.view_number) as u32;
        // TODO [Alice]: consider removing TBT.
        self.tbt + Duration::new(u64::checked_pow(2, exp).map_or(u64::MAX, identity), 0)
    }
}

// These tests test Algorithm in a simulated multi-Participant setting without networking.
#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::sync::mpsc::{self, Sender, Receiver};
    use std::time::{Duration, Instant};
    use std::thread;
    use tempfile;
    use rand;
    use rand::rngs::OsRng;
    use hotstuff_rs_types::identity::{ParticipantSet, KeyPair, PublicKeyBytes};
    use hotstuff_rs_types::messages::{QuorumCertificate, ConsensusMsg, BlockHeight, Block, Data, DataHash};
    use hotstuff_rs_types::stored::StorageMutations;
    use crate::block_tree::{self, BlockTreeSnapshotFactory};
    use crate::config::{BlockTreeStorageConfig, NetworkingConfiguration, ProgressModeNetworkingConfig, SyncModeNetworkingConfig, IdentityConfig, AlgorithmConfig};
    use crate::ipc::{AbstractHandle, NotConnectedError, RecvFromError};
    use crate::rest_api::{AbstractSyncModeClient, GetBlocksFromTailError};
    use crate::app::{App, SpeculativeStorageReader};
    use super::{Algorithm, Pacemaker};

    #[test]
    fn one_participant() {
        // Make a BlockTree for the Participant.
        let block_tree_db_dir = tempfile::tempdir().unwrap();
        let block_tree_config = BlockTreeStorageConfig { db_path: block_tree_db_dir.path().to_path_buf() };
        let (block_tree_writer, block_tree_snapshot_factory) = block_tree::open(&block_tree_config);
        let mock_app = MockApp { pending_additions: 5 };

        // Prepare configuration structs.
        let networking_config = make_mock_networking_config(Duration::new(1, 0));
        let identity_config = make_mock_identity_configs(1).into_iter().next().unwrap();
        let algorithm_config = make_mock_algorithm_config();

        // Prepare mocked network handles.
        let ipc_handle = {
            let participants = identity_config.static_participant_set.iter().map(|(public_addr, _)| *public_addr).collect();
            let mut mock_ipc_handle_factory = MockIPCHandleFactory::new(participants);
            mock_ipc_handle_factory.get_handle(identity_config.my_public_key.to_bytes())
        };
        let sync_mode_client = {
            let mut bt_snapshot_factories = HashMap::new();
            bt_snapshot_factories.insert(identity_config.my_public_key.to_bytes(), block_tree_snapshot_factory.clone());
            MockSyncModeClient::new(bt_snapshot_factories)
        };

        // Prepare 1 Algorithm.
        let mut algorithm = Algorithm {
            cur_view: 1,
            top_qc: QuorumCertificate::genesis_qc(identity_config.static_participant_set.len()),
            block_tree: block_tree_writer,
            pacemaker: Pacemaker { 
                tbt: algorithm_config.target_block_time,
                participant_set: identity_config.static_participant_set.clone(),
                net_latency: networking_config.progress_mode.expected_worst_case_net_latency,
            },
            app: mock_app,
            ipc_handle,
            sync_mode_client,
            round_robin_idx: 0,
            networking_config,
            identity_config,
            algorithm_config,
        };

        // Start the Algorithm in the background.
        thread::spawn(move || algorithm.start());

        // Periodically query the BlockTree until number == 5  (all pending additions are successfully applied).
        loop {
            let bt_snapshot = block_tree_snapshot_factory.snapshot();
            if let Some(state) = bt_snapshot.get_from_storage(&vec![]) {
                if let Some(number) = state.get(0) {
                    if *number == 5 {
                        break
                    }
                }
            }
            thread::sleep(Duration::new(1, 0));
        }
    }

    #[test]
    fn three_participants() {
        todo!()
    } 

    struct MockApp {
        pending_additions: usize,
    }

    impl App for MockApp {
        fn propose_block(
            &mut self, 
            parent_state: Option<(crate::app::Block, crate::app::SpeculativeStorageReader)>,
            _deadline: std::time::Instant
        ) -> (Data, DataHash, StorageMutations) {
            if self.pending_additions >= 1 {
                self.pending_additions -= 1;
                let cur_value = match parent_state {
                    Some((_, storage)) => storage.get(&vec![]).unwrap_or(vec![0])[0],
                    None => 0,
                };
                let data = vec![vec![1]];
                let data_hash = [1u8; 32];
                let mut storage_mutations = StorageMutations::new();
                storage_mutations.insert(vec![], vec![cur_value+1]);

                (data, data_hash, storage_mutations)
            } else {
                let data = vec![vec![]];
                let data_hash = [0u8; 32];
                let storage_mutations = StorageMutations::new();

                (data, data_hash, storage_mutations)
            }
        }

        fn validate_block(
            &mut self,
            block: crate::app::Block,
            storage: Option<SpeculativeStorageReader>,
            _deadline: std::time::Instant
        ) -> Result<StorageMutations, crate::app::ExecuteError> {
            let addition = block.data.get(0).unwrap_or(&vec![0])[0];
            let cur_value = match storage {
                Some(storage) => storage.get(&vec![]).unwrap_or(vec![0])[0],
                None => 0,
            };
            let mut storage_mutations = StorageMutations::new();
            storage_mutations.insert(vec![], vec![cur_value+addition]);
            Ok(storage_mutations)
        }
    }

    struct MockIPCHandleFactory {
        peers: HashMap<PublicKeyBytes, Sender<(OriginAddr, ConsensusMsg)>>,
        mailboxes: HashMap<PublicKeyBytes, Option<Receiver<(OriginAddr, ConsensusMsg)>>>,
    }

    type OriginAddr = PublicKeyBytes;

    impl MockIPCHandleFactory {
        fn new(participants: Vec<PublicKeyBytes>) -> MockIPCHandleFactory {
            let mut peers = HashMap::new();
            let mut mailboxes = HashMap::new();
            for addr in participants {
                let (sender, receiver) = mpsc::channel();
                peers.insert(addr, sender);
                mailboxes.insert(addr, Some(receiver));
            }

            MockIPCHandleFactory {
                peers, 
                mailboxes
            }
        }

        fn get_handle(&mut self, for_addr: PublicKeyBytes) -> MockIPCHandle {
            MockIPCHandle {
                my_addr: for_addr,
                peers: self.peers.clone(),
                mailbox: self.mailboxes.get_mut(&for_addr).unwrap().take().unwrap(),
                msgs_stash: VecDeque::new(),
            }
        }
    }

    struct MockIPCHandle {
        my_addr: PublicKeyBytes,
        peers: HashMap<PublicKeyBytes, Sender<(OriginAddr, ConsensusMsg)>>,
        mailbox: Receiver<(OriginAddr, ConsensusMsg)>,
        // Messages that have been removed from mailbox through recv_from, but did not come from the desired 
        // sender address.
        msgs_stash: VecDeque<(OriginAddr, ConsensusMsg)>,
    }

    impl AbstractHandle for MockIPCHandle {
        fn broadcast(&mut self, msg: &ConsensusMsg) {
            for peer in self.peers.values() {
                peer.send((self.my_addr, msg.clone())).unwrap();
            }
        } 

        fn send_to(&mut self, public_addr: &PublicKeyBytes, msg: &ConsensusMsg) -> Result<(), NotConnectedError> {
            self.peers.get(public_addr).unwrap().send((self.my_addr, msg.clone()));
            Ok(())
        }

        fn recv_from(&mut self, public_addr: &PublicKeyBytes, timeout: Duration) -> Result<ConsensusMsg, RecvFromError> {
            let deadline = Instant::now() + timeout;

            // Try to get a matching message from msgs_stash.
            if let Some(matching_idx) = self.msgs_stash.iter().position(|(origin, _)| origin == public_addr) {
                return Ok(self.msgs_stash.remove(matching_idx).unwrap().1)
            } 

            // Try to receive a matching message from mailbox until deadline.
            while Instant::now() < deadline {
                if let Ok((origin, msg)) = self.mailbox.recv_timeout(deadline - Instant::now()) {
                    if origin == *public_addr {
                        return Ok(msg);
                    } else {
                        self.msgs_stash.push_back((origin, msg));
                    }
                }
            }

            Err(RecvFromError::Timeout)
        }

        fn recv_from_any(&mut self, timeout: Duration) -> Result<(PublicKeyBytes, ConsensusMsg), RecvFromError> {
            // Try to get a message from msgs_stash, if it's not empty.
            if let Some((origin, msg)) = self.msgs_stash.pop_front() {
                Ok((origin, msg))
            }

            // Try to receive a message from mailbox until timeout elapses.
            else {
                self.mailbox.recv_timeout(timeout).map_err(|_| RecvFromError::Timeout)
            }
        }
    }

    struct MockSyncModeClient(HashMap<PublicKeyBytes, BlockTreeSnapshotFactory>);

    impl MockSyncModeClient {
        fn new(bt_snapshot_factories: HashMap<PublicKeyBytes, BlockTreeSnapshotFactory>) -> MockSyncModeClient {
            MockSyncModeClient(bt_snapshot_factories)
        }
    }

    impl AbstractSyncModeClient for MockSyncModeClient {
        fn get_blocks_from_tail(&self, participant: PublicKeyBytes, tail_block_height: BlockHeight, limit: usize) -> Result<Vec<Block>, GetBlocksFromTailError> {
            let bt_snapshot = self.0.get(&participant).unwrap().snapshot();
            if let Some(tail_block_hash) = bt_snapshot.get_committed_block_hash(tail_block_height) {
                if let Some(chain) = bt_snapshot.get_blocks_from_tail(&tail_block_hash, limit, true) {
                    Ok(chain)
                } else {
                    Err(GetBlocksFromTailError::TailBlockNotFound)
                }
            } else {
                Err(GetBlocksFromTailError::TailBlockNotFound)
            } 
        }
    }

    fn make_mock_identity_configs(n: usize) -> Vec<IdentityConfig> {
        let mut csprng = OsRng {};
        let mut keypairs = Vec::with_capacity(n);
        for _ in 0..n {
            keypairs.push(KeyPair::generate(&mut csprng));
        }

        let mut static_participant_set = ParticipantSet::new();
        let dummy_ip_addr = "0.0.0.0".parse().unwrap();
        for keypair in &keypairs {
            static_participant_set.insert(keypair.public.to_bytes(), dummy_ip_addr);
        }

        let mut identity_configs = Vec::with_capacity(n);
        for keypair in keypairs.into_iter() {
            identity_configs.push(IdentityConfig {
                my_public_key: keypair.public.clone(),
                my_keypair: keypair,
                static_participant_set: static_participant_set.clone(),
            });
        }

        identity_configs.try_into().unwrap()
    }

    fn make_mock_networking_config(expected_worst_case_net_latency: Duration) -> NetworkingConfiguration {
        NetworkingConfiguration {
            progress_mode: ProgressModeNetworkingConfig {
                expected_worst_case_net_latency,

                // All the fields below are unused and are set to dummy values.
                listening_addr: "0.0.0.0".parse().unwrap(),
                listening_port: 0,
                initiator_timeout: Duration::new(0, 0),
                read_timeout: Duration::new(0, 0),
                write_timeout: Duration::new(0, 0),
                reader_channel_buffer_len: 0,
                writer_channel_buffer_len: 0,
            },
            sync_mode: SyncModeNetworkingConfig {
                // All fields are unused.
                request_jump_size: 0,
                request_timeout: Duration::new(0, 0),
                block_tree_api_listening_addr: "0.0.0.0".parse().unwrap(),
                block_tree_api_listening_port: 0,
            },
        }
    }

    fn make_mock_algorithm_config() -> AlgorithmConfig {
        AlgorithmConfig {
            app_id: 0,
            target_block_time: Duration::new(1, 0),
            sync_mode_execution_timeout: Duration::new(1, 0),
        }
    }
}
