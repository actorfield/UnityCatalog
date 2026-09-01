//! An in-memory `ObjectLog`.
//!
//! Public rather than test-gated: it is the reference implementation of the
//! contract — `put_if_absent` never overwrites, `list_after` is exclusive and
//! ordered — and it lets callers outside this crate exercise the log store
//! without an object store. Also usable for a single-process dev mode, where
//! nothing needs to outlive the process.

use super::log::{ObjectLog, PutResult};
use std::collections::BTreeMap;
use std::sync::Mutex;
use uc_errors::UcError;

/// A poisoned lock is recovered rather than propagated. Poisoning means another
/// thread panicked while holding it; re-panicking here would turn one failure
/// into a permanently unusable store, and this map has no invariant that a
/// partial write could break.
#[derive(Default)]
pub struct MemoryLog {
    // BTreeMap so `list_after` returns keys in the lexicographic order the log
    // protocol depends on, without sorting at every call.
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl MemoryLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored objects. For tests that assert on log growth.
    pub fn len(&self) -> usize {
        self.objects.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove an object, to simulate a hole or a pruned commit.
    pub fn remove(&self, key: &str) -> Option<Vec<u8>> {
        self.objects.lock().unwrap_or_else(|e| e.into_inner()).remove(key)
    }
}

#[async_trait::async_trait]
impl ObjectLog for MemoryLog {
    async fn put_if_absent(&self, key: &str, body: Vec<u8>) -> Result<PutResult, UcError> {
        let mut o = self.objects.lock().unwrap_or_else(|e| e.into_inner());
        if o.contains_key(key) {
            return Ok(PutResult::AlreadyExists);
        }
        o.insert(key.to_string(), body);
        Ok(PutResult::Created)
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, UcError> {
        Ok(self.objects.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned())
    }

    async fn list_after(&self, prefix: &str, start_after: &str) -> Result<Vec<String>, UcError> {
        Ok(self
            .objects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .filter(|k| k.starts_with(prefix) && k.as_str() > start_after)
            .cloned()
            .collect())
    }

    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), UcError> {
        self.objects.lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), body);
        Ok(())
    }
}
