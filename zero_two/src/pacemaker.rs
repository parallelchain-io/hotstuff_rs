/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0.
    
    Authors: Alice Lim
*/

use std::time::Duration;
use crate::types::*;

pub trait Pacemaker: Send {
    fn view_timeout(&mut self, cur_view: ViewNumber, highest_qc_view_number: ViewNumber) -> Duration;
    fn view_leader(&mut self, cur_view: ViewNumber, validator_set: ValidatorSet) -> PublicKeyBytes;
    fn proposal_rebroadcast_period(&self) -> Duration;
    fn sync_request_limit(&self) -> u32;
}

pub struct DefaultPacemaker;

impl Pacemaker for DefaultPacemaker {
    fn view_leader(&mut self, cur_view: ViewNumber, validator_set: ValidatorSet) -> PublicKeyBytes {
        todo!() 
    }
    
    fn view_timeout(&mut self, cur_view: ViewNumber, highest_qc_view_number: ViewNumber) -> Duration {
        todo!() 
    }

    fn proposal_rebroadcast_period(&self) -> Duration {
        todo!()
    }

    fn sync_request_limit(&self) -> u32 {
        todo!()
    }
}
