//! A long step's heartbeat: while a harness's installer or package
//! manager works quietly for minutes, Link says it's still going.

use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    every_seconds: u64,
}

static SETTINGS: std::sync::LazyLock<Settings> = std::sync::LazyLock::new(|| {
    toml::from_str(include_str!("progress.toml")).expect("the harness progress.toml")
});

/// While held, a note every [`Settings::every_seconds`] on stderr saying
/// `what` is still going and for how long; dropping it stops the notes.
/// It writes beside the step's own output, so it never waits on it.
pub struct Heartbeat {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Heartbeat {
    /// Notes about `what`, a phrase such as "installing Hermes".
    pub fn start(what: &str) -> Self {
        Self::every(what, Duration::from_secs(SETTINGS.every_seconds), |line| {
            link_format::aside(link_format::Note(link_format::sentence!("{}.", line)));
        })
    }

    fn every(what: &str, period: Duration, say: impl Fn(String) + Send + 'static) -> Self {
        let (stop, stopped) = mpsc::channel::<()>();
        let what = what.to_owned();
        let started = Instant::now();
        let thread = std::thread::spawn(move || {
            while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(period) {
                say(still(&what, started.elapsed()));
            }
        });
        Self {
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The note: "Still installing Hermes, 2 min 30 s so far."
fn still(what: &str, elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let so_far = match (seconds / 60, seconds % 60) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} min"),
        (m, s) => format!("{m} min {s} s"),
    };
    format!("Still {what}, {so_far} so far")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_says_what_and_for_how_long() {
        assert_eq!(
            still("installing Hermes", Duration::from_secs(150)),
            "Still installing Hermes, 2 min 30 s so far"
        );
        assert_eq!(
            still("installing Hermes", Duration::from_secs(120)),
            "Still installing Hermes, 2 min so far"
        );
        assert_eq!(
            still("installing Hermes", Duration::from_secs(30)),
            "Still installing Hermes, 30 s so far"
        );
    }

    #[test]
    fn it_beats_while_held_and_stops_when_dropped() {
        let (said, heard) = mpsc::channel();
        let beat = Heartbeat::every("installing X", Duration::from_millis(1), move |line| {
            let _ = said.send(line);
        });
        let first = heard
            .recv_timeout(Duration::from_secs(10))
            .expect("a note while the step runs");
        assert!(first.starts_with("Still installing X, "), "{first}");
        drop(beat);
        // Dropped, the thread has ended: its sender is gone with it.
        while heard.try_recv().is_ok() {}
        assert_eq!(heard.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }

    #[test]
    fn its_period_leaves_a_watcher_room() {
        assert!((1..=60).contains(&SETTINGS.every_seconds));
    }
}
