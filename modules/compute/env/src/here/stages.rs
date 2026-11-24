//! A file's stages and a run's, each member run in its step.

use super::*;

impl Here<'_> {
    /// A stage, as its file says, given the secrets `values` says (as
    /// root, none: a sealed one would reach it as a value): one saved in
    /// the image from the home folder, since it makes `workdir`, and one
    /// at every start from `workdir`. An error naming it when it fails.
    pub(super) fn stage(&self, step: &file::Step, values: Values) -> anyhow::Result<()> {
        let members = crate::stage::Stage::<&file::Stage>::from(step);
        let ran = run_step(&members, |stage, timeout, prefix| {
            let mut stage = (*stage).clone();
            stage.timeout = timeout;
            let values = if stage.root() { Values::None } else { values };
            let cwd = if stage.saved() {
                self.home.clone()
            } else {
                self.workdir()?
            };
            self.run_stage(&stage, values, Vec::new(), &cwd, prefix)
        })?;
        match ran {
            Ran::Passed => Ok(()),
            Ran::Failed(code) => {
                say(self.mark, &format!("failed {}", step.name()));
                bail!("stage {} failed (exit status: {code})", step.name())
            }
            Ran::TimedOut(timeout) => {
                say(self.mark, &format!("failed {}", step.name()));
                bail!("stage {} reached its timeout, {timeout}", step.name())
            }
        }
    }

    /// `stage` run from `cwd`, given the secrets `values` says, writing
    /// `write` besides what its sandbox spec allows; said with its seconds
    /// when it passes.
    #[expect(
        clippy::disallowed_methods,
        reason = "polls a stage within its timeout"
    )]
    fn run_stage(
        &self,
        stage: &file::Stage,
        values: Values,
        write: Vec<PathBuf>,
        cwd: &Path,
        prefix: bool,
    ) -> anyhow::Result<Ran> {
        let started = Instant::now();
        // As root, what it starts is root's, which this can't signal: its
        // timeout is `timeout`'s, under sudo, which kills its group.
        let argv = match (stage.root(), stage.timeout) {
            (true, Some(timeout)) => [
                vec![
                    "timeout".into(),
                    "-s".into(),
                    "KILL".into(),
                    format!("{}s", timeout.get().as_secs_f64()),
                ],
                sh(&stage.run),
            ]
            .concat(),
            _ => sh(&stage.run),
        };
        let mut child = self
            .confined_in(
                stage.sandbox.as_deref(),
                stage.user.as_ref(),
                &argv,
                write,
                values,
                None,
                cwd,
                prefix,
            )
            .within(format!(
                "starting stage {} in {}",
                stage.name,
                cwd.display()
            ))?;
        let output = if prefix {
            [
                child.stdout.take().map(|r| prefix_output(r, &stage.name)),
                child.stderr.take().map(|r| prefix_output(r, &stage.name)),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let status = match stage.timeout {
            None => child.wait(),
            Some(timeout) => loop {
                match child.try_wait() {
                    Ok(Some(status)) if started.elapsed() < timeout.get() => break Ok(status),
                    Ok(None) if started.elapsed() < timeout.get() => {
                        std::thread::sleep(Duration::from_millis(100))
                    }
                    Ok(_) => {
                        say(
                            self.mark,
                            &format!(
                                "stage {} {:.1} timed-out",
                                stage.name,
                                started.elapsed().as_secs_f64()
                            ),
                        );
                        stop_group(child.id());
                        untrack(&child);
                        let _ = child.wait();
                        for thread in output {
                            let _ = thread.join();
                        }
                        return Ok(Ran::TimedOut(timeout));
                    }
                    Err(err) => break Err(err),
                }
            },
        };
        if prefix {
            stop_group(child.id());
        }
        untrack(&child);
        for thread in output {
            let _ = thread.join();
        }
        let status = status?;
        if !status.success() {
            say(
                self.mark,
                &format!(
                    "stage {} {:.1} failed",
                    stage.name,
                    started.elapsed().as_secs_f64()
                ),
            );
            #[cfg(unix)]
            if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
                return Ok(Ran::Failed(128 + signal));
            }
            return Ok(Ran::Failed(status.code().unwrap_or(1)));
        }
        say(
            self.mark,
            &format!(
                "stage {} {:.1}",
                stage.name,
                started.elapsed().as_secs_f64()
            ),
        );
        Ok(Ran::Passed)
    }
}

