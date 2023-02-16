/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

use std::time::Duration;
use crate::types::Keypair;

#[derive(Clone)]
pub struct Configuration {
    pub sync_request_limit: u32,
    pub proposal_rebroadcast_period: Duration,
    
}