//! [`NumberApp`], a simple implementation of [`App`] currently used in all of the integration tests.

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
    block_tree::{accessors::public::BlockTreeSnapshot, pluggables::KVGet},
    types::{
        crypto_primitives::{CryptoHasher, Digest, VerifyingKey},
        data_types::{CryptoHash, Data, Datum, Power},
        update_sets::{AppStateUpdates, ValidatorSetUpdates},
    },
};

use crate::common::{mem_db::MemDB, verifying_key_bytes::VerifyingKeyBytes};

/// A simple implementation of [`App`] for use in integration tests.
///
/// The number app maintains an app state consisting of a single number, which can be queried using the
/// [`number`](`NumberApp::number`) function. Users can increase this number by submitting
/// [`Increment`](NumberAppTransaction::Increment) transactions to the app's `tx_queue`, which is
/// provided to the number app during creation as an argument of the struct's [`new`](NumberApp::new)
/// constructor.
///
/// ## Timing
///
/// In order to reduce the rate at which blocks are produced in integration tests `NumberApp` is hardcoded
/// to spend at least 250 milliseconds in `produce_block`, and 250 milliseconds in `validate_block`.
/// This means that at the *bare minimum*, replicas maintaining a `NumberApp` should configure their
/// `max_view_time` to be 500 milliseconds if they are to consistently make progress.
pub(crate) struct NumberApp {
    tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>,
}

/// User-sent instructions that number app execute in [`produce_block`](App::produce_block) and
/// [`validate_block`](App::validate_block).
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum NumberAppTransaction {
    /// Increase the number in the app state by 1.
    Increment,

    /// Add a new validator with the specified verifying key and power or change the power of an existing
    /// validator.
    SetValidator(VerifyingKeyBytes, Power),

    /// Delete an existing validator. If the specified validator does not exist, this transaction is a no-
    /// op.
    DeleteValidator(VerifyingKeyBytes),
}

// The key in the app state where the "number" is stored.
const NUMBER_KEY: [u8; 1] = [0];

impl NumberApp {
    /// Create a new number app that which will pop and execute transactions from the provided
    /// `tx_queue`.
    ///
    /// Callers should clone a reference to the `tx_queue` before calling this constructor and use the
    /// reference to insert transactions to the `tx_queue` whenever needed.
    pub(crate) fn new(tx_queue: Arc<Mutex<Vec<NumberAppTransaction>>>) -> NumberApp {
        Self { tx_queue }
    }

    /// Return an `AppStateUpdates` that when applied on an empty app state will produce a good "initial"
    /// app state for a number app: one containing the number 0.
    pub(crate) fn initial_app_state() -> AppStateUpdates {
        let mut state = AppStateUpdates::new();
        state.insert(NUMBER_KEY.to_vec(), u32::to_le_bytes(0).to_vec());
        state
    }

    /// Get the number stored in a number app's app state from the given block tree.
    pub(crate) fn number<S: KVGet>(block_tree: BlockTreeSnapshot<S>) -> u32 {
        u32::deserialize(&mut &*block_tree.committed_app_state(&NUMBER_KEY).unwrap()).unwrap()
    }
}

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
            let mut hasher = CryptoHasher::new();
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

        self.validate_block_for_sync(request)
    }

    fn validate_block_for_sync(
        &mut self,
        request: ValidateBlockRequest<MemDB>,
    ) -> ValidateBlockResponse {
        let data = &request.proposed_block().data;
        let data_hash: CryptoHash = {
            let mut hasher = CryptoHasher::new();
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
}

impl NumberApp {
    /// Given the `current_number`, execute the given `transactions` and return the resulting
    /// `AppStateUpdates` and `ValidatorSetUpdates`.
    fn execute(
        &self,
        current_number: u32,
        transactions: &Vec<NumberAppTransaction>,
    ) -> (Option<AppStateUpdates>, Option<ValidatorSetUpdates>) {
        let mut number = current_number;
        let mut validator_set_updates: Option<ValidatorSetUpdates> = None;

        for transaction in transactions {
            match transaction {
                NumberAppTransaction::Increment => {
                    number += 1;
                }
                NumberAppTransaction::SetValidator(validator, power) => {
                    validator_set_updates
                        .get_or_insert(ValidatorSetUpdates::new())
                        .insert(VerifyingKey::from_bytes(validator).unwrap(), *power);
                }
                NumberAppTransaction::DeleteValidator(validator) => {
                    validator_set_updates
                        .get_or_insert(ValidatorSetUpdates::new())
                        .delete(VerifyingKey::from_bytes(validator).unwrap());
                }
            }
        }

        let app_state_updates = if number != current_number {
            let mut updates = AppStateUpdates::new();
            updates.insert(NUMBER_KEY.to_vec(), number.try_to_vec().unwrap());
            Some(updates)
        } else {
            None
        };

        (app_state_updates, validator_set_updates)
    }
}
