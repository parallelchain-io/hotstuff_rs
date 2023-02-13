use ed25519_dalek::Keypair;
use crate::app::App;
use crate::config::Configuration;
use crate::networking::Network;
use crate::pacemaker::Pacemaker;
use crate::state::{BlockTreeCamera, KVStore};
use crate::types::{AppStateUpdates, ValidatorSetUpdates};
pub struct Replica;

impl Replica {
    pub fn initialize<'a, K: KVStore<'a>>(
        kv_store: &mut K,
        initial_app_state: AppStateUpdates, 
        initial_validator_set: ValidatorSetUpdates
    ) {
        todo!()
    }

    pub fn start<'a, K: KVStore<'a>>(
        app: impl App<K::Snapshot>,
        keypair: Keypair,
        network_stub: impl Network,
        kv_store: K,
        pacemaker: impl Pacemaker,
        configuration: Configuration,
    ) -> (Replica, BlockTreeCamera<'a, K>) {
        todo!()
    }
}

