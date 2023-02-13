use std::time::Duration;
use crate::types::Keypair;

#[derive(Clone)]
pub struct Configuration {
    pub sync_mode_execution_timeout: Duration,
    
}