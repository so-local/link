//! A daemon's log: a service with no person and no [`Out`](crate::Out),
//! whose stderr its service manager keeps as its log file. It logs with
//! `tracing`'s macros (`info!`, `warn!`, `error!`), and [`logs`] gives
//! every line one shape: its time in UTC, its level and its words,
//! plain, never colored.
//!
//! ```text
//! 2026-10-25T00:47:11.508Z  WARN giving the held messages: closed
//! ```

pub use tracing::{debug, error, info, warn};

/// The crates a daemon serves or calls through, held at warnings
/// whatever `RUST_LOG` says: at `debug` they log a request's bytes,
/// credentials among them.
const HELD: &[&str] = &["hyper", "hyper_util", "h2", "reqwest", "axum"];

/// Logs this daemon's lines to stderr, from now on: at `info` and up,
/// or as `RUST_LOG` says, with [`HELD`] at warnings. Call once, at its
/// start.
pub fn logs() {
    logs_holding(&[]);
}

/// [`logs`], with the crates in `also` held at warnings too: a client a
/// daemon reaches its own services through that logs a request's bytes
/// below that, such as a database's. Call once, at its start.
pub fn logs_holding(also: &[&str]) {
    let asked = std::env::var("RUST_LOG").ok();
    let _ = tracing::subscriber::set_global_default(subscriber(std::io::stderr, asked, also));
}

/// The filter `asked` (`RUST_LOG`'s) makes: `info` when none or not
/// one, then [`HELD`] and `also` at warnings.
fn filter(asked: Option<String>, also: &[&str]) -> tracing_subscriber::EnvFilter {
    use tracing_subscriber::EnvFilter;
    let filter = asked
        .and_then(|asked| EnvFilter::try_new(asked).ok())
        .unwrap_or_else(|| EnvFilter::new("info"));
    HELD.iter().chain(also).fold(filter, |filter, held| {
        filter.add_directive(format!("{held}=warn").parse().expect("a held crate's name"))
    })
}

/// The subscriber [`logs_holding`] installs, writing to `to`, filtered as
/// `asked` says, with `also` held too.
fn subscriber<W>(
    to: W,
    asked: Option<String>,
    also: &[&str],
) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'w> tracing_subscriber::fmt::MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_env_filter(filter(asked, also))
        .with_writer(to)
        .with_ansi(false)
        .with_target(false)
        .finish()
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    /// A log written to memory, to read back.
    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<u8>>>);

    impl Write for Kept {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(bytes)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_log_line_is_its_time_level_and_words() {
        let kept = Kept::default();
        let to = kept.clone();
        tracing::subscriber::with_default(super::subscriber(move || to.clone(), None, &[]), || {
            super::warn!("giving the held messages: closed");
            super::debug!("not at info");
        });
        let log = String::from_utf8(kept.0.lock().unwrap().clone()).unwrap();
        let line = log.lines().next().unwrap();
        let (time, rest) = line.split_once(' ').unwrap();
        assert!(time.ends_with('Z') && time.contains('T'), "{line}");
        assert_eq!(rest.trim_start(), "WARN giving the held messages: closed");
        assert_eq!(log.lines().count(), 1, "{log}");
        assert!(!log.contains('\x1b'));
    }

    #[test]
    fn the_held_crates_log_warnings_alone_whatever_is_asked() {
        let kept = Kept::default();
        let to = kept.clone();
        let asked = Some("debug".to_string());
        let also = ["a_database_client"];
        tracing::subscriber::with_default(
            super::subscriber(move || to.clone(), asked, &also),
            || {
                super::debug!("asked for");
                tracing::info!(target: "hyper::proto", "a request's bytes");
                tracing::debug!(target: "a_database_client", "a query's bytes");
                tracing::warn!(target: "a_database_client", "closed");
            },
        );
        let log = String::from_utf8(kept.0.lock().unwrap().clone()).unwrap();
        assert!(log.contains("asked for") && log.contains("closed"), "{log}");
        assert!(!log.contains("bytes"), "{log}");
    }
}
