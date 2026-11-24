//! The one writer of an index ([`Writer`]): a thread of its own that does
//! every write, so no call waits for one.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, TryLockError};
use std::time::Duration;

use link_format::sentence;

use crate::Index;

/// How often the writer looks again, unwoken.
const TURN: Duration = Duration::from_secs(1);

/// What a writer writes: an index, or what holds one.
pub trait Writes: Send + 'static {
    /// The writer's turn, holding what it writes, its work's share of the
    /// machine taken from `pool`: whether more waits at once, so it takes
    /// another turn without waiting.
    fn work(&mut self, pool: &Pool) -> bool;
}

impl Writes for Index {
    fn work(&mut self, pool: &Pool) -> bool {
        self.notice_converter();
        if !self.pending()
            && !self.due()
            && !self.common().journal().news()
            && !self.common().words.behind()
        {
            return false;
        }
        let _turn = pool.take(self.opinions().threads);
        match self.turn() {
            Ok(more) => more,
            Err(err) => {
                crate::warn(sentence!("The index isn't refreshed."), &err);
                false
            }
        }
    }
}

/// The turns a service's writers take at once: as many as the most
/// threads any index it serves asks for
/// ([`crate::opinions::Opinions::threads`]), so a large corpus being
/// indexed leaves the machine usable.
pub struct Pool {
    /// Turns taken, and the most at once.
    taken: Mutex<(usize, usize)>,
    freed: Condvar,
}

/// A turn taken from a pool, given back when dropped.
pub struct Turn<'a>(&'a Pool);

impl Pool {
    /// A pool of one turn at a time, until an index asks for more.
    pub fn new() -> Arc<Pool> {
        Arc::new(Pool {
            taken: Mutex::new((0, 1)),
            freed: Condvar::new(),
        })
    }

    /// A turn, once fewer than the most are taken; `threads` raises the
    /// most.
    pub fn take(&self, threads: usize) -> Turn<'_> {
        let mut taken = self.taken.lock().unwrap_or_else(|e| e.into_inner());
        taken.1 = taken.1.max(threads);
        while taken.0 >= taken.1 {
            taken = self.freed.wait(taken).unwrap_or_else(|e| e.into_inner());
        }
        taken.0 += 1;
        Turn(self)
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.0.taken.lock().unwrap_or_else(|e| e.into_inner()).0 -= 1;
        self.0.freed.notify_one();
    }
}

/// What a writer's thread shares with its handle.
struct Inner<T> {
    held: Mutex<T>,
    woken: Mutex<bool>,
    wake: Condvar,
    stopped: AtomicBool,
    /// How many wait for the lock ([`Writer::lock`]).
    wanting: std::sync::atomic::AtomicUsize,
}

impl<T> Inner<T> {
    fn woken(&self) {
        *self.woken.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.wake.notify_all();
    }
}

/// An index's one writer: a thread taking turns ([`Writes::work`]) on
/// what it holds, between which anything else that writes it takes the
/// lock. Stopped and waited for when dropped.
pub struct Writer<T: Writes> {
    inner: Arc<Inner<T>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl<T: Writes> Writer<T> {
    /// `held`'s writer, started, its turns taken from `pool`.
    pub fn start(held: T, pool: Arc<Pool>) -> Self {
        let inner = Arc::new(Inner {
            held: Mutex::new(held),
            woken: Mutex::new(false),
            wake: Condvar::new(),
            stopped: AtomicBool::new(false),
            wanting: std::sync::atomic::AtomicUsize::new(0),
        });
        let turns = inner.clone();
        let thread = std::thread::spawn(move || {
            while !turns.stopped.load(Ordering::SeqCst) {
                // Another waiting for the lock takes it first: turns that
                // follow at once never shut it out.
                {
                    let woken = turns.woken.lock().unwrap_or_else(|e| e.into_inner());
                    let _ = turns.wake.wait_timeout_while(woken, TURN, |_| {
                        turns.wanting.load(Ordering::SeqCst) > 0
                            && !turns.stopped.load(Ordering::SeqCst)
                    });
                }
                let more = turns
                    .held
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .work(&pool);
                if more {
                    continue;
                }
                let woken = turns.woken.lock().unwrap_or_else(|e| e.into_inner());
                let (mut woken, _) = turns
                    .wake
                    .wait_timeout_while(woken, TURN, |woken| {
                        !*woken && !turns.stopped.load(Ordering::SeqCst)
                    })
                    .unwrap_or_else(|e| e.into_inner());
                *woken = false;
            }
        });
        Self {
            inner,
            thread: Some(thread),
        }
    }

    /// What it writes, once its turn ends: for a write of another's (a
    /// flush, a write-back), never a call's.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.inner.wanting.fetch_add(1, Ordering::SeqCst);
        let held = self.inner.held.lock().unwrap_or_else(|e| e.into_inner());
        self.inner.wanting.fetch_sub(1, Ordering::SeqCst);
        self.inner.wake.notify_all();
        held
    }

    /// Whether `busy` says what it writes is busy, or a turn holds it.
    pub fn busy(&self, busy: impl FnOnce(&T) -> bool) -> bool {
        match self.inner.held.try_lock() {
            Ok(held) => busy(&held),
            Err(TryLockError::Poisoned(held)) => busy(&held.into_inner()),
            Err(TryLockError::WouldBlock) => true,
        }
    }

    /// What it writes, unless a turn holds it now.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.inner.held.try_lock() {
            Ok(held) => Some(held),
            Err(TryLockError::Poisoned(held)) => Some(held.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    /// Its next turn now, not at the next of its period's.
    pub fn wake(&self) {
        self.inner.woken();
    }

    /// What wakes it, for its journal: nothing once it's gone.
    pub fn waker(&self) -> crate::index::journal::Waker {
        let inner = Arc::downgrade(&self.inner);
        Arc::new(move || {
            if let Some(inner) = inner.upgrade() {
                inner.woken();
            }
        })
    }
}

impl<T: Writes> Drop for Writer<T> {
    fn drop(&mut self) {
        self.inner.stopped.store(true, Ordering::SeqCst);
        self.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
