//! Native session operations share the capture barrier. A capture owns it
//! exclusively until its native calls, file operations and routing commit end.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{
    Mutex as AsyncMutex, MutexGuard, OwnedMutexGuard, OwnedRwLockReadGuard, OwnedRwLockWriteGuard,
    RwLock,
};

pub(crate) type Capture = Arc<OwnedRwLockWriteGuard<()>>;

pub(crate) struct Activity {
    access: Arc<RwLock<()>>,
    serial: Option<AsyncMutex<()>>,
    threads: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl Activity {
    pub fn new(single_session: bool) -> Self {
        Self {
            access: Arc::default(),
            serial: single_session.then(AsyncMutex::default),
            threads: Mutex::default(),
        }
    }

    /// An admitted session operation, including time waiting behind an
    /// earlier operation. A queued capture prevents later admissions.
    pub async fn enter<'a>(&'a self, thread: &'a str) -> SessionOperation<'a> {
        let reading = self.access.clone().read_owned().await;
        let serial = match &self.serial {
            Some(serial) => Some(serial.lock().await),
            None => None,
        };
        let lock = crate::harness::lock(&self.threads)
            .entry(thread.to_string())
            .or_default()
            .clone();
        // Construct before waiting, so cancellation removes an unused entry.
        let mut operation = SessionOperation {
            activity: self,
            thread,
            lock,
            held: None,
            _serial: serial,
            _reading: reading,
        };
        operation.held = Some(operation.lock.clone().lock_owned().await);
        operation
    }

    /// Wait for admitted operations, then exclude new ones through the
    /// complete capture. A blocking store retains a clone during its I/O.
    pub async fn capture(&self) -> anyhow::Result<Capture> {
        tokio::time::timeout(crate::harness::TURN_WAIT, self.access.clone().write_owned())
            .await
            .map(Arc::new)
            .map_err(|_| anyhow::anyhow!("a session operation is running: ask again once it ends"))
    }
}

pub(crate) struct SessionOperation<'a> {
    activity: &'a Activity,
    thread: &'a str,
    lock: Arc<AsyncMutex<()>>,
    held: Option<OwnedMutexGuard<()>>,
    _serial: Option<MutexGuard<'a, ()>>,
    _reading: OwnedRwLockReadGuard<()>,
}

impl Drop for SessionOperation<'_> {
    fn drop(&mut self) {
        let mut threads = crate::harness::lock(&self.activity.threads);
        // The table and this operation, plus its acquired lock when present.
        // Every other waiter also owns a reference under this same table lock.
        let own = 2 + usize::from(self.held.is_some());
        if Arc::strong_count(&self.lock) == own {
            threads.remove(self.thread);
        }
    }
}

#[cfg(test)]
mod tests;
