use std::time::Duration;
use crate::types::*;

pub trait Pacemaker: 'static {
    fn view_timeout(cur_view: ViewNumber, highest_qc_view_number: ViewNumber) -> Duration;
    fn view_leader(cur_view: ViewNumber, validator_set: ValidatorSet) -> PublicKey;
}

pub struct DefaultPacemaker;

impl Pacemaker for DefaultPacemaker {
    fn view_leader(cur_view: ViewNumber, validator_set: ValidatorSet) -> PublicKey {
        todo!() 
    }
    
    fn view_timeout(cur_view: ViewNumber, highest_qc_view_number: ViewNumber) -> Duration {
        todo!() 
    }
}
