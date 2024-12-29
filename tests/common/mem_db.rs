//! A simple, volatile, in-memory implementation of [`KVStore``].

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

use hotstuff_rs::block_tree::pluggables::{KVGet, KVStore, WriteBatch};

/// An in-memory implementation of [`KVStore`].
#[derive(Clone)]
pub(crate) struct MemDB(Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>);

impl MemDB {
    /// Create a new, empty `MemDB`.
    pub(crate) fn new() -> MemDB {
        MemDB(Arc::new(Mutex::new(HashMap::new())))
    }
}

impl KVStore for MemDB {
    type WriteBatch = MemWriteBatch;
    type Snapshot<'a> = MemDBSnapshot<'a>;

    fn write(&mut self, wb: Self::WriteBatch) {
        let mut map = self.0.lock().unwrap();
        for (key, value) in wb.insertions {
            map.insert(key, value);
        }
        for key in wb.deletions {
            map.remove(&key);
        }
    }

    fn clear(&mut self) {
        self.0.lock().unwrap().clear();
    }

    fn snapshot<'b>(&'b self) -> MemDBSnapshot<'b> {
        MemDBSnapshot(self.0.lock().unwrap())
    }
}

impl KVGet for MemDB {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.lock().unwrap().get(key).cloned()
    }
}

// A simple implementation of [`WriteBatch`].
pub(crate) struct MemWriteBatch {
    insertions: HashMap<Vec<u8>, Vec<u8>>,
    deletions: HashSet<Vec<u8>>,
}

impl WriteBatch for MemWriteBatch {
    fn new() -> Self {
        MemWriteBatch {
            insertions: HashMap::new(),
            deletions: HashSet::new(),
        }
    }

    fn set(&mut self, key: &[u8], value: &[u8]) {
        let _ = self.deletions.remove(key);
        self.insertions.insert(key.to_vec(), value.to_vec());
    }

    fn delete(&mut self, key: &[u8]) {
        let _ = self.insertions.remove(key);
        self.deletions.insert(key.to_vec());
    }
}

/// A simple implementation of [`KVGet`] used as `KVStore::Snapshot` for `MemDB`.
pub(crate) struct MemDBSnapshot<'a>(MutexGuard<'a, HashMap<Vec<u8>, Vec<u8>>>);

impl KVGet for MemDBSnapshot<'_> {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.get(key).cloned()
    }
}
