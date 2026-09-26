use super::ids::JobId;
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use tokio_util::sync::CancellationToken;

/// A registry of currently-running jobs and their cancellation tokens.
/// Cloneable; the runtime keeps one and the dispatcher keeps another.
#[derive(Clone, Default)]
pub struct CancellationRegistry {
    /// Token map paired with a condvar so waiters (graceful shutdown) are
    /// woken on every completion instead of polling with sleeps.
    inner: Arc<(Mutex<HashMap<JobId, CancellationToken>>, Condvar)>,
}

impl CancellationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new token under `id`. Returns the token (so the caller
    /// can pass it into the spawned task).
    pub fn register(&self, id: JobId) -> CancellationToken {
        let token = CancellationToken::new();
        if let Ok(mut g) = self.inner.0.lock() {
            g.insert(id, token.clone());
        }
        token
    }

    /// Cancel and remove the token for `id`. No-op if unknown.
    pub fn cancel(&self, id: JobId) -> bool {
        let token = self.inner.0.lock().ok().and_then(|mut g| g.remove(&id));
        self.inner.1.notify_all();
        match token {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    /// Drop the token for `id` (job has finished naturally).
    pub fn complete(&self, id: JobId) {
        if let Ok(mut g) = self.inner.0.lock() {
            g.remove(&id);
        }
        self.inner.1.notify_all();
    }

    /// Request cancellation for every in-flight job while retaining registry
    /// entries until their exactly-once terminal result confirms completion.
    pub fn request_cancel_all(&self) {
        if let Ok(g) = self.inner.0.lock() {
            for token in g.values() {
                token.cancel();
            }
        }
    }

    /// Force-clear every job token after a bounded graceful wait has expired.
    pub fn cancel_all(&self) {
        if let Ok(mut g) = self.inner.0.lock() {
            for (_, token) in g.drain() {
                token.cancel();
            }
        }
        self.inner.1.notify_all();
    }

    /// Blocks until the registry is empty (all in-flight jobs completed) or
    /// `timeout` elapses. Wakes on each completion via condvar rather than
    /// polling with sleeps. Returns `true` when the registry drained cleanly.
    pub fn wait_until_empty(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let (lock, cvar) = &*self.inner;
        let mut guard = match lock.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        while !guard.is_empty() {
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next_guard, wait_result) = match cvar.wait_timeout(guard, remaining) {
                Ok(result) => result,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard = next_guard;
            if wait_result.timed_out() {
                return guard.is_empty();
            }
        }
        true
    }

    /// How many jobs are currently registered.
    pub fn len(&self) -> usize {
        self.inner.0.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
