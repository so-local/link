//! Ctrl-C during a counted flow: the step it stopped at, a Next to start
//! again, exit 130 and no record. A flow arms it at each step; a command
//! that has finished, or lends the terminal to another program, disarms
//! it, and Ctrl-C is what it was before: the system's, or the program's
//! own. A Ctrl-C the process was started ignoring (a background job's) stays
//! ignored.

use std::os::fd::IntoRawFd;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, Once};

/// What to say if Ctrl-C comes now: none outside a flow.
static SAYING: Mutex<Option<String>> = Mutex::new(None);
/// The write end of the pipe the signal handler wakes the watcher through.
static WAKE: AtomicI32 = AtomicI32::new(-1);
static INSTALLED: Once = Once::new();
/// What Ctrl-C did before this installed its own.
static OLD: Mutex<Option<libc::sigaction>> = Mutex::new(None);

/// Set when Ctrl-C was ignored at the start: nothing is installed.
static IGNORED: AtomicBool = AtomicBool::new(false);

/// Ctrl-C now writes `text` to stderr and ends the process with 130.
pub(crate) fn arm(text: String) {
    INSTALLED.call_once(install);
    if IGNORED.load(Ordering::Relaxed) {
        return;
    }
    *SAYING.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
}

/// Ctrl-C ends the process as the system does.
pub(crate) fn disarm() {
    *SAYING.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

extern "C" fn interrupted(_: libc::c_int) {
    let byte = 1u8;
    // SAFETY: write is async-signal-safe, and the byte is on the stack.
    unsafe { libc::write(WAKE.load(Ordering::Relaxed), (&raw const byte).cast(), 1) };
}

fn install() {
    // SAFETY: sigaction with no new action only reads the current one.
    let old = unsafe {
        let mut old: libc::sigaction = std::mem::zeroed();
        libc::sigaction(libc::SIGINT, std::ptr::null(), &mut old);
        old
    };
    if old.sa_sigaction == libc::SIG_IGN {
        // The process was started with Ctrl-C ignored: it stays so.
        IGNORED.store(true, Ordering::Relaxed);
        return;
    }
    // Made close-on-exec as it's made: a child another thread starts
    // between a plain pipe and its fcntl would hold an end for good.
    let Ok((read, write)) = std::io::pipe() else {
        return;
    };
    let ends = [read.into_raw_fd(), write.into_raw_fd()];
    // SAFETY: as above; the handler never blocks on a full pipe.
    unsafe { libc::fcntl(ends[1], libc::F_SETFL, libc::O_NONBLOCK) };
    WAKE.store(ends[1], Ordering::Relaxed);
    let watcher = std::thread::Builder::new()
        .name("lnk-ctrl-c".into())
        .spawn(move || watch(ends[0]));
    if watcher.is_err() {
        return;
    }
    // SAFETY: the handler only writes a byte to the pipe.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = interrupted as extern "C" fn(libc::c_int) as usize;
        action.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
    }
    *OLD.lock().unwrap_or_else(|e| e.into_inner()) = Some(old);
}

/// Waits for the handler's byte, then ends the process.
fn watch(read: libc::c_int) {
    let mut byte = 0u8;
    loop {
        // SAFETY: read fills the one byte it is given.
        let n = unsafe { libc::read(read, (&raw mut byte).cast(), 1) };
        if n > 0 {
            break;
        }
        if n == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return;
        }
    }
    let text = SAYING.lock().unwrap_or_else(|e| e.into_inner()).take();
    match text {
        Some(text) => {
            let mut bytes = text.as_bytes();
            while !bytes.is_empty() {
                // SAFETY: write reads the bytes it is given.
                let n =
                    unsafe { libc::write(libc::STDERR_FILENO, bytes.as_ptr().cast(), bytes.len()) };
                match usize::try_from(n) {
                    Ok(n) if n > 0 => bytes = &bytes[n..],
                    _ => break,
                }
            }
            std::process::exit(i32::from(crate::Exit::Interrupted.code()));
        }
        None => {
            // SAFETY: back to what it was, then the signal again.
            unsafe {
                match OLD.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                    Some(old) => libc::sigaction(libc::SIGINT, old, std::ptr::null_mut()),
                    None => libc::signal(libc::SIGINT, libc::SIG_DFL) as libc::c_int,
                };
                libc::raise(libc::SIGINT);
            }
        }
    }
}
