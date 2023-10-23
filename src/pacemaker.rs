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

use crate::types::*;
use std::cmp::min;
use std::convert::identity;
use std::time::Duration;

/// # Safety
///
/// Durations returned by [Pacemaker::view_timeout] and [Pacemaker::sync_response_timeout] must be "well below"
/// [u64::MAX] seconds. A good limit is to cap them at [u32::MAX].
///
/// In the most popular target platforms, Durations can only go up to [u64::MAX] seconds, so keeping returned
/// durations lower than [u64::MAX] avoids overflows in calling code, which may add to the returned duration.
pub trait Pacemaker: Send {
    fn view_timeout(
        &mut self,
        cur_view: ViewNumber,
        highest_qc_view_number: ViewNumber,
    ) -> Duration;

    fn view_leader(&mut self, cur_view: ViewNumber, validator_set: &ValidatorSet)
        -> VerifyingKey;
}

/// A pacemaker which selects leaders in a round-robin fashion, and prescribes exponentially increasing view timeouts to
/// eventually bring replicas into the same view.
pub struct DefaultPacemaker {
    minimum_view_timeout: Duration,
}

impl DefaultPacemaker {
    /// # Safety
    ///
    /// `minimum_view_timeout` and `sync_response_timeout` must not be larger than [u32::MAX] seconds for reasons
    /// explained in [Pacemaker].
    pub fn new(minimum_view_timeout: Duration) -> DefaultPacemaker {
        Self {
            minimum_view_timeout,
        }
    }
}

impl Pacemaker for DefaultPacemaker {
    fn view_leader(
        &mut self,
        cur_view: ViewNumber,
        validator_set: &ValidatorSet,
    ) -> VerifyingKey {
        let num_validators = validator_set.len();
        *validator_set
            .validators()
            .nth((cur_view % num_validators as u64) as usize)
            .unwrap()
    }

    fn view_timeout(
        &mut self,
        cur_view: ViewNumber,
        highest_qc_view_number: ViewNumber,
    ) -> Duration {
        let exp = min(u32::MAX as u64, cur_view - highest_qc_view_number) as u32;
        self.minimum_view_timeout
            + Duration::new(
                u32::checked_pow(2, exp).map_or(u32::MAX, identity) as u64,
                0,
            )
    }
}
