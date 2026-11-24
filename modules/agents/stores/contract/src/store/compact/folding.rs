//! A compact conversation's fold, on its files: the tail's events written
//! as the next frame, then let go of. The append that brings the tail to
//! a frame's worth hands its fold to a thread of its own ([`fold_later`]),
//! which compresses with no lock held, so neither the turn's next append
//! nor a read waits out zstd's level ([`fold_aside`]). Only the append
//! that finds a tail whose folds were lost folds it itself ([`fold_now`]).

use std::io::{Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use anyhow::Context;
use link_files::nofollow::{lock_fd, read_open};
use serde_json::Value;

use super::{FRAMES, TAIL, TAIL_MAX, decode, due, encode, overdue, spans};

/// The most `events.zst` is read to: far past any conversation.
const FRAMES_MAX: u64 = 1 << 40;

/// How long a program waits for its folds before it ends ([`settle`]):
/// past it, a fold still running is said, and its tail is folded by its
/// next append.
pub const SETTLE_MAX: Duration = Duration::from_secs(30);

/// A compact conversation's folder, as its store reaches it: each of its
/// files opened by name, never through a link.
pub trait Folder: Send + 'static {
    /// The regular file `name` here, opened with `flags` (and `mode`,
    /// made with `O_CREAT`).
    fn open(&self, name: &str, flags: libc::c_int, mode: u32) -> std::io::Result<std::fs::File>;

    /// This folder again, for a fold handed on to a thread of its own.
    fn duplicate(&self) -> std::io::Result<Self>
    where
        Self: Sized;
}

