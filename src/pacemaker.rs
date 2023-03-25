/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0.
*/

//! [Trait definition](Pacemaker) for pacemakers: user-provided types that determine the 'liveness' behavior of replicas.
//! 
//! Specifically, pacemakers tell replica code four things:
//! 1. [View timeout](Pacemaker::view_timeout): given the current view, and the highest view I (the replica) has seen
//!    consensus progress, how long should I stay in the current view to wait for messages?
//! 2. [View leader](Pacemaker::view_leader): given the current view, and the current validator set, who should I consider
//!    the current leader?
//! 3. [Sync request limit](Pacemaker::sync_request_limit): when I'm syncing, how many blocks should I request my sync peer
//!    send in a single response?
//! 4. [Sync response timeout](Pacemaker::sync_response_timeout): how long should I wait for a response to a sync request
//!    that I make? 

use std::cmp::min;
use std::convert::identity;
use std::time::Duration;
use crate::types::*;

pub trait Pacemaker: Send {
    fn view_timeout(&mut self, cur_view: ViewNumber, highest_qc_view_number: ViewNumber) -> Duration;
    fn view_leader(&mut self, cur_view: ViewNumber, validator_set: &ValidatorSet) -> PublicKeyBytes;
    fn sync_request_limit(&self) -> u32;
    fn sync_response_timeout(&self) -> Duration;
}

/// A pacemaker which selects leaders in a round-robin fashion, and prescribes exponentially increasing view timeouts to
/// eventually bring replicas into the same view.
pub struct DefaultPacemaker {
    minimum_view_timeout: Duration,
    sync_request_limit: u32,
    sync_response_timeout: Duration
}

impl DefaultPacemaker {
    pub fn new(minimum_view_timeout: Duration, sync_request_limit: u32, sync_response_timeout: Duration) -> DefaultPacemaker {
        Self { 
            minimum_view_timeout,
            sync_request_limit, 
            sync_response_timeout 
        }
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

    fn sync_request_limit(&self) -> u32 {
        self.sync_request_limit
    }

    fn sync_response_timeout(&self) -> Duration {
        self.sync_response_timeout
    }
}
