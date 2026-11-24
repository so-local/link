//! Foreground stop delivery keeps the owner alive through child cleanup.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering::SeqCst};

static CHILD: AtomicI32 = AtomicI32::new(0);
/// A stop sent before there was a child to pass it to.
static PENDING: AtomicI32 = AtomicI32::new(0);
static STOPPED: AtomicBool = AtomicBool::new(false);

extern "C" fn pass(signal: libc::c_int) {
    STOPPED.store(true, SeqCst);
    // The foreground process group already receives Ctrl-C. Pass a
    // managed stop to this child's wrapper too, including when that
    // child starts after the interrupt. Keep this log reader alive.
    let signal = if signal == libc::SIGINT {
        libc::SIGTERM
    } else {
        signal
    };
    match CHILD.load(SeqCst) {
        // SAFETY: kill is async-signal-safe.
        child if child > 0 => unsafe {
            libc::kill(child, signal);
        },
        _ => PENDING.store(signal, SeqCst),
    }
}

/// Catches them from now: true once one came.
pub fn catch() -> &'static AtomicBool {
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: `pass` only calls kill and stores atomics.
        unsafe { libc::signal(signal, pass as *const () as libc::sighandler_t) };
    }
    &STOPPED
}

/// Passes them to `child` from now, and one that came before; with
/// none (an idle agent's child stopped), keeps one that comes for the
/// next.
pub fn pass_to(child: Option<u32>) {
    let Some(child) = child else {
        CHILD.store(0, SeqCst);
        return;
    };
    CHILD.store(child as i32, SeqCst);
    let pending = PENDING.swap(0, SeqCst);
    if pending > 0 {
        // SAFETY: a signal to the child just started.
        unsafe { libc::kill(child as i32, pending) };
    }
}