/// `events.zst` in `folder`, whole: none when it isn't there.
fn frames(folder: &impl Folder) -> std::io::Result<Vec<u8>> {
    match folder.open(FRAMES, libc::O_RDONLY, 0) {
        Ok(file) => read_open(file, FRAMES_MAX),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

/// The frames of `folder`, and where the whole ones end: one cut short at
/// the end (a writer stopped while writing it) isn't one.
fn whole_frames(folder: &impl Folder) -> anyhow::Result<(Vec<u8>, usize)> {
    let frames = frames(folder).context("reading the conversation's frames")?;
    let whole = spans(&frames).last().map_or(0, |span| span.end);
    Ok((frames, whole))
}

/// The number of the last event in `folder`'s whole frames: 0 for none.
/// For an append that finds the tail empty.
pub fn last_framed(folder: &impl Folder) -> anyhow::Result<u64> {
    let (frames, whole) = whole_frames(folder)?;
    last_in(&frames[..whole])
}

/// The number of the last event in `frames`, whole frames: 0 for none.
fn last_in(frames: &[u8]) -> anyhow::Result<u64> {
    let Some(span) = spans(frames).into_iter().rev().find(|span| {
        // The frame naming the thread holds none.
        decode(&frames[span.clone()]).is_ok_and(|lines| !lines.is_empty())
    }) else {
        return Ok(0);
    };
    let lines = decode(&frames[span])?;
    let last = lines[..lines.len() - 1]
        .rsplit(|b| *b == b'\n')
        .next()
        .unwrap_or_default();
    let event: Value = serde_json::from_slice(last).context("reading an event")?;
    super::super::seq_of(&event).context("an event without its number")
}

/// The tail, `tail` locked, read from its start.
fn whole_tail(tail: &mut std::fs::File) -> std::io::Result<Vec<u8>> {
    tail.seek(std::io::SeekFrom::Start(0))?;
    let mut lines = Vec::new();
    Read::by_ref(tail).take(TAIL_MAX).read_to_end(&mut lines)?;
    Ok(lines)
}

/// The tail's events and bytes.
fn measured(lines: &[u8]) -> (usize, u64) {
    let count = lines.iter().filter(|b| **b == b'\n').count();
    (count, lines.len() as u64)
}

/// What a fold frames of the tail's `lines`, the frames' last event being
/// `last`.
struct Framing {
    /// Each whole line numbered past `last`: one in a frame already, a
    /// writer having stopped before it emptied the tail, isn't framed
    /// again.
    lines: Vec<u8>,
    /// Where the tail's whole lines end.
    end: usize,
    /// The number of the last event framed then: `last` for none.
    last: u64,
}

fn framing(lines: &[u8], last: u64) -> anyhow::Result<Framing> {
    let mut framing = Framing {
        lines: Vec::with_capacity(lines.len()),
        end: 0,
        last,
    };
    for line in lines.split_inclusive(|b| *b == b'\n') {
        if line.last() != Some(&b'\n') {
            break;
        }
        framing.end += line.len();
        let event: Value = serde_json::from_slice(line).context("reading an event")?;
        if let Some(seq) = super::super::seq_of(&event).filter(|seq| *seq > framing.last) {
            framing.lines.extend_from_slice(line);
            framing.last = seq;
        }
    }
    Ok(framing)
}

/// After an append wrote its event to `tail`, still locked: the tail
/// folded once it's due, handed on ([`fold_later`]), or by this append
/// once its folds were lost ([`overdue`]). The event is written whatever
/// becomes of the fold, so a fold that fails is said, never the append's
/// failure, and leaves the tail whole for the next append.
pub fn appended(folder: &impl Folder, tail: &mut std::fs::File) {
    let folded = (|| -> anyhow::Result<()> {
        let lines = whole_tail(tail).context("reading the conversation's tail")?;
        let (count, size) = measured(&lines);
        if overdue(count, size) {
            fold_now(folder, tail, &lines)?;
        } else if due(count, size) {
            let handed = folder.duplicate()?;
            fold_later(tail, move |hand| fold_aside(&handed, hand))?;
        }
        Ok(())
    })();
    if let Err(err) = folded {
        crate::warned(
            link_format::sentence!("Couldn't fold a conversation's events into a frame."),
            &err,
        );
    }
}

/// The tail's `lines`, `tail` locked for writing, as the next frame,
/// compressed while the lock's held, then the tail emptied: for a tail
/// whose folds were lost.
fn fold_now(folder: &impl Folder, tail: &std::fs::File, lines: &[u8]) -> anyhow::Result<()> {
    let (frames, whole) = whole_frames(folder)?;
    let framed = framing(lines, last_in(&frames[..whole])?)?;
    let encoded = match framed.lines.is_empty() {
        true => Vec::new(),
        false => vec![encode(&framed.lines)?],
    };
    write_frames(folder, tail, &frames, whole, &encoded)
}

/// The tail of `folder` folded into a frame if it's still due: the fold
/// the append that filled it handed on. Its lines are read under the
/// tail's lock as a reader's and compressed with none held, so appends
/// and reads go on meanwhile; then, under the lock as a writer's, the
/// frame is written only if the tail still starts with those lines and
/// no other fold wrote a frame since. What was appended meanwhile, a few
/// events, is framed after it while the lock's held, so the tail is
/// emptied only once each of its events is in a frame: cutting the
/// folded lines off its front would leave a moment a writer stopped in
/// could lose the rest.
fn fold_aside(folder: &impl Folder, handed: Handed) -> anyhow::Result<()> {
    let mut tail = match folder.open(TAIL, libc::O_RDWR, 0) {
        Ok(tail) => tail,
        // Gone with its conversation, moved or deleted meanwhile.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).context("opening the conversation's tail"),
    };
    let fd = tail.as_raw_fd();
    lock_fd(fd, libc::LOCK_SH)?;
    let lines = whole_tail(&mut tail)?;
    let (count, size) = measured(&lines);
    if !due(count, size) {
        // Done while the lock's held ([`Handed`]).
        drop(handed);
        return Ok(());
    }
    let (frames, whole) = whole_frames(folder)?;
    let framed = framing(&lines, last_in(&frames[..whole])?)?;
    lock_fd(fd, libc::LOCK_UN)?;
    let mut written = Vec::new();
    if !framed.lines.is_empty() {
        written.push(encode(&framed.lines)?);
    }
    lock_fd(fd, libc::LOCK_EX)?;
    let now = whole_tail(&mut tail)?;
    // Frames are only added to, or cut short at the end: their size, the
    // same, says they are.
    let frames_now = match folder.open(FRAMES, libc::O_RDONLY, 0) {
        Ok(file) => file.metadata()?.len(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0,
        Err(err) => return Err(err).context("reading the conversation's frames"),
    };
    let same = now.starts_with(&lines[..framed.end]) && frames_now == frames.len() as u64;
    let folded = match same {
        // Folded by another program meanwhile, or written to by hand:
        // left for the next append.
        false => Ok(()),
        true => (|| {
            let rest = framing(&now[framed.end..], framed.last)?;
            if !rest.lines.is_empty() {
                written.push(encode(&rest.lines)?);
            }
            write_frames(folder, &tail, &frames, whole, &written)
        })(),
    };
    // Done while the lock's held ([`Handed`]).
    drop(handed);
    folded
}

/// `written`, frames, put after `frames`' `whole` ones (a frame cut short
/// at the end cut off first) and kept on the disk, then the tail, locked
/// for writing, emptied.
fn write_frames(
    folder: &impl Folder,
    tail: &std::fs::File,
    frames: &[u8],
    whole: usize,
    written: &[Vec<u8>],
) -> anyhow::Result<()> {
    let writing = "writing the conversation's frames";
    let mut file = folder
        .open(FRAMES, libc::O_WRONLY | libc::O_CREAT, 0o600)
        .context(writing)?;
    if whole < frames.len() {
        file.set_len(whole as u64).context(writing)?;
    }
    if !written.is_empty() {
        file.seek(std::io::SeekFrom::Start(whole as u64))
            .context(writing)?;
        for frame in written {
            file.write_all(frame).context(writing)?;
        }
        // Kept before the tail lets its events go.
        file.sync_data().context(writing)?;
    }
    tail.set_len(0).context("emptying the conversation's tail")
}

/// The folds handed on and not yet done, each by its tail's device and
/// inode, which stay the same however the tail is emptied, and the
/// signal each one done gives.
static FOLDING: std::sync::LazyLock<(std::sync::Mutex<Tails>, std::sync::Condvar)> =
    std::sync::LazyLock::new(Default::default);

/// Tails, each by its device and inode.
type Tails = std::collections::HashSet<(u64, u64)>;

/// A fold handed on, until it's dropped: the fold drops it while it
/// still holds the tail's lock, so an append that finds the tail due
/// after it hands the next one on.
pub struct Handed {
    file: (u64, u64),
}

impl Drop for Handed {
    fn drop(&mut self) {
        let (folding, done) = &*FOLDING;
        folding
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.file);
        done.notify_all();
    }
}

/// `fold` run on a thread of its own, unless one handed on for the same
/// tail (`tail`'s file, locked by the caller) is waiting or running: the
/// append that filled the tail returns meanwhile. A fold that fails is
/// said, and leaves the tail whole for the next; one whose thread can't
/// start is an error, which [`appended`] says.
fn fold_later(
    tail: &std::fs::File,
    fold: impl FnOnce(Handed) -> anyhow::Result<()> + Send + 'static,
) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = tail.metadata()?;
    let key = (meta.dev(), meta.ino());
    if !FOLDING
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key)
    {
        return Ok(());
    }
    let handed = Handed { file: key };
    let run = move || {
        if let Err(err) = fold(handed) {
            crate::warned(
                link_format::sentence!("Couldn't fold a conversation's events into a frame."),
                &err,
            );
        }
    };
    #[cfg(any(test, feature = "test-support"))]
    if testing::refused(key) {
        // Dropped, and the fold handed on with it.
        drop(run);
        anyhow::bail!("a test refused the fold its thread");
    }
    // Not started, it's dropped, and the fold with it.
    std::thread::Builder::new()
        .name("fold".into())
        .spawn(run)
        .context("starting a thread to fold the conversation's tail")?;
    Ok(())
}

