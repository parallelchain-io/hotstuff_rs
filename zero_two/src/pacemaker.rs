/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0.
    
    Authors: Alice Lim
*/

use std::time::Duration;
use crate::types::*;

pub trait Pacemaker: 'static {
    fn view_timeout(&self, cur_view: ViewNumber, highest_qc_view_number: ViewNumber) -> Duration;
    fn view_leader(&self, cur_view: ViewNumber, validator_set: ValidatorSet) -> PublicKeyBytes;
}

pub struct DefaultPacemaker;

impl Pacemaker for DefaultPacemaker {
    fn view_leader(&self, cur_view: ViewNumber, validator_set: ValidatorSet) -> PublicKeyBytes {
        todo!() 
    }
    
    fn view_timeout(&self, cur_view: ViewNumber, highest_qc_view_number: ViewNumber) -> Duration {
        todo!() 
    }
}
