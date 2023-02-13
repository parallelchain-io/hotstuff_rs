use crate::types::*;
use crate::state::{BlockTreeSnapshot, KVGet};

pub trait App<K: KVGet> {
    fn id(&self) -> AppID;
    fn propose_block(&mut self, request: ProposeBlockRequest<K>) -> ProposeBlockResponse;
    fn validate_block(&mut self, request: ValidateBlockRequest<K>) -> ValidateBlockResponse;
}

pub struct ProposeBlockRequest<'a, S: KVGet> {
    cur_view: ViewNumber,
    prev_block: CryptoHash,
    block_tree: BlockTreeSnapshot<'a, S>,
    app_state: SpeculativeAppState,
}

impl<'a, S: KVGet> ProposeBlockRequest<'a, S> {
    pub fn cur_view(&self) -> ViewNumber {
        self.cur_view
    }

    pub fn prev_block(&self) -> CryptoHash {
        self.prev_block
    }

    pub fn block_tree(&self) -> &BlockTreeSnapshot<'a, S> {
        &self.block_tree
    }

    pub fn app_state(&self) -> &SpeculativeAppState {
        &self.app_state
    }
}

pub struct ProposeBlockResponse {
    data: Data,
    app_state_updates: Option<AppStateUpdates>,
    validator_set_updates: Option<ValidatorSetUpdates>
}

pub struct ValidateBlockRequest<'a, S: KVGet> {
    cur_view: ViewNumber,
    proposed_block: Block,
    block_tree: BlockTreeSnapshot<'a, S>,
    app_state: SpeculativeAppState,
}

impl<'a, S: KVGet> ValidateBlockRequest<'a, S> {
    fn cur_view(&self) -> ViewNumber {
        self.cur_view
    }

    fn proposed_block(&self) -> &Block {
        &self.proposed_block
    }

    fn block_tree(&self) -> &BlockTreeSnapshot<'a, S> {
        &self.block_tree
    }

    pub fn app_state(&self) -> &SpeculativeAppState {
        &self.app_state
    }
}

pub enum ValidateBlockResponse {
    Valid {
        app_state_updates: Option<AppStateUpdates>,
        validator_set_updates: Option<ValidatorSetUpdates>
    },
    Invalid,
}

// TODO: copy from hotstuff 0.1.
struct SpeculativeAppState;

impl SpeculativeAppState {
    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        todo!()
    }
}

