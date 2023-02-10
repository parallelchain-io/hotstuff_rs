use crate::types::*;
use crate::state::{BlockTreeSnapshot, KVGet};

pub trait App {
    type K: KVGet;

    fn propose_block(&mut self, request: ProposeBlockRequest<Self::K>) -> ProposeBlockResponse;
    fn validate_block(&mut self, request: ValidateBlockRequest<Self::K>) -> ValidateBlockResponse;
}

pub struct ProposeBlockRequest<K: KVGet> {
    cur_view: ViewNumber,
    prev_block: CryptoHash,
    block_tree: BlockTreeSnapshot<K>,
}

pub struct ProposeBlockResponse {
    data: Data,
    app_state_updates: Option<AppStateUpdates>,
    validator_set_updates: Option<ValidatorSetUpdates>
}

pub struct ValidateBlockRequest<K: KVGet> {
    cur_view: ViewNumber,
    proposed_block: Block,
    block_tree: BlockTreeSnapshot<K>,
}

pub enum ValidateBlockResponse {
    Valid {
        app_state_updates: Option<AppStateUpdates>,
        validator_set_updates: Option<ValidatorSetUpdates>
    },
    Invalid,
}