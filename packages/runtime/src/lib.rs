//! The one runtime of tokio's a program runs its async code on from
//! blocking code: made the first time it's asked for, and kept for the
//! process, so a command, or a call made again and again, never builds
//! one of its own.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Runs `future` to its end on this process's runtime, made (multi
/// threaded, with its timers and I/O) the first time it's asked for: its
/// answer, or its error, or the runtime's if none could be made. Not
/// from inside async code, which is on a runtime already.
pub fn block_on<T, E>(future: impl Future<Output = Result<T, E>>) -> anyhow::Result<T>
where
    E: Into<anyhow::Error>,
{
    runtime()?.block_on(future).map_err(Into::into)
}

/// This process's runtime, made the first time it's asked for. Two
/// threads asking at once may each make one; one is kept, the other
/// dropped before it runs anything.
fn runtime() -> std::io::Result<&'static Runtime> {
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let made = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    Ok(RUNTIME.get_or_init(|| made))
}

#[cfg(test)]
mod tests {
    /// What one call spawned is still running for the next, from another
    /// thread: the two ran on one runtime, never dropped between them.
    #[test]
    fn every_call_runs_on_one_runtime() {
        let (ask, asked) = tokio::sync::oneshot::channel::<u32>();
        let (answer, answered) = tokio::sync::oneshot::channel::<u32>();
        super::block_on(async {
            tokio::spawn(async move {
                if let Ok(n) = asked.await {
                    let _ = answer.send(n + 1);
                }
            });
            anyhow::Ok(())
        })
        .unwrap();
        let got = std::thread::spawn(move || {
            super::block_on(async move {
                ask.send(41).unwrap();
                answered.await
            })
            .unwrap()
        })
        .join()
        .unwrap();
        assert_eq!(got, 42);
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
