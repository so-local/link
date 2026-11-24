//! The private native gateway is owned with ACP, rather than detached by it.

use std::{
    io,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use link_secretive::process::OwnedChild;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    argv: Vec<String>,
    state_file: String,
    ready_state: String,
    poll_ms: u64,
    shutdown_seconds: u64,
    managed_env: String,
    managed_value: String,
}

static SETTINGS: std::sync::LazyLock<Settings> = std::sync::LazyLock::new(|| {
    toml::from_str(include_str!("gateway.toml")).expect("native gateway lifecycle settings")
});

/// The frontend may attach to this owner but must not create a detached replacement.
pub(crate) fn managed() -> (&'static str, &'static str) {
    (&SETTINGS.managed_env, &SETTINGS.managed_value)
}

#[derive(Deserialize)]
struct Status {
    pid: u32,
    gateway_state: String,
}

pub(crate) struct Gateway {
    command: Box<dyn Fn() -> anyhow::Result<Command> + Send + Sync>,
    home: PathBuf,
    wait: Duration,
    child: Mutex<Option<OwnedChild>>,
    stopping: AtomicBool,
}

impl Gateway {
    pub(crate) fn new(
        command: impl Fn() -> anyhow::Result<Command> + Send + Sync + 'static,
        home: PathBuf,
        wait: Duration,
    ) -> Self {
        Self {
            command: Box::new(command),
            home,
            wait,
            child: Mutex::new(None),
            stopping: AtomicBool::new(false),
        }
    }

    /// Native readiness must name the still-owned child, never an older or foreign owner.
    pub(crate) fn ensure(&self) -> io::Result<()> {
        let mut held = self.child.lock().unwrap_or_else(|e| e.into_inner());
        if self.stopping.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Hermes is stopping",
            ));
        }
        if held
            .as_ref()
            .is_some_and(|child| child.exited().unwrap_or(true))
        {
            held.take();
        }
        if held.is_none() {
            let mut command = (self.command)().map_err(|e| io::Error::other(format!("{e:#}")))?;
            command.args(&SETTINGS.argv).stdin(Stdio::null());
            *held = Some(OwnedChild::spawn(&mut command)?);
        }
        let child = held.as_mut().expect("the native gateway was started");
        let pid = child.child().id();
        let began = Instant::now();
        loop {
            if self.stopping.load(Ordering::SeqCst) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Hermes is stopping",
                ));
            }
            if child.exited()? {
                return Err(io::Error::other(
                    "Hermes's owned gateway ended before readiness",
                ));
            }
            let ready = std::fs::read(self.home.join(&SETTINGS.state_file))
                .ok()
                .and_then(|text| serde_json::from_slice::<Status>(&text).ok())
                .is_some_and(|status| {
                    status.pid == pid && status.gateway_state == SETTINGS.ready_state
                });
            if ready {
                return Ok(());
            }
            if began.elapsed() >= self.wait {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Hermes's owned gateway did not become ready",
                ));
            }
            poll();
        }
    }

    /// Called after ACP closes. Keep wait ownership through graceful stop and final group cleanup.
    pub(crate) fn stop(&self) -> io::Result<()> {
        self.stopping.store(true, Ordering::SeqCst);
        let Some(mut child) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return Ok(());
        };
        if !child.exited()? {
            // SAFETY: the unreaped direct child reserves this process identity.
            if unsafe { libc::kill(child.child().id() as i32, libc::SIGINT) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let began = Instant::now();
        while !child.exited()? && began.elapsed() < Duration::from_secs(SETTINGS.shutdown_seconds) {
            poll();
        }
        let clean = child.exited()?;
        let status = child.finish()?;
        if !clean {
            return Err(io::Error::other(
                "Hermes's owned gateway required forced cleanup",
            ));
        }
        if !status.success() {
            return Err(io::Error::other(format!(
                "Hermes's owned gateway ended with {status}"
            )));
        }
        Ok(())
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "polls native process readiness and graceful shutdown"
)]
fn poll() {
    std::thread::sleep(Duration::from_millis(SETTINGS.poll_ms));
}

#[cfg(test)]
mod tests;
