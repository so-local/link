//! What a proxy has carried, for `lnk sandbox show` and `lnk measure`.

use super::*;

/// A connection from the sandbox, counting what it reads (up) and what's
/// written to it (down) into a [`Call`]'s counters.
pub struct Counted<S> {
    inner: S,
    up: Arc<AtomicU64>,
    down: Arc<AtomicU64>,
}

impl<S> Counted<S> {
    pub fn new(inner: S, call: &Call) -> Self {
        Self {
            inner,
            up: call.up.clone(),
            down: call.down.clone(),
        }
    }
}

impl<S: crate::routes::Peer> crate::routes::Peer for Counted<S> {
    fn hangup_socket(&self) -> Option<std::os::fd::RawFd> {
        self.inner.hangup_socket()
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Counted<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        let read = buf.filled().len() - before;
        self.up.fetch_add(read as u64, Ordering::Relaxed);
        carried(0, read);
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Counted<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let poll = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &poll {
            self.down.fetch_add(*n as u64, Ordering::Relaxed);
            carried(1, *n);
        }
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// What this process's proxy has carried, over every call it let
/// through: up, from its sandbox, and down, to it.
pub(super) static CARRIED: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// Whether [`keep_traffic`]'s thread knows [`CARRIED`] moved: cleared
/// when it has written all and sleeps, and set by the first byte after,
/// which alone wakes it, so the bytes that follow cost no wake.
pub(super) static MOVED: AtomicBool = AtomicBool::new(false);

/// [`keep_traffic`]'s thread, woken when there is something to write;
/// otherwise it sleeps.
pub(super) static TRAFFIC_THREAD: std::sync::OnceLock<std::thread::Thread> =
    std::sync::OnceLock::new();

/// Adds `n` bytes to [`CARRIED`]'s `way`, and wakes [`keep_traffic`]'s
/// thread if they're the first since it wrote all.
pub(super) fn carried(way: usize, n: usize) {
    if n == 0 {
        return;
    }
    // Sequentially consistent with the thread's look before it sleeps,
    // so a byte that finds `MOVED` set is one that look sees.
    CARRIED[way].fetch_add(n as u64, Ordering::SeqCst);
    if !MOVED.load(Ordering::SeqCst)
        && !MOVED.swap(true, Ordering::SeqCst)
        && let Some(thread) = TRAFFIC_THREAD.get()
    {
        thread.unpark();
    }
}

/// Wakes [`keep_traffic`]'s thread, to see the proxy ended.
fn wake() {
    if let Some(thread) = TRAFFIC_THREAD.get() {
        thread.unpark();
    }
}

/// How often a proxy writes what it has carried ([`keep_traffic`]).
pub(super) const TRAFFIC_EVERY: Duration = Duration::from_secs(5);

/// What one proxy has carried, as `lnk sandbox show --json` prints
/// it: what the measure module asks.
pub use link_measure_contract::client::asked::Traffic;

/// This proxy's traffic file, once [`keep_traffic`] writes it: held while
/// it's written, and taken, file gone, when the proxy ends
/// ([`end_traffic`]).
pub(super) static TRAFFIC_FILE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Removes this proxy's traffic file, its program over, and writes it no
/// more: a file of a process gone is never read, and its number may come
/// back.
pub fn end_traffic() {
    let mut file = TRAFFIC_FILE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(path) = file.take() {
        let _ = std::fs::remove_file(path);
    }
    drop(file);
    // Its thread, asleep, ends.
    wake();
}

/// Where proxies keep what they carried, in `dir` ([`dir()`]): a file a
/// proxy, `<pid>.json` by its process, read only by [`traffic`] (`lnk
/// sandbox show --json`, which the measure module reads).
pub(super) fn traffic_dir(dir: &Path) -> PathBuf {
    link_sandbox_client::traffic_dir(dir)
}

/// Writes what this process's proxy carries for `wire`'s sandbox to
/// `traffic/<pid>.json` in its folder, every few seconds while it
/// changes, on a thread of its own, for [`traffic`] to read (`lnk
/// sandbox show`). What a model's answers count is the model
/// route's bridge's to say, to the measure module.
pub fn keep_traffic(wire: &Wire) {
    // Sweep snapshots left by a killed predecessor when their owner
    // next starts. Observers never remove another component's state.
    let _ = traffic(&wire.dir);
    let (dir, sandbox, pid) = (
        traffic_dir(&wire.dir),
        wire.sandbox.clone(),
        std::process::id(),
    );
    let path = dir.join(format!("{pid}.json"));
    *TRAFFIC_FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.clone());
    let started = link_measure_contract::client::started(pid);
    let spawned = std::thread::Builder::new()
        .name("traffic".into())
        .spawn(move || {
            let mut written = None;
            let mut at = std::time::Instant::now();
            loop {
                let now = (
                    CARRIED[0].load(Ordering::SeqCst),
                    CARRIED[1].load(Ordering::SeqCst),
                );
                // Bytes every few seconds, the first at once.
                let due = at.elapsed() >= TRAFFIC_EVERY || written.is_none();
                if due && written != Some(now) {
                    let traffic = Traffic {
                        pid,
                        sandbox: sandbox.clone(),
                        up: now.0,
                        down: now.1,
                        started,
                    };
                    let json = serde_json::to_vec(&traffic).unwrap_or_default();
                    let file = TRAFFIC_FILE.lock().unwrap_or_else(|e| e.into_inner());
                    // Ended: written no more.
                    if file.is_none() {
                        return;
                    }
                    if link_files::write_private(&path, &json).is_ok() {
                        written = Some(now);
                    }
                    // Tried: the next try, written or not, waits its turn.
                    at = std::time::Instant::now();
                    drop(file);
                }
                if TRAFFIC_FILE
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_none()
                {
                    return;
                }
                if written == Some(now) {
                    // All written: asleep until a byte moves ([`carried`]) or
                    // the proxy ends ([`end_traffic`]). Bytes from here on wake it, and those
                    // since the look are looked at again first.
                    MOVED.store(false, Ordering::SeqCst);
                    let carried = (
                        CARRIED[0].load(Ordering::SeqCst),
                        CARRIED[1].load(Ordering::SeqCst),
                    );
                    if carried == now {
                        std::thread::park();
                    }
                } else {
                    // Bytes not yet written wait out their 5 seconds, which
                    // more bytes don't cut short.
                    MOVED.store(true, Ordering::SeqCst);
                    std::thread::park_timeout(TRAFFIC_EVERY.saturating_sub(at.elapsed()));
                }
            }
        });
    let Ok(thread) = spawned else {
        return;
    };
    let _ = TRAFFIC_THREAD.set(thread.thread().clone());
    // Bytes carried before the thread was known wake it now.
    thread.thread().unpark();
}

/// What each running proxy has carried, by the files in `dir`'s
/// `traffic/`; those of proxies gone are deleted.
pub fn traffic(dir: &Path) -> Vec<Traffic> {
    let read = link_measure_contract::client::traffic::read(&traffic_dir(dir));
    // Cleanup remains with the publisher, never the measure reader.
    for path in read.stale {
        let _ = std::fs::remove_file(path);
    }
    read.traffic
}