/// Every fold [`fold_later`] handed on done, waited for at most `within`:
/// whether they are.
pub fn settled_within(within: Duration) -> bool {
    let until = Instant::now() + within;
    let (folding, done) = &*FOLDING;
    let mut held = folding.lock().unwrap_or_else(|e| e.into_inner());
    while !held.is_empty() {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        held = done
            .wait_timeout(held, left)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
    true
}

/// Every fold handed on, done: a program waits for them before it ends,
/// so none is lost, for at most [`SETTLE_MAX`]. A fold still running then
/// is said; its tail stays whole, and its next append folds it.
pub fn settle() {
    if !settled_within(SETTLE_MAX) {
        crate::warned(
            link_format::sentence!("Stopped waiting for a conversation's fold."),
            &anyhow::anyhow!(
                "it ran past {} seconds; the conversation's next append folds its tail",
                SETTLE_MAX.as_secs()
            ),
        );
    }
}

/// What a test makes of a fold: one held midway through compressing, and
/// one whose thread is refused.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    use std::sync::{Arc, Condvar, Mutex};

    /// Each compression held, by the bytes whose presence in its lines
    /// holds it, and whether one entered and whether it's let go.
    type Holds = Vec<(Vec<u8>, Arc<(Mutex<(bool, bool)>, Condvar)>)>;

    static HOLDS: Mutex<Holds> = Mutex::new(Vec::new());

    /// The tails whose folds' threads are refused, by device and inode.
    static REFUSED: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

    /// Compressions of lines holding `marker`, held midway until it's
    /// dropped: a fold made slow, so a test sees what waits for it.
    pub struct Hold {
        state: Arc<(Mutex<(bool, bool)>, Condvar)>,
    }

    impl Hold {
        /// Each compression of lines holding `marker` held from now.
        pub fn compressing(marker: &[u8]) -> Self {
            let state = Arc::new((Mutex::new((false, false)), Condvar::new()));
            HOLDS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((marker.to_vec(), state.clone()));
            Self { state }
        }

        /// Whether a compression it holds has started.
        pub fn entered(&self) -> bool {
            self.state.0.lock().unwrap_or_else(|e| e.into_inner()).0
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            HOLDS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(_, state)| !Arc::ptr_eq(state, &self.state));
            self.state.0.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
            self.state.1.notify_all();
        }
    }

    /// A compression of `lines`, held while a [`Hold`] of a marker they
    /// hold lives.
    pub(in crate::store::compact) fn held(lines: &[u8]) {
        let state = HOLDS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(marker, _)| lines.windows(marker.len()).any(|w| w == marker.as_slice()))
            .map(|(_, state)| state.clone());
        let Some(state) = state else {
            return;
        };
        let (flags, signal) = &*state;
        let mut flags = flags.lock().unwrap_or_else(|e| e.into_inner());
        flags.0 = true;
        // Let go when the test's hold drops, whatever becomes of the test.
        while !flags.1 {
            flags = signal.wait(flags).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// The folds of the tail at `tail` refused their thread until it's
    /// dropped, as a thread that can't start refuses one.
    pub struct Refusal((u64, u64));

    impl Refusal {
        pub fn of(tail: &std::path::Path) -> std::io::Result<Self> {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::metadata(tail)?;
            let key = (meta.dev(), meta.ino());
            REFUSED.lock().unwrap_or_else(|e| e.into_inner()).push(key);
            Ok(Self(key))
        }
    }

    impl Drop for Refusal {
        fn drop(&mut self) {
            REFUSED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|key| *key != self.0);
        }
    }

    pub(super) fn refused(key: (u64, u64)) -> bool {
        REFUSED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&key)
    }
}
