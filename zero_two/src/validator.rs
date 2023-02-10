use crate::app::App;
use crate::config::Configuration;
use crate::state::{BlockTreeSnapshot, KVCamera};
use crate::types::{AppStateUpdates, ValidatorSetUpdates};
pub struct Validator;

impl Validator {
    pub fn initialize(initial_app_state: AppStateUpdates, initial_validator_set: ValidatorSetUpdates);
    pub fn start<S: KVCamera>(app: impl App, configuration: Configuration) -> (Validator, BlockTreeSnapshot<S>);
}

