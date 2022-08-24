use std::time::Duration;
use crate::msg_types::{ViewNumber, QuorumCertificate};
use crate::identity::{PublicAddr, ParticipantSet};

pub fn leader(cur_view: ViewNumber, participant_set: ParticipantSet) -> PublicAddr {
    todo!()
}

pub fn timeout(tnt: Duration, cur_view: ViewNumber, generic_qc: QuorumCertificate) -> Duration {
    todo!()
}