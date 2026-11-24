//! Services and agents: started, served in the background, and waited
//! on until ready.

use super::*;

impl Here<'_> {
    /// Whether `service` answers as ready now.
    pub(super) fn ready(&self, service: &Service) -> bool {
        match (service.ready.port, &service.ready.run) {
            (Some(port), _) => std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_millis(500),
            )
            .is_ok(),
            (None, Some(run)) => self
                .confined(
                    service.sandbox.as_deref(),
                    service.user.as_ref(),
                    &sh(run),
                    Vec::new(),
                    Values::None,
                )
                .and_then(|mut child| {
                    let status = child.wait();
                    untrack(&child);
                    Ok(status?.success())
                })
                .unwrap_or(false),
            (None, None) => true,
        }
    }

    /// Waits until `service` is ready, its timeout, or `child` (the
    /// service, when this started it) ends first.
    #[expect(
        clippy::disallowed_methods,
        reason = "polls a service until it's ready"
    )]
    fn wait_ready(&self, service: &Service, mut child: Option<&mut Child>) -> anyhow::Result<()> {
        let started = Instant::now();
        let timeout = service.ready.timeout.get();
        // A command's check is a sandbox each time: asked less often the
        // longer it waits.
        let mut pause = Duration::from_millis(250);
        loop {
            if self.ready(service) {
                say(
                    self.mark,
                    &format!(
                        "service {} {:.1}",
                        service.name,
                        started.elapsed().as_secs_f64()
                    ),
                );
                return Ok(());
            }
            if let Some(child) = child.as_deref_mut()
                && let Some(status) = child.try_wait()?
            {
                say(self.mark, &format!("failed {}", service.name));
                bail!(
                    "service {} ended before it was ready ({status})",
                    service.name
                );
            }
            if started.elapsed() > timeout {
                say(self.mark, &format!("failed {}", service.name));
                bail!(
                    "service {} wasn't ready after {}s",
                    service.name,
                    timeout.as_secs()
                );
            }
            std::thread::sleep(pause);
            pause = next_pause(pause, service.ready.run.is_some());
        }
    }

    /// A job's service: none when it's up already (the box was started
    /// with it), else started here, waited on, and stopped at the job's
    /// end.
    pub(super) fn service_for_job(&self, service: &Service) -> anyhow::Result<Option<Child>> {
        if self.ready(service) {
            say(self.mark, &format!("service {} 0.0", service.name));
            return Ok(None);
        }
        // A service is given the variables and never a secret: the file
        // doesn't choose where one goes, and as root it would read them.
        let cwd = self.workdir()?;
        let mut child = self.confined_in(
            service.sandbox.as_deref(),
            service.user.as_ref(),
            &sh(&service.run),
            Vec::new(),
            Values::None,
            service.ready.port,
            &cwd,
            false,
        )?;
        if let Err(err) = self.wait_ready(service, Some(&mut child)) {
            stop_group(child.id());
            untrack(&child);
            let _ = child.wait();
            return Err(err);
        }
        Ok(Some(child))
    }

    /// A service of a box started with the environment: a background
    /// service (`lnk env serve`), which outlives this, waited on.
    pub(super) fn start_service(&self, service: &Service, file: &Path) -> anyhow::Result<()> {
        let (label, unit) = service_names(&self.file.name, &service.name);
        let path = self.dir.join(file);
        let mut argv = vec![
            lnk_path()?,
            "env".into(),
            "serve".into(),
            "--spec".into(),
            path.display().to_string(),
            service.name.clone(),
        ];
        if self.direct {
            argv.push("--no-sandbox".into());
        }
        let job = link_service::Job {
            argv,
            env: BTreeMap::new(),
            workdir: self.workdir()?,
            log: link_files::layout::Hidden::Envs
                .path()
                .join("logs")
                .join(format!("{}-{}.log", self.file.name, service.name)),
        };
        link_service::start(&label, &unit, &job)?;
        link_service::keep_after_logout(&format!("service {}", service.name));
        self.wait_ready(service, None)
    }
}

/// `lnk env serve --spec <file> <service>`: a service of a started environment,
/// in its sandbox, until it's stopped: what its background service runs.
/// It's given the variables, never a secret: a background service's
/// file is on the disk. `direct`, in no sandbox, where the machine's
/// policy allows it.
pub fn serve(path: &Path, name: &str, direct: bool) -> anyhow::Result<i32> {
    if direct {
        link_sandbox_contract::isolation::check_unsandboxed()?;
    }
    let read = file::read(path)?;
    let service = read.file.service(name)?;
    let given = Given::default();
    let here = Here {
        upstream: None,
        protected: None,
        file: &read.file,
        dir: &read.dir,
        home: link_files::try_home_dir()?,
        mark: "",
        given: &given,
        kind: link_machine::machine::kind()?,
        direct,
    };
    let cwd = here.workdir()?;
    let mut child = here.confined_in(
        service.sandbox.as_deref(),
        service.user.as_ref(),
        &sh(&service.run),
        Vec::new(),
        Values::None,
        service.ready.port,
        &cwd,
        false,
    )?;
    let status = child.wait()?;
    Ok(status.code().unwrap_or(1))
}

/// The wait after `pause` before a service is asked again whether it's
/// ready: a port's check is a connection, at once; a command's a
/// sandbox, so twice as long each time, up to 2 seconds.
fn next_pause(pause: Duration, by_command: bool) -> Duration {
    match by_command {
        true => (pause * 2).min(Duration::from_secs(2)),
        false => pause,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_asked_less_often_the_longer_it_waits() {
        let mut pause = Duration::from_millis(250);
        let mut waits = Vec::new();
        for _ in 0..5 {
            pause = next_pause(pause, true);
            waits.push(pause.as_millis());
        }
        assert_eq!(waits, [500, 1000, 2000, 2000, 2000]);
        let port = Duration::from_millis(250);
        assert_eq!(next_pause(port, false), port);
    }
}
