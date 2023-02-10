use std::time::Duration;
use crate::types::*;

trait Pacemaker {
    fn view_timeout(cur_view: ViewNumber, highest_qc_view_number: ViewNumber) -> Duration;
    fn view_leader(cur_view: ViewNumber, validator_set: ValidatorSet) -> PublicKey;
}