//! Calls applied in a sandbox ([`Sandboxed`]): one for a session's
//! calls, not one each.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, bail};
use serde_json::{Value, json};

/// The most of its sandbox's errors a call that didn't run says.
const ERRORS_MAX: usize = 16 << 10;

/// Calls applied by `apply` (a program and its arguments, its
/// sandbox's): started at the first call and kept for the next, each call
/// a line on its input and its answer a line on its output, so a session
/// starts its sandbox once, not once a call. One that ends, or answers
/// out of turn, is started again at the next call.
pub struct Sandboxed<'a> {
    apply: &'a [String],
    running: RefCell<Option<Running>>,
}

/// `apply` started: its input, its output, and what it said on stderr,
/// read as it goes so it never fills.
struct Running {
    child: Child,
    input: Option<ChildStdin>,
    output: Option<BufReader<ChildStdout>>,
    errors: Arc<Mutex<String>>,
    reading: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Running {
    /// Its input closed, `apply` ends.
    fn drop(&mut self) {
        drop(self.input.take());
        let _ = self.child.wait();
    }
}

impl<'a> Sandboxed<'a> {
    pub fn new(apply: &'a [String]) -> Self {
        Self {
            apply,
            running: RefCell::new(None),
        }
    }

    /// Call `name`, the call on its input: its answer, or what it refused.
    pub fn call(&self, name: &str, arguments: &Value) -> anyhow::Result<Value> {
        self.call_expecting(name, arguments, &crate::Expect::default())
    }

    /// [`Sandboxed::call`], a `write` or an `edit` expecting the file's version
    /// `expect` ([`crate::apply_expecting`]).
    pub fn call_expecting(
        &self,
        name: &str,
        arguments: &Value,
        expect: &crate::Expect,
    ) -> anyhow::Result<Value> {
        let mut call = json!({"name": name, "arguments": arguments});
        if let Some(version) = &expect.version {
            call["expect"] = version.as_str().into();
        }
        if !expect.by_file.is_empty() {
            call["by_file"] = Value::Object(expect.by_file.clone());
        }
        let call = call.to_string();
        // Said, never sent: a call past the most would be cut, and its
        // sandbox would answer only that it isn't JSON.
        let message_max = crate::limits().message_max();
        if call.len() as u64 >= message_max {
            bail!(
                "the call is {} bytes as JSON, past the {} MiB a call may be: write less at once",
                call.len(),
                message_max >> 20
            );
        }
        let mut running = self.running.borrow_mut();
        if running.is_none() {
            *running = Some(self.start()?);
        }
        let asked = running
            .as_mut()
            .map_or(Err(Unanswered::Ended), |r| r.ask(&call));
        let answer = match asked {
            Ok(answer) => answer,
            // It ended, answered past the most, or a sandbox that failed
            // to start never read the call: stopped, never left blocked on
            // a pipe, and what it said.
            Err(why) => {
                let mut ended = running.take().context("no sandbox")?;
                drop(ended.input.take());
                if why == Unanswered::TooLong {
                    let _ = ended.child.kill();
                }
                // Its output closed, a process of it still writing there
                // ends too.
                drop(ended.output.take());
                let status = ended.child.wait()?;
                if let Some(reading) = ended.reading.take() {
                    let _ = reading.join();
                }
                if why == Unanswered::TooLong {
                    bail!(
                        "the call's answer was past the {} MiB an answer may be, so its sandbox \
                         was stopped: ask for less at once",
                        crate::limits().message_max() >> 20
                    );
                }
                let errors = ended.errors.lock().map(|e| e.clone()).unwrap_or_default();
                bail!(
                    "the call didn't run ({status}): {}",
                    link_format::clean(errors.trim())
                );
            }
        };
        match (&answer["ok"], &answer["error"]) {
            (Value::Null, Value::String(error)) => bail!("{error}"),
            (Value::Null, _) => bail!("the call gave no answer"),
            (ok, _) => Ok(ok.clone()),
        }
    }

    fn start(&self) -> anyhow::Result<Running> {
        let (program, args) = self
            .apply
            .split_first()
            .context("no program to apply a call with")?;
        let mut child = link_secretive::process::command(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {program}"))?;
        let errors = Arc::new(Mutex::new(String::new()));
        let reading = child.stderr.take().map(|stderr| {
            let errors = errors.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { break };
                    let Ok(mut errors) = errors.lock() else { break };
                    if errors.len() + line.len() < ERRORS_MAX {
                        errors.push_str(&line);
                        errors.push('\n');
                    }
                }
            })
        });
        Ok(Running {
            input: child.stdin.take(),
            output: Some(BufReader::new(child.stdout.take().context("its output")?)),
            child,
            errors,
            reading,
        })
    }
}

/// Why a call has no answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unanswered {
    /// Its sandbox ended, or what it answered isn't an answer.
    Ended,
    /// Its answer ran past [`crate::Limits::message_max`].
    TooLong,
}

impl Running {
    /// The answer to `call`, or why there's none.
    fn ask(&mut self, call: &str) -> Result<Value, Unanswered> {
        let ended = |_| Unanswered::Ended;
        let input = self.input.as_mut().ok_or(Unanswered::Ended)?;
        input.write_all(call.as_bytes()).map_err(ended)?;
        input.write_all(b"\n").map_err(ended)?;
        input.flush().map_err(ended)?;
        let output = self.output.as_mut().ok_or(Unanswered::Ended)?;
        let mut line = Vec::new();
        output
            .take(crate::limits().message_max())
            .read_until(b'\n', &mut line)
            .map_err(ended)?;
        if !line.ends_with(b"\n") {
            return Err(match line.len() as u64 >= crate::limits().message_max() {
                true => Unanswered::TooLong,
                false => Unanswered::Ended,
            });
        }
        serde_json::from_slice(&line).map_err(|_| Unanswered::Ended)
    }
}