impl Here<'_> {
    /// The run's `stages`, in order, from `workdir`, given `values` (the
    /// environment's file with what the run adds), each writing `out` too:
    /// the first that fails ends it with its exit code, and one that
    /// reaches its timeout with 124; 0 when every one passed.
    pub(super) fn run(
        &self,
        stages: &[Shipped],
        mut values: Env,
        out: Option<&Out>,
        profile: Option<&link_env_contract::profile::Profiling>,
    ) -> anyhow::Result<i32> {
        let sampler = profile.and_then(|p| {
            let first = stages.first()?.name();
            Some(super::profile::Sampler::start(
                self.mark,
                p.every.get(),
                first,
            ))
        });
        let mut write = Vec::new();
        if let Some(out) = out {
            let folder = self.home.join(&out.folder);
            link_files::create_private_dir(&folder)
                .within(format!("making {}", folder.display()))?;
            // Given as a variable beside the file's.
            values
                .variables
                .insert(out.variable.clone(), folder.display().to_string());
            write.push(folder);
        }
        let cwd = self.workdir()?;
        let here = Here {
            upstream: None,
            protected: None,
            file: &values,
            dir: self.dir,
            home: self.home.clone(),
            mark: self.mark,
            given: self.given,
            kind: self.kind,
            direct: self.direct,
        };
        let label = random_hex(4)?;
        for step in stages {
            if let Some(sampler) = &sampler {
                sampler.edge(step.name());
            }
            let ran = run_step(step, |shipped, timeout, prefix| {
                let spec = shipped
                    .sandbox
                    .as_deref()
                    .map(|text| write_spec(&format!("run-{label}-{}", shipped.name), text))
                    .transpose()?;
                let stage = file::Stage {
                    name: shipped.name.clone(),
                    run: shipped.run.clone(),
                    sandbox: spec.clone(),
                    user: shipped.user.clone(),
                    inputs: None,
                    when: file::When::Start,
                    timeout,
                };
                let here = Here {
                    upstream: shipped.upstream.as_deref(),
                    protected: shipped.protected.as_deref(),
                    ..here.clone()
                };
                let ran = here.run_stage(&stage, Values::All, write.clone(), &cwd, prefix);
                if let Some(spec) = &spec {
                    let _ = std::fs::remove_file(spec);
                }
                ran
            })?;
            // Its end, whatever it was, before the next stage's start.
            if let Some(sampler) = &sampler {
                sampler.edge(step.name());
            }
            match ran {
                Ran::Passed => {}
                Ran::Failed(code) => {
                    say(self.mark, &format!("failed {}", step.name()));
                    return Ok(code);
                }
                Ran::TimedOut(_) => {
                    say(self.mark, &format!("timed-out {}", step.name()));
                    return Ok(124);
                }
            }
        }
        Ok(0)
    }
}

/// Run every member before returning the first failure received. A group
/// has one deadline, in addition to each member's own limit.
fn run_step<T: crate::stage::Member + Sync>(
    step: &crate::stage::Stage<T>,
    run: impl Fn(&T, Option<file::Timeout>, bool) -> anyhow::Result<Ran> + Sync,
) -> anyhow::Result<Ran> {
    use crate::stage::Stage;
    match step {
        Stage::Command(command) => run(command, command.timeout(), false),
        Stage::Parallel(group) => std::thread::scope(|scope| {
            let (send, receive) = std::sync::mpsc::channel();
            let started = Instant::now();
            for member in &group.parallel {
                let send = send.clone();
                let run = &run;
                scope.spawn(move || {
                    let remaining = group.timeout.map(|t| {
                        file::Timeout::from_std(t.get().saturating_sub(started.elapsed()))
                    });
                    let timeout = match (member.timeout(), remaining) {
                        (Some(a), Some(b)) => Some(file::Timeout::from_std(a.get().min(b.get()))),
                        (a, b) => a.or(b),
                    };
                    let _ = send.send(run(member, timeout, true));
                });
            }
            drop(send);
            let mut first = None;
            for result in receive {
                if !matches!(result, Ok(Ran::Passed)) && first.is_none() {
                    first = Some(result);
                }
            }
            first.unwrap_or(Ok(Ran::Passed))
        }),
    }
}

/// Prefix bounded chunks, so even a command without newlines cannot grow
/// an unbounded buffer. Hold stdout's lock only while writing one chunk.
fn prefix_output(
    mut reader: impl std::io::Read + Send + 'static,
    name: &str,
) -> std::thread::JoinHandle<()> {
    let name = name.to_owned();
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        let mut out =
            link_format::streams::passed(link_format::streams::Stream::Out).labeled(&name);
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let _ = out.write_all(&buffer[..n]);
            let _ = out.flush();
        }
    })
}

/// How a stage ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ran {
    Passed,
    /// With its exit code, 128 and the signal when one ended it.
    Failed(i32),
    /// At its own timeout.
    TimedOut(file::Timeout),
}
