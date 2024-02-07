/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0.
*/

//! [Trait definition](Pacemaker) for pacemakers: user-provided types that determine the 'liveness' behavior of replicas.
//!
//! Specifically, pacemakers tell replica code two things:
//! 1. [View timeout](Pacemaker::view_timeout): given the current view, how long should I stay in the current view to wait for messages?
//! 2. [View leader](Pacemaker::view_leader): given the current view, and the current validator set, who should I consider
//!    the current leader?
//! 3. [Should sync](Pacemaker::should_sync): given the current view, should I sync with other validators to make sure that
//!    future views timeout at the same time for all validators? This method is optional, a default implementation is to always
//!    return false.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::types::*;

pub trait Pacemaker: Send {
    fn view_timeout(
        &mut self,
        cur_view: ViewNumber,
    ) -> Instant;

    fn view_leader(&mut self, cur_view: ViewNumber, validator_set: &ValidatorSet)
        -> VerifyingKey;

    fn should_sync(&self, cur_view: ViewNumber)
        -> bool;
}

/// A pacemaker which selects leaders in a weighted round-robin fashion, and sets view timeouts for an epoch upon entering the epoch.
/// 
/// ## Leader selection
/// The leader selection proceeds according to Interleaved Weighted Round Robin, which guarantees that the frequency with which
/// a given validator is selected as the leader is proportional to its power. However, the views in which validators act as leaders
/// are interleaved, rather than consecutive, unlike in the Classical Weighted Round Robin.
/// 
/// ## View timeout
/// The view timeout is determined by the timeout instant allocated to the view at the begginning of the corresponding epoch, 
/// following the Lewis-Pye view synchronization protocol.
/// An epoch is a sequence of views of fixed length during which replicas do not need to communicate before advancing to a new view,
/// rather they advance on view timeout or on seeing a QC for the current view.
/// The view timeouts set on entering a new epoch guarantee guarantee that the replicas' views are in sync for the duration of an epoch
/// provided that the validators synchronize via all-to-all broadcast before entering a new epoch.
pub struct DefaultPacemaker {
    view_time: Duration,
    epoch_length: u32,
    cur_epoch: u64,
    timeouts: BTreeMap<ViewNumber, Instant>,
    last_view_and_leader: Option<(ViewNumber, VerifyingKey)>
}

impl DefaultPacemaker {
    /// # Safety
    /// `view_time` must not be larger than [u32::MAX] seconds. This is to avoid overflows in the [Pacemaker::view_timeout]
    /// implementation which adds this duration to an instant.
    /// `epoch_length` must be greater than 0, but not larger than [u32::MAX] to avoid overflows in the [Pacemaker::view_timeout]
    /// implementation which multiplies `view_time` by at most `epoch_length`.
    pub fn new(
            view_time: Duration,
            epoch_length: u32,
            cur_epoch: u64,
        ) -> DefaultPacemaker {
        Self {
            view_time,
            epoch_length,
            cur_epoch,
            timeouts: BTreeMap::new(),
            last_view_and_leader: None,
        }
    }
}

impl Pacemaker for DefaultPacemaker {
    fn view_leader(
        &mut self,
        cur_view: ViewNumber,
        validator_set: &ValidatorSet,
    ) -> VerifyingKey {
        
        if self.last_view_and_leader.is_some_and(|(v, _)| v == cur_view) {
            return self.last_view_and_leader.unwrap().1
        }

        let total_power = validator_set.total_power();
        let n = validator_set.len();
        let index = cur_view % total_power as u64;
        let max_power = validator_set.validators_and_powers().into_iter()
            .map(|(_, power)| power).max().expect("The validator set is empty!");
        
        let mut counter = 0;

        // Search for a validator with a given index in the abstract array of leaders.
        for threshold in 1..=max_power {
            for validator_no in 0..n {
                let validator = validator_set.validators().nth(validator_no).unwrap();
                if validator_set.power(validator).unwrap() >= &threshold {
                    if counter == index {
                        self.last_view_and_leader = Some((cur_view, *validator));
                        return *validator
                    }
                    counter += 1
                }
            }
        }

        // This should never happen.
        panic!("Cannot compute the view leader!")


    }

    fn view_timeout(
        &mut self,
        cur_view: ViewNumber,
    ) -> Instant {
        
        let cur_epoch = cur_view.div_ceil(self.epoch_length.into());

        if cur_epoch < self.cur_epoch {
            panic!("Cannot enter a previous epoch!")
        }

        // If just entered a new epoch then set the timeouts for the remaining views in the epoch.
        if cur_epoch > self.cur_epoch {

            self.cur_epoch = cur_epoch;
            self.timeouts = self.timeouts.split_off(&cur_view);
            
            let epoch_view = (self.epoch_length as u64).checked_mul(self.cur_epoch).expect("Integer overflow when computing the epoch view!");
            let epoch_start = Instant::now();
            for view in cur_view..=epoch_view {
                // let view_timeout = epoch_start + (self.view_time * ((view - cur_view + 1).try_into().unwrap()));
                let view_timeout = 
                    epoch_start.checked_add(
                    self.view_time.checked_mul(
                                (view - cur_view + 1).try_into()
                                .expect("Integer overflow when computing view timeout!")
                                )
                                .expect("Integer overflow when computing view timeout!")
                            )
                            .expect("Integer overflow when computing view timeout!");

                self.timeouts.insert(view, view_timeout);
            }    
        }

        // Get the timeout for the current view.
        *self.timeouts.get(&cur_view).expect("The timeout for view no. {cur_view} has not been set.")

    }

    fn should_sync(
        &self, 
        cur_view: ViewNumber
    ) -> bool {
        
        cur_view % (self.epoch_length as u64) == 0
    }
}
