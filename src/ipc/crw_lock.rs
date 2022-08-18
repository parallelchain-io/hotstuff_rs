use std::sync::{Mutex, MutexGuard};
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};

/// CRwLock is a RwLock with a twist: while regular RwLocks allow multiple Readers or a single Writer to exist at
/// any given moment, a CRwLock allows multiple Readers AND a single Writer. Hence the name Conjunction (AND) RwLock.
/// 
/// This exists in `ipc` for a particular reason: TcpStreams are safe to read from and write into concurrently, but
/// are not safe to:
/// 1. Be read from multiple threads concurrently, or
/// 2. Be written from multiple threads concurrently.
/// 
/// Situation 1 is avoided because `Handle::recv_from` and `recv_from_any` are blocking calls, and only one thread (
/// the Worker Thread in Progress Mode) owns a `Handle`, and situation 2 is avoided by this struct's guarantee.
/// 
/// For more context about the creation of this struct, visit: 
/// https://backend-gitlab.digital-transaction.net:3000/parallelchain/source/hotstuff_rs/-/issues/7
pub(crate) struct CRwLock<T> {
    t: SyncUnsafeCell<T>,
    write_lock: Mutex<()>,
}

impl<T> CRwLock<T> {
    pub fn new(t: T) -> CRwLock<T> {
        CRwLock { 
            t: SyncUnsafeCell::new(t), 
            write_lock: Mutex::new(()),
        }
    }

    pub fn read(&self) -> &T {
        unsafe {
            &*self.t.get()
        }
    }

    pub fn write(&self) -> CRwLockWriteGuard<T> {
        let _write_lock_guard = self.write_lock.lock().unwrap();
        unsafe { 
            let t = &mut *self.t.get(); 
            CRwLockWriteGuard { 
                t,
                _write_lock_guard,
            }
        }
    }
}

pub(crate) struct CRwLockWriteGuard<'a, T> {
    t: &'a mut T,
    _write_lock_guard: MutexGuard<'a, ()>,
}

impl<'a, T> Deref for CRwLockWriteGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.t
    }
}

impl<'a, T> DerefMut for CRwLockWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.t
    } 
}

struct SyncUnsafeCell<T>(UnsafeCell<T>);

unsafe impl<T> Sync for SyncUnsafeCell<T> {}

impl<T> SyncUnsafeCell<T> {
    fn new(t: T) -> SyncUnsafeCell<T> {
        SyncUnsafeCell(UnsafeCell::new(t))
    }
}

impl<T> Deref for SyncUnsafeCell<T> {
    type Target = UnsafeCell<T>;
    fn deref(&self) -> &Self::Target {
        &self.0 
    }
}

impl<T> DerefMut for SyncUnsafeCell<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
