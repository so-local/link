//! How a run is confined: its sandbox, as root, or as another user.

use super::*;

impl Here<'_> {
    /// Where the work starts: `workdir`, from the home folder, or the
    /// home folder; an error naming it when it isn't there.
    pub(super) fn workdir(&self) -> anyhow::Result<PathBuf> {
        let Some(workdir) = self.file.workdir() else {
            return Ok(self.home.clone());
        };
        let folder = self.home.join(workdir);
        ensure!(
            folder.is_dir(),
            "{}'s workdir {} isn't there: a stage makes it, as a clone does",
            self.file.name,
            folder.display()
        );
        Ok(folder)
    }

    /// The variables, and the secrets `values` says, for what runs; each
    /// sealed one given to git too, for its host.
    fn with(&self, values: Values, serve: Option<u16>, write: Vec<PathBuf>) -> RunWith {
        let mut env: BTreeMap<String, String> = self.file.variables.clone();
        let mut secrets = Vec::new();
        // Only those its file names: a job's own are its stages' alone.
        if values == Values::All {
            env.extend(
                self.given
                    .secrets
                    .iter()
                    .filter(|(name, _)| self.file.secret_names().contains(name))
                    .map(|(name, value)| (name.clone(), value.expose_owned())),
            );
        }
        if values != Values::None {
            let named = |name: &&String| self.file.sealed_names().contains(name);
            for (name, value) in self.given.sealed.iter().filter(|(n, _)| named(n)) {
                let Some(sealed) = self.given.sealing.get(name) else {
                    continue;
                };
                secrets.push(RunSecret {
                    secret: Sealed {
                        name: file::proxy_name(name),
                        value: value.duplicate(),
                        // `lnk sandbox run` makes one of its own, for this run.
                        placeholder: Default::default(),
                        rule: sealed.rule(),
                    },
                    url: link_secret_contract::sealed_url_var(name),
                    placeholder: name.clone(),
                    git: true,
                });
            }
        }
        RunWith {
            named: None,
            env,
            secrets,
            serve: serve.into_iter().collect(),
            write,
        }
    }

    /// `argv` started in `spec`'s sandbox, or as root outside any when
    /// `root`, from `workdir`, with the variables (and when
    /// `values`, the secrets) given on a pipe: its own process group,
    /// its input closed, its output this one's.
    pub(super) fn confined(
        &self,
        spec: Option<&Path>,
        user: Option<&User>,
        argv: &[String],
        write: Vec<PathBuf>,
        values: Values,
    ) -> anyhow::Result<Child> {
        let cwd = self.workdir()?;
        self.confined_in(spec, user, argv, write, values, None, &cwd, false)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "each is a separate choice of how the program runs, which the callers make apart"
    )]
    pub(super) fn confined_in(
        &self,
        spec: Option<&Path>,
        user: Option<&User>,
        argv: &[String],
        write: Vec<PathBuf>,
        values: Values,
        serve: Option<u16>,
        cwd: &Path,
        capture: bool,
    ) -> anyhow::Result<Child> {
        if let Some(upstream) = self.upstream {
            link_sandbox_contract::net::Upstream::parse(upstream)?;
            ensure!(
                spec.is_some() && matches!(whose(user), Whose::Own),
                "a routed stage requires its own sandbox, never root or another box user"
            );
        }
        let with = self.with(values, serve, write);
        let child = match (spec, whose(user)) {
            (_, Whose::Root) => {
                // Root only on a box: nothing `lnk env` runs on a computer
                // is root, whatever its file says.
                ensure!(
                    self.kind.is_box(),
                    "nothing runs as root but on a box, and this machine is a computer"
                );
                // As root, beside the proxy: nothing seals a secret.
                let mut command = link_secretive::process::command(sudo());
                command
                    .args(["-n", "-E", "--"])
                    .args(path_through_sudo(std::env::var("PATH").ok().as_deref()))
                    .args(argv)
                    .env_clear()
                    .envs(kept())
                    .envs(&with.env);
                // A root stage is the user's own shell, under sudo: it reads
                // an environment's secrets from its environment alone.
                for sealed in &with.secrets {
                    link_secretive::process::give(
                        &mut command,
                        &sealed.placeholder,
                        &sealed.secret.value,
                    );
                }
                spawn(command, cwd, capture)?
            }
            (None, Whose::Theirs(name)) => {
                ensure!(
                    self.kind.is_box(),
                    "a run as {name} is a box's, and this machine is a computer"
                );
                // Their values: the variables alone, never a secret given
                // to this user.
                let mut values = BTreeMap::new();
                for (variable, value) in &with.env {
                    if !self.given.secrets.contains_key(variable) {
                        values.insert(variable.clone(), link_secretive::Secret::new(value.clone()));
                    }
                }
                let words: Vec<String> = argv.iter().map(|w| crate::shell_quote(w)).collect();
                let command = run_as(
                    &box_users::Places::here(),
                    sudo().as_os_str(),
                    name,
                    &words.join(" "),
                    &values,
                )?;
                spawn(command, cwd, capture)?
            }
            (Some(_), Whose::Theirs(name)) => {
                bail!("a run as {name} is in {name}'s own sandbox: give it none")
            }
            (Some(_), Whose::Own) if self.direct => {
                // No sandbox, so no proxy to hold a sealed secret: one
                // is refused rather than given as a value.
                ensure!(
                    with.secrets.is_empty(),
                    "a sealed secret is held by a sandbox's proxy, and this runs with no sandbox"
                );
                let mut command = link_secretive::process::command(&argv[0]);
                command
                    .args(&argv[1..])
                    .env_clear()
                    .envs(kept())
                    .envs(&with.env);
                spawn(command, cwd, capture)?
            }
            (Some(spec), Whose::Own) => {
                let spec = self.dir.join(spec);
                let (reader, writer) = pipe()?;
                let mut command = link_core::lnk();
                command.args(["sandbox", "run", "--yes"]);
                if let Some(upstream) = self.upstream {
                    command.args(["--upstream", upstream]);
                }
                if let Some(protected) = self.protected {
                    let text = std::fs::read_to_string(&spec)?;
                    command.args(link_sandbox_client::protected::flags(protected, &text)?);
                } else {
                    command.arg("--spec").arg(&spec);
                }
                command.args(["--with-fd", "3", "--"]).args(argv);
                pass_fd(&mut command, &reader);
                let child = spawn(command, cwd, capture);
                drop(reader);
                let child = child?;
                let text = serde_json::to_vec(&with)?;
                let mut writer = std::fs::File::from(writer);
                // A child gone already has said why.
                let _ = writer.write_all(&text);
                drop(writer);
                child
            }
            (None, Whose::Own) => bail!("nothing says where it runs: a spec, or root"),
        };
        Ok(child)
    }
}

