//! Headless system services use the same job and quoting as user services.
//! A plan is executable with the native runner or inspectable with a fake.

use crate::Job;
use anyhow::{Context, ensure};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Systemd,
    Launchd,
}

#[derive(Clone)]
pub struct Service {
    pub name: String,
    pub user: String,
    pub job: Job,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub systemd_directory: PathBuf,
    pub launchd_directory: PathBuf,
    pub systemctl: PathBuf,
    pub launchctl: PathBuf,
    pub restart_seconds: u64,
}

impl Default for Settings {
    fn default() -> Self {
        static SETTINGS: std::sync::LazyLock<Settings> = std::sync::LazyLock::new(|| {
            toml::from_str(include_str!("system.toml")).expect("system service defaults")
        });
        SETTINGS.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub required: bool,
}

pub struct Plan {
    pub control: Control,
    pub text: String,
}

pub struct Control {
    pub path: PathBuf,
    pub install: Vec<Invocation>,
    pub stop: Vec<Invocation>,
    pub remove: Vec<Invocation>,
}

impl Service {
    pub fn plan(&self, platform: Platform, settings: &Settings) -> anyhow::Result<Plan> {
        ensure!(
            !self.user.is_empty()
                && self
                    .user
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
            "invalid system service user"
        );
        ensure!(
            settings.restart_seconds > 0,
            "system service restart delay must be positive"
        );
        crate::service::check(&self.job)?;
        let control = Control::new(&self.name, platform, settings)?;
        let text = match platform {
            Platform::Systemd => {
                crate::service::unit_for(&self.job, Some(&self.user), settings.restart_seconds)
            }
            Platform::Launchd => crate::service::plist_for(
                &self.name,
                &self.job,
                Some(&self.user),
                settings.restart_seconds,
            ),
        };
        Ok(Plan { control, text })
    }
}

impl Control {
    pub fn new(name: &str, platform: Platform, settings: &Settings) -> anyhow::Result<Self> {
        ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b)),
            "invalid system service name"
        );
        let invoke = |program: &Path, args: Vec<String>, required| Invocation {
            program: program.into(),
            args,
            required,
        };
        let (path, install, stop, remove) = match platform {
            Platform::Systemd => {
                let unit = format!("{name}.service");
                let call = |args| invoke(&settings.systemctl, args, true);
                (
                    settings.systemd_directory.join(&unit),
                    vec![
                        call(vec!["daemon-reload".into()]),
                        call(vec!["enable".into(), "--now".into(), unit.clone()]),
                    ],
                    vec![call(vec!["stop".into(), unit.clone()])],
                    vec![
                        call(vec!["disable".into(), "--now".into(), unit]),
                        call(vec!["daemon-reload".into()]),
                    ],
                )
            }
            Platform::Launchd => {
                let path = settings.launchd_directory.join(format!("{name}.plist"));
                let target = format!("system/{name}");
                let call = |args, required| invoke(&settings.launchctl, args, required);
                let stop = call(vec!["bootout".into(), target.clone()], false);
                let install = vec![
                    stop.clone(),
                    call(
                        vec![
                            "bootstrap".into(),
                            "system".into(),
                            path.display().to_string(),
                        ],
                        true,
                    ),
                    call(vec!["enable".into(), target.clone()], true),
                    call(vec!["kickstart".into(), "-k".into(), target], true),
                ];
                (path, install, vec![stop.clone()], vec![stop])
            }
        };
        ensure!(
            path.is_absolute(),
            "system service directory must be absolute"
        );
        Ok(Control {
            path,
            install,
            stop,
            remove,
        })
    }

    pub fn stop(
        &self,
        mut run: impl FnMut(&Invocation) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.run(&self.stop, &mut run)
    }

    pub fn remove(
        &self,
        mut run: impl FnMut(&Invocation) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        // Remove the file before the final daemon reload, preventing the
        // removed service from becoming eligible again at the next boot.
        let (last, before) = self
            .remove
            .split_last()
            .context("missing service removal operation")?;
        self.run(before, &mut run)?;
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        self.run(std::slice::from_ref(last), &mut run)
    }

    fn run(
        &self,
        calls: &[Invocation],
        run: &mut impl FnMut(&Invocation) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        for call in calls {
            if let Err(error) = run(call)
                && call.required
            {
                return Err(error);
            }
        }
        Ok(())
    }
}

impl Plan {
    pub fn install(
        &self,
        mut run: impl FnMut(&Invocation) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        link_files::write_private(&self.control.path, self.text.as_bytes())
            .context("writing system service")?;
        self.control.run(&self.control.install, &mut run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn both_system_drivers_keep_argument_boundaries_and_remove_their_own_file() {
        let root = link_test_kit::TempDir::new("system-service");
        let settings = Settings {
            systemd_directory: root.to_path_buf(),
            launchd_directory: root.to_path_buf(),
            ..Settings::default()
        };
        let service = Service {
            name: "example".into(),
            user: "root".into(),
            job: Job {
                argv: vec![
                    "/opt/Link Tools/lnk".into(),
                    "--name".into(),
                    "cost$%<&".into(),
                ],
                env: BTreeMap::new(),
                workdir: root.to_path_buf(),
                log: root.join("service.log"),
            },
        };
        for platform in [Platform::Systemd, Platform::Launchd] {
            let plan = service.plan(platform, &settings).unwrap();
            let mut called = Vec::new();
            plan.install(|call| {
                called.push(call.clone());
                Ok(())
            })
            .unwrap();
            assert_eq!(called, plan.control.install);
            assert!(plan.control.path.is_file());
            match platform {
                Platform::Systemd => assert!(
                    plan.text.contains("cost$$%%<&") && plan.text.contains("multi-user.target")
                ),
                Platform::Launchd => assert!(
                    plan.text.contains("cost$%&lt;&amp;")
                        && plan.text.contains("<string>System</string>")
                ),
            }
            plan.control.remove(|_| Ok(())).unwrap();
            assert!(!plan.control.path.exists());
        }
        let invalid = Service {
            name: "../other".into(),
            ..service
        };
        assert!(invalid.plan(Platform::Systemd, &settings).is_err());
    }
}
