//! A loopback proxy binds before preparing its program. The launch
//! template is private and immutable; each running profile is its own.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::ensure;
use link_format::WithinExt;
use serde::{Deserialize, Serialize};

/// What is deferred until a loopback proxy has bound its port: generated
/// environment (including this proxy's URL capabilities) and its
/// Seatbelt profile. Downstream service credentials and inherited run
/// variables' values are not persisted here.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    env: BTreeMap<String, String>,
    profile: Option<Profile>,
    #[serde(default)]
    given: Vec<String>,
    #[serde(skip)]
    dir: PathBuf,
}

link_files::specification!(Launch, include_str!("launch.schema.toml"));

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    /// The argument that names sandbox-exec's generated profile.
    argument: usize,
    template: String,
}

impl Launch {
    pub(super) fn keep(
        name: &str,
        argv: &[String],
        env: &BTreeMap<String, String>,
    ) -> anyhow::Result<PathBuf> {
        let profile = argv.windows(3).enumerate().find_map(|(at, args)| {
            (args[0] == "/usr/bin/sandbox-exec" && args[1] == "-f").then_some((at + 2, &args[2]))
        });
        let profile = profile
            .map(|(argument, path)| -> anyhow::Result<Profile> {
                let template = std::fs::read_to_string(path)?;
                // The unique initial profile has been snapshotted. Each
                // launch writes its own bound profile from this template.
                std::fs::remove_file(path)?;
                Ok(Profile { argument, template })
            })
            .transpose()?;
        let launch = Launch {
            env: env.clone(),
            given: Vec::new(),
            profile,
            dir: PathBuf::new(),
        };
        let path = crate::rules_file(name)?
            .with_extension(format!("launch-{}.json", link_files::random_token()?));
        link_files::write_private(&path, &link_files::saved::print_json(&launch)?)?;
        Ok(path)
    }

    pub fn read(path: &Path) -> anyhow::Result<Self> {
        // The filesystem root's owner is privileged in containers whose
        // image maps root to another uid. Trust that identity along with
        // root and this process, while refusing writable ancestors.
        let owners = [unsafe { libc::geteuid() }, 0, std::fs::metadata("/")?.uid()];
        link_files::fs::check_folders(
            path,
            link_files::fs::Trust {
                owners: &owners,
                deny_mode: 0o022,
                ancestors: true,
                no_link: true,
            },
        )?;
        let trusted = link_files::fs::read_trusted(
            path,
            link_files::fs::Trust::alone(&[unsafe { libc::geteuid() }]),
            1024 * 1024,
            "a proxy launch's",
        )?;
        let mut launch: Self = link_files::saved::parse_json(trusted.text.as_bytes())
            .within("reading the proxy launch")?;
        launch.dir = trusted
            .path
            .parent()
            .within("the launch has no folder")?
            .to_owned();
        Ok(launch)
    }

    /// The run's declared variable names, never their values at rest.
    /// Called by the parent before launching its freshly wrapped command.
    pub fn given(path: &Path, names: impl IntoIterator<Item = String>) -> anyhow::Result<()> {
        let mut launch = Self::read(path)?;
        launch.given = names.into_iter().collect();
        for name in &launch.given {
            link_sandbox_client::check_given_variable(name)?;
        }
        link_files::write_private(path, &link_files::saved::print_json(&launch)?)?;
        Ok(())
    }

    /// Fill only the generated proxy address after the listener owns it.
    /// The profile is unique to this run, so simultaneous starts cannot
    /// put one run behind another's proxy.
    pub fn prepare(&self, port: u16, argv: &[String]) -> anyhow::Result<Prepared> {
        ensure!(port != 0, "the loopback proxy hasn't bound its port");
        let mut env: BTreeMap<_, _> = self
            .env
            .iter()
            .map(|(name, value)| {
                (
                    name.clone(),
                    link_sandbox_proxy::client::bound_port(value, port),
                )
            })
            .collect();
        for name in &self.given {
            link_sandbox_client::check_given_variable(name)?;
            let value = link_secretive::Secret::from_env(name)
                .within(format!("the run wasn't given {name}"))?;
            env.entry(name.clone())
                .or_insert_with(|| link_sandbox_proxy::client::bound_port(value.expose(), port));
        }
        let mut argv = argv.to_vec();
        let mut kept = None;
        if let Some(profile) = &self.profile {
            ensure!(
                profile.argument >= 2 && profile.argument < argv.len(),
                "invalid profile argument"
            );
            ensure!(
                argv[profile.argument - 2] == "/usr/bin/sandbox-exec"
                    && argv[profile.argument - 1] == "-f",
                "invalid profile command"
            );
            ensure!(
                profile.template.contains("\"localhost:0\""),
                "the profile has no proxy port"
            );
            let content = profile
                .template
                .replace("\"localhost:0\"", &format!("\"localhost:{port}\""));
            ensure!(self.dir.is_absolute(), "the launch has no trusted folder");
            let path = self
                .dir
                .join(format!("loopback-{}.sb", link_files::random_token()?));
            link_files::write_private(&path, content.as_bytes())?;
            argv[profile.argument] = path.display().to_string();
            kept = Some(path);
        }
        Ok(Prepared { argv, env, kept })
    }
}

pub struct Prepared {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    kept: Option<PathBuf>,
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if let Some(path) = &self.kept {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_metadata_requires_a_private_file_and_trusted_ancestors() {
        use std::os::unix::fs::PermissionsExt;
        let dir = link_test_kit::TempDir::new("launch-trust");
        let path = dir.join("launch.json");
        link_files::write_private(&path, br#"{"env":{},"profile":null,"schema":1}"#).unwrap();
        Launch::read(&path).unwrap();
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(Launch::read(&link).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Launch::read(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(&*dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(Launch::read(&path).is_err());
        std::fs::set_permissions(&*dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        Launch::read(&path).unwrap();
    }

    #[test]
    fn each_bound_proxy_gets_its_own_profile_and_environment() {
        let dir = link_test_kit::TempDir::new("launch-profile");
        let launch = Launch {
            given: Vec::new(),
            env: [("HTTPS_PROXY".into(), "http://127.0.0.1:0".into())].into(),
            profile: Some(Profile {
                argument: 2,
                template: "(allow network-outbound (remote ip \"localhost:0\"))".into(),
            }),
            dir: dir.to_path_buf(),
        };
        let argv: Vec<String> = ["/usr/bin/sandbox-exec", "-f", "/unused/template.sb", "sh"]
            .map(String::from)
            .into();
        let first = launch.prepare(51234, &argv).unwrap();
        let second = launch.prepare(51235, &argv).unwrap();
        assert_ne!(first.argv[2], second.argv[2]);
        assert!(
            std::fs::read_to_string(&first.argv[2])
                .unwrap()
                .contains("localhost:51234")
        );
        assert!(
            std::fs::read_to_string(&second.argv[2])
                .unwrap()
                .contains("localhost:51235")
        );
        assert_eq!(first.env["HTTPS_PROXY"], "http://127.0.0.1:51234");
        let profile = PathBuf::from(&first.argv[2]);
        drop(first);
        assert!(!profile.exists());
        assert!(launch.prepare(0, &argv).is_err());
    }
}