/// A run of `command` (a shell's command line) as box user `name`, in
/// their sandbox and folder, as their session would run it, given
/// `values`: `sudo` (`sudo`, the program) to them, then their sandbox's
/// [`box_users::entry`]. The values go in the environment, kept through
/// `sudo` and named to the sandbox (`--keep-env`), never on a command
/// line. An error if this box has no such user.
fn run_as(
    places: &box_users::Places,
    sudo: &std::ffi::OsStr,
    name: &str,
    command: &str,
    values: &BTreeMap<String, link_secretive::Secret>,
) -> anyhow::Result<Command> {
    let Some(user) = box_users::load(places, name)? else {
        bail!("{name} isn't a user of this box: lnk box user add {name} adds one");
    };
    let mut run = link_secretive::process::command(sudo);
    run.args(["-n", "-H", "-u", name]);
    if !values.is_empty() {
        let names: Vec<&str> = values.keys().map(String::as_str).collect();
        run.arg(format!("--preserve-env={}", names.join(",")));
    }
    run.arg("--")
        .args(box_users::entry(places, name, &user.folder));
    for (variable, value) in values {
        run.arg("--keep-env").arg(variable);
        link_secretive::process::give(&mut run, variable, value);
    }
    run.args(["--", command]);
    Ok(run)
}

/// What goes between `sudo --` and a root command so it keeps this one's
/// `PATH`: sudo puts its `secure_path` in its place, even with `-E`, and a
/// job's stages find the box's `lnk` on it.
fn path_through_sudo(path: Option<&str>) -> Vec<String> {
    match path {
        Some(path) if !path.is_empty() => vec!["/usr/bin/env".into(), format!("PATH={path}")],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A root stage keeps its caller's `PATH` past sudo's own.
    #[test]
    fn a_root_stage_keeps_its_path() {
        assert_eq!(
            super::path_through_sudo(Some("/home/ubuntu/.lnk/exec/bin:/usr/bin")),
            ["/usr/bin/env", "PATH=/home/ubuntu/.lnk/exec/bin:/usr/bin"]
        );
        assert!(super::path_through_sudo(None).is_empty());
        assert!(super::path_through_sudo(Some("")).is_empty());
    }

    /// A run as a box's user goes through `sudo` to them into their
    /// sandbox, its values in its environment, never on its command line;
    /// a run as a user the box hasn't is refused.
    #[test]
    fn a_run_as_a_box_user_enters_their_sandbox_given_its_values() {
        use std::os::unix::fs::PermissionsExt;
        let dir = link_test_kit::TempDir::new("env-run-as");
        // SAFETY: geteuid has no failure and touches no memory.
        let me = unsafe { libc::geteuid() };
        let places = box_users::Places {
            root: dir.join("box"),
            owners: vec![0, me],
        };
        let user = places.user("ann").join("user.toml");
        std::fs::create_dir_all(user.parent().unwrap()).unwrap();
        std::fs::write(
            &user,
            "schema = 1\nfolder = \"/home/ann\"\nkey = \"ssh-ed25519 AAAA\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut folder = user.clone();
        while folder.pop() && folder.starts_with(&*dir) {
            std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        if !link_test_kit::gate::trusted_file(&user, &[0, me]) {
            return;
        }
        let values = BTreeMap::from([(
            "ANNS_TOKEN".to_string(),
            link_secretive::Secret::new("ann-only".to_string()),
        )]);
        let sudo = std::ffi::OsStr::new("sudo");
        let run = run_as(&places, sudo, "ann", "echo hi", &values).unwrap();
        let words: Vec<String> = run
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let entry = box_users::entry(&places, "ann", Path::new("/home/ann"));
        let mut expected: Vec<String> =
            ["-n", "-H", "-u", "ann", "--preserve-env=ANNS_TOKEN", "--"]
                .map(String::from)
                .to_vec();
        expected.extend(entry);
        expected.extend(["--keep-env", "ANNS_TOKEN", "--", "echo hi"].map(String::from));
        assert_eq!(words, expected);
        let given = run
            .get_envs()
            .find(|(name, _)| *name == "ANNS_TOKEN")
            .and_then(|(_, value)| value);
        assert_eq!(given, Some(std::ffi::OsStr::new("ann-only")));
        let err = run_as(&places, sudo, "ben", "true", &BTreeMap::new()).unwrap_err();
        assert!(
            err.to_string().contains("ben isn't a user of this box"),
            "{err}"
        );
    }
}
