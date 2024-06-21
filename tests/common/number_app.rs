use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use borsh::{BorshDeserialize, BorshSerialize};
use hotstuff_rs::{
    app::{
        App, ProduceBlockRequest, ProduceBlockResponse, ValidateBlockRequest, ValidateBlockResponse,
    },
    state::{block_tree_camera::BlockTreeSnapshot, kv_store::KVGet},
    types::{
        basic::{AppStateUpdates, CryptoHash, Data, Datum, Power},
        collectors::VerifyingKey,
        validators::ValidatorSetUpdates,
    },
};
use sha2::{Digest, Sha256};

use crate::common::{mem_db::MemDB, verifying_key_bytes::VerifyingKeyBytes};

pub(crate) struct NumberApp {
    tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>,
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum NumberAppTransaction {
    Increment,
    SetValidator(VerifyingKeyBytes, Power),
    DeleteValidator(VerifyingKeyBytes),
}

const NUMBER_KEY: [u8; 1] = [0];

impl App<MemDB> for NumberApp {
    fn produce_block(&mut self, request: ProduceBlockRequest<MemDB>) -> ProduceBlockResponse {
        thread::sleep(Duration::from_millis(250));
        let initial_number = u32::from_le_bytes(
            request
                .block_tree()
                .app_state(&NUMBER_KEY)
                .unwrap()
                .try_into()
                .unwrap(),
        );

        let mut tx_queue = self.tx_queue.lock().unwrap();

        let (app_state_updates, validator_set_updates) = self.execute(initial_number, &tx_queue);
        let data = Data::new(vec![Datum::new(tx_queue.try_to_vec().unwrap())]);
        let data_hash = {
            let mut hasher = Sha256::new();
            hasher.update(&data.vec()[0].bytes());
            let bytes = hasher.finalize().into();
            CryptoHash::new(bytes)
        };

        tx_queue.clear();

        ProduceBlockResponse {
            data_hash,
            data,
            app_state_updates,
            validator_set_updates,
        }
    }

    fn validate_block(&mut self, request: ValidateBlockRequest<MemDB>) -> ValidateBlockResponse {
        thread::sleep(Duration::from_millis(250));
        let data = &request.proposed_block().data;
        let data_hash: CryptoHash = {
            let mut hasher = Sha256::new();
            hasher.update(&data.vec()[0].bytes());
            let bytes = hasher.finalize().into();
            CryptoHash::new(bytes)
        };

        if request.proposed_block().data_hash != data_hash {
            ValidateBlockResponse::Invalid
        } else {
            let initial_number = u32::from_le_bytes(
                request
                    .block_tree()
                    .app_state(&NUMBER_KEY)
                    .unwrap()
                    .try_into()
                    .unwrap(),
            );

            if let Ok(transactions) = Vec::<NumberAppTransaction>::deserialize(
                &mut &*request.proposed_block().data.vec()[0].bytes().as_slice(),
            ) {
                let (app_state_updates, validator_set_updates) =
                    self.execute(initial_number, &transactions);
                ValidateBlockResponse::Valid {
                    app_state_updates,
                    validator_set_updates,
                }
            } else {
                ValidateBlockResponse::Invalid
            }
        }
    }

    fn validate_block_for_sync(
        &mut self,
        request: ValidateBlockRequest<MemDB>,
    ) -> ValidateBlockResponse {
        self.validate_block(request)
    }
}

impl NumberApp {
    pub(crate) fn new(tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>) -> NumberApp {
        Self { tx_queue }
    }

    pub(crate) fn initial_app_state() -> AppStateUpdates {
        let mut state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), u32::to_le_bytes(0).to_vec());
        state
    }

    pub(crate) fn number<S: KVGet>(block_tree: BlockTreeSnapshot<S>) -> u32 {
        u32::deserialize(&mut &*block_tree.committed_app_state(&NUMBER_KEY).unwrap()).unwrap()
    }

    fn execute(
        &self,
        initial_number: u32,
        transactions: &Vec<NumberAppTransaction>,
    ) -> (Option<AppStateUpdates>, Option<ValidatorSetUpdates>) {
        let mut number = initial_number;

        let mut validator_set_updates: Option<ValidatorSetUpdates> = None;
        for transaction in transactions {
            match transaction {
                NumberAppTransaction::Increment => {
                    number += 1;
                }
                NumberAppTransaction::SetValidator(validator, power) => {
                    if let Some(updates) = &mut validator_set_updates {
                        updates.insert(VerifyingKey::from_bytes(validator).unwrap(), *power);
                    } else {
                        let mut vsu = ValidatorSetUpdates::new();
                        vsu.insert(VerifyingKey::from_bytes(validator).unwrap(), *power);
                        validator_set_updates = Some(vsu);
                    }
                }
                NumberAppTransaction::DeleteValidator(validator) => {
                    if let Some(updates) = &mut validator_set_updates {
                        updates.delete(VerifyingKey::from_bytes(validator).unwrap());
                    } else {
                        let mut vsu = ValidatorSetUpdates::new();
                        vsu.delete(VerifyingKey::from_bytes(validator).unwrap());
                        validator_set_updates = Some(vsu);
                    }
                }
            }
        }
        let app_state_updates = if number != initial_number {
            let mut updates = AppStateUpdates::new();
            updates.insert(NUMBER_KEY.to_vec(), number.try_to_vec().unwrap());
            Some(updates)
        } else {
            None
        };

        (app_state_updates, validator_set_updates)
    }
}
