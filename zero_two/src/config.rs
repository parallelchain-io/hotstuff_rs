use std::time::Duration;
use crate::types::Keypair;

pub struct Configuration {
    pub keypair: Keypair,
    pub sync_mode_execution_timeout: Duration,
    
}