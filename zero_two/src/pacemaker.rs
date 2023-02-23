/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0.
    
    Authors: Alice Lim
*/

use std::cmp::min;
use std::convert::identity;
use std::time::Duration;
use crate::types::*;

pub trait Pacemaker: Send {
    fn view_timeout(&mut self, cur_view: ViewNumber, highest_qc_view_number: ViewNumber) -> Duration;
    fn view_leader(&mut self, cur_view: ViewNumber, validator_set: &ValidatorSet) -> PublicKeyBytes;
    fn proposal_rebroadcast_period(&self) -> Duration;
    fn sync_request_limit(&self) -> u32;
    fn sync_response_timeout(&self) -> Duration;
}

pub struct DefaultPacemaker {
    minimum_view_timeout: Duration,
    proposal_rebroadcast_period: Duration,
    sync_request_limit: u32,
    sync_response_timeout: Duration
}

impl DefaultPacemaker {
    pub fn new(minimum_view_timeout: Duration, proposal_rebroadcast_period: Duration, sync_request_limit: u32, sync_response_timeout: Duration) -> DefaultPacemaker {
        Self { minimum_view_timeout, proposal_rebroadcast_period, sync_request_limit, sync_response_timeout }
    }
}

impl Pacemaker for DefaultPacemaker {
    fn view_leader(&mut self, cur_view: ViewNumber, validator_set: &ValidatorSet) -> PublicKeyBytes {
        let num_validators = validator_set.len();
        *validator_set.validators().skip(cur_view as usize % num_validators).next().unwrap()
    }
    
    fn view_timeout(&mut self, cur_view: ViewNumber, highest_qc_view_number: ViewNumber) -> Duration {
        let exp = min(u32::MAX as u64, cur_view - highest_qc_view_number) as u32;
        self.minimum_view_timeout + Duration::new(u64::checked_pow(2, exp).map_or(u64::MAX, identity), 0)
    }

    fn proposal_rebroadcast_period(&self) -> Duration {
        self.proposal_rebroadcast_period
    }

    fn sync_request_limit(&self) -> u32 {
        self.sync_request_limit
    }

    fn sync_response_timeout(&self) -> Duration {
        self.sync_response_timeout
    }
}
