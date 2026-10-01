//! One async mutex per key, created on demand and forgotten when the last
//! holder leaves.
//!
//! Two request paths need "at most one of these per account" or "per
//! (account, asset)": the importer, so same-account imports do not wipe each
//! other's staging rows, and multipart upload completion, so two clients
//! cannot race `store_verified` on one SHA-256. A plain map of `Arc<Mutex<()>>`
//! does the job but never shrinks: one entry per asset ever completed, for the
//! life of the process. [`KeyedLocks`] shrinks it again. The guard removes its
//! key on drop unless another task is already waiting on the same mutex.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::{Mutex, OwnedMutexGuard};

type LockMap = HashMap<String, Arc<Mutex<()>>>;

/// Per-key async mutexes. Cloning shares the same map.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyedLocks {
    /// A `std` mutex: it is only ever held for a map lookup, never across an
    /// `.await`, and `Drop` cannot await.
    map: Arc<StdMutex<LockMap>>,
}

/// Holds the key's mutex until dropped.
#[derive(Debug)]
pub(crate) struct KeyedLockGuard {
    guard: OwnedMutexGuard<()>,
    key: String,
    locks: KeyedLocks,
}

impl KeyedLocks {
    /// Wait for and take the mutex for `key`.
    pub(crate) async fn lock(&self, key: String) -> KeyedLockGuard {
        let mutex = self
            .map
            .lock()
            .expect("keyed lock map poisoned")
            .entry(key.clone())
            .or_default()
            .clone();
        let guard = mutex.lock_owned().await;
        KeyedLockGuard {
            guard,
            key,
            locks: self.clone(),
        }
    }

    /// Number of keys currently known. Tests use it to show the map shrinks.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map.lock().expect("keyed lock map poisoned").len()
    }
}

impl Drop for KeyedLockGuard {
    fn drop(&mut self) {
        let mut map = self.locks.map.lock().expect("keyed lock map poisoned");
        // Two holders is the map's entry plus this guard. A task waiting on
        // the same key cloned the `Arc` before calling `lock_owned`, so it
        // shows as a third and the entry stays for it.
        if Arc::strong_count(OwnedMutexGuard::mutex(&self.guard)) == 2 {
            map.remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn entry_is_removed_when_the_last_holder_leaves() {
        let locks = KeyedLocks::default();
        {
            let _guard = locks.lock("a:1".into()).await;
            assert_eq!(locks.len(), 1);
        }
        assert_eq!(locks.len(), 0);
    }

    #[tokio::test]
    async fn waiters_serialize_and_the_entry_survives_until_the_last_one() {
        let locks = KeyedLocks::default();
        let first = locks.lock("a:1".into()).await;
        let waiter = {
            let locks = locks.clone();
            tokio::spawn(async move {
                let _guard = locks.lock("a:1".into()).await;
                locks.len()
            })
        };
        // Give the waiter time to queue on the mutex.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(locks.len(), 1);
        drop(first);
        // The waiter held the entry, so it was still there when it ran.
        assert_eq!(waiter.await.unwrap(), 1);
        assert_eq!(locks.len(), 0);
    }

    #[tokio::test]
    async fn different_keys_do_not_block_each_other() {
        let locks = KeyedLocks::default();
        let _a = locks.lock("a".into()).await;
        let _b = tokio::time::timeout(Duration::from_millis(100), locks.lock("b".into()))
            .await
            .expect("lock on a different key must not wait");
        assert_eq!(locks.len(), 2);
    }
}
