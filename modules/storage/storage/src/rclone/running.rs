//! Running rclone: each command gets a config file of only the sections it
//! reaches, the Keychain's settings and the account's sign-ins, and nothing
//! of the shell's own setup; the files of runs that died are swept.

use super::*;
use link_format::WithinExt;

impl Rclone {
    /// rclone with `args`, and the sign-in of each account the clouds
    /// they name sign in as.
    pub(super) fn command(&self, args: &[&str]) -> anyhow::Result<Command> {
        let mut command = link_secretive::process::command(&self.program);
        let sign_ins = self.sign_ins_for(args)?;
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            let theirs = !sign_ins.is_empty()
                && (name.starts_with("AWS_") || name == "GOOGLE_APPLICATION_CREDENTIALS");
            if name.starts_with("RCLONE_") || theirs {
                command.env_remove(key);
            }
        }
        // Only the sections this run reaches, secrets and all, in a file
        // of its own: never another cloud's or folder's key, and none in
        // its environment. rclone's own setup writes the config file.
        let config = match args.first() {
            Some(&"config") => self.config.clone(),
            _ => self.run_config(args, &sign_ins)?,
        };
        let mut set: BTreeMap<String, Secret> = BTreeMap::new();
        for (_, sign_in) in sign_ins {
            for (var, value) in &sign_in.env {
                ensure!(
                    set.get(var).is_none_or(|v| v == value),
                    "these clouds sign in as different accounts in a way that can't mix in one \
                     run: one at a time"
                );
                set.insert(var.clone(), value.duplicate());
                // A cloud's sign-in, as rclone's cloud reads it: from its
                // environment (AWS's keys, Google's credentials file).
                link_secretive::process::give(&mut command, var, value);
            }
        }
        command.arg("--config").arg(&config);
        command.arg("--ask-password=false");
        command.args(args);
        Ok(command)
    }

    /// The sections `args` name (`aws:bucket`, or its encryption's,
    /// `aws-personal-crypt:`) that sign in as an account, each with that
    /// account's sign-in.
    pub(super) fn sign_ins_for(
        &self,
        args: &[&str],
    ) -> anyhow::Result<Vec<(String, link_cloud_contract::Reply)>> {
        let named = named_sections(args);
        if named.is_empty() {
            return Ok(Vec::new());
        }
        let accounts = self.accounts()?;
        // A section is the cloud's whose name it starts with the longest:
        // `aws-job-personal-crypt` is cloud `aws-job`'s, not `aws`'s.
        let mut clouds: Vec<String> =
            Clouds::load_from(&self.config.with_file_name("clouds.toml"))?
                .clouds
                .into_iter()
                .map(|c| c.name)
                .chain(accounts.keys().cloned())
                .collect();
        clouds.sort_by_key(|c| std::cmp::Reverse(c.len()));
        let whose = |section: &str| clouds.iter().find(|c| is_of(section, c)).cloned();
        let mut found = Vec::new();
        for (cloud, account) in accounts {
            if !named
                .iter()
                .any(|section| whose(section).as_deref() == Some(cloud.as_str()))
            {
                continue;
            }
            let sign_in = self
                .sign_in(&account)
                .within(format!("signing {cloud} in as the account {account}"))?;
            found.push((cloud, sign_in));
        }
        Ok(found)
    }

    /// A config file for one run with `args`: the sections they name, and
    /// each remote one of those wraps (`remote = <section>:...`), with
    /// their secrets from the config file and the Keychain and the keys
    /// `sign_ins` give, written private beside the config file. Read back
    /// once the run is done ([`Rclone::settle_runs`]).
    fn run_config(
        &self,
        args: &[&str],
        sign_ins: &[(String, link_cloud_contract::Reply)],
    ) -> anyhow::Result<PathBuf> {
        self.settle_runs();
        let keychain = self.keychain_settings()?;
        let mut wanted: Vec<String> = named_sections(args)
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut written: Vec<(String, Vec<(String, String)>)> = Vec::new();
        while let Some(name) = wanted.pop() {
            if written.iter().any(|(n, _)| *n == name) {
                continue;
            }
            let Some(mut settings) = file_section(&self.config, &name)? else {
                continue;
            };
            for (section, key, value) in &keychain {
                if *section == name {
                    settings.push((key.clone(), value.expose_owned()));
                }
            }
            for (key, value) in &settings {
                if key == "remote" || key == "upstreams" {
                    wanted.extend(
                        value
                            .split_whitespace()
                            .filter_map(|part| part.split_once(':'))
                            .map(|(section, _)| section.to_string())
                            .filter(|section| !section.is_empty()),
                    );
                }
            }
            written.push((name, settings));
        }
        let mut given = written.clone();
        let mut added = Vec::new();
        for (section, sign_in) in sign_ins {
            let Some((_, settings)) = given.iter_mut().find(|(n, _)| n == section) else {
                continue;
            };
            for (key, value) in sign_in.credential.iter() {
                if key != "type" {
                    settings.retain(|(k, _)| k != key);
                    settings.push((key.clone(), value.expose_owned()));
                    added.push((section.clone(), key.clone()));
                }
            }
        }
        let mut text = String::new();
        for (name, settings) in &given {
            text += &format!("[{name}]\n");
            for (key, value) in settings {
                ensure!(
                    !key.contains(['\n', '\r', '=']) && !value.contains(['\n', '\r']),
                    "a setting can't span lines"
                );
                text += &format!("{key} = {value}\n");
            }
            text.push('\n');
        }
        static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = self.config.with_file_name("runs");
        link_files::create_private_dir(&dir)?;
        let path = dir.join(format!(
            "rclone-{}-{}.conf",
            std::process::id(),
            RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        link_files::write_private(&path, text.as_bytes())?;
        self.runs.borrow_mut().push(Run {
            path: path.clone(),
            written,
            added,
        });
        Ok(path)
    }

    /// Each run's config file read back and removed: a section rclone
    /// changed (a token it refreshed) saved to the config file, each key a
    /// sign-in gave as the section had it.
    pub(super) fn settle_runs(&self) {
        for run in self.runs.borrow_mut().drain(..) {
            for (name, before) in &run.written {
                let Ok(Some(after)) = file_section(&run.path, name) else {
                    continue;
                };
                // A key a sign-in gave is the section's own as it was, or
                // none: never saved.
                let given = |k: &str| run.added.iter().any(|(s, a)| s == name && a == k);
                let mut after: Vec<(String, String)> =
                    after.into_iter().filter(|(k, _)| !given(k)).collect();
                after.extend(before.iter().filter(|(k, _)| given(k)).cloned());
                let mut sorted_before = before.clone();
                sorted_before.sort();
                let mut sorted_after = after.clone();
                sorted_after.sort();
                if sorted_after != sorted_before {
                    let after: Vec<(&str, &str)> = after
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.as_str()))
                        .collect();
                    if let Err(err) = put_section(&self.config, name, &after) {
                        crate::said::warn(
                            link_format::sentence!(
                                "The settings rclone changed for {} could not be saved.",
                                name.as_str()
                            ),
                            &err,
                        );
                    }
                }
            }
            let _ = std::fs::remove_file(&run.path);
        }
    }

    /// Each cloud that signs in as a `lnk cloud` account, by its section,
    /// with that account.
    fn accounts(&self) -> anyhow::Result<BTreeMap<String, String>> {
        let mut accounts = self.signing_in.borrow().clone();
        let clouds = self.config.with_file_name("clouds.toml");
        for cloud in Clouds::load_from(&clouds)?.clouds {
            if let (Some(account), None) = (&cloud.account, &cloud.only_for) {
                accounts.insert(cloud.name.clone(), account.clone());
            }
        }
        Ok(accounts)
    }

    /// `account`'s sign-in for its buckets (its adapter's `storage`), asked
    /// once a command.
    fn sign_in(&self, account: &str) -> anyhow::Result<link_cloud_contract::Reply> {
        if let Some(known) = self.sign_ins.borrow().get(account) {
            return Ok(known.duplicate());
        }
        let sign_in = crate::connect::call_cloud(account, "storage", &serde_json::json!({}))?;
        self.sign_ins
            .borrow_mut()
            .insert(account.to_string(), sign_in.duplicate());
        Ok(sign_in)
    }

    /// Why cloud `name` reads only, when it signs in as a `lnk cloud`
    /// account whose storage says it can't write (an account past what it
    /// may keep): the account and its cloud's message.
    pub fn reads_only(&self, name: &str) -> anyhow::Result<Option<(String, String)>> {
        let Some(account) = self.accounts()?.get(name).cloned() else {
            return Ok(None);
        };
        let sign_in = self.sign_in(&account)?;
        if sign_in.writes != Some(false) {
            return Ok(None);
        }
        let message = sign_in
            .message
            .clone()
            .unwrap_or_else(|| format!("{account} reads only: delete some, or make room there"));
        Ok(Some((account, link_format::clean(&message))))
    }

    /// Deletes `files` from `cloud` through its account's adapter
    /// (`delete-objects`), by their names in the bucket (encrypted on an
    /// encrypted cloud), a batch at a time: an account that reads only
    /// frees space this way. What the adapter said when it refused the
    /// first batch (one that doesn't answer `delete-objects`), leaving it
    /// to rclone.
    pub(super) fn delete_through(
        &self,
        account: &str,
        cloud: &Cloud,
        files: &[String],
    ) -> anyhow::Result<Option<anyhow::Error>> {
        let names = match cloud.encrypted {
            true => self.encrypted_names(cloud, files)?,
            false => files.to_vec(),
        };
        for (n, batch) in names
            .chunks(link_cloud_contract::MAX_DELETE_OBJECTS)
            .enumerate()
        {
            let request = serde_json::json!({"bucket": cloud.bucket, "objects": batch});
            match crate::connect::call_cloud(account, "delete-objects", &request) {
                Ok(_) => {}
                Err(err) if n == 0 => return Ok(Some(err)),
                Err(err) => {
                    return Err(
                        err.context(format!("deleting from {} through {account}", cloud.name))
                    );
                }
            }
        }
        Ok(None)
    }

    /// Runs rclone with `args`, its output captured; fails with what it
    /// said unless it succeeds.
    pub(super) fn run(&self, args: &[&str]) -> anyhow::Result<Output> {
        let output = self.output(args)?;
        if !output.status.success() {
            bail!("{}", said(&output));
        }
        Ok(output)
    }

    pub(super) fn output(&self, args: &[&str]) -> anyhow::Result<Output> {
        self.command(args)?
            .stdin(Stdio::null())
            .output()
            .within("running rclone")
    }

    /// Runs rclone with `args` and a list of `files` for
    /// `--files-from-raw`, its output shown (progress, errors): each line
    /// after its cloud's name when sending beside others.
    pub(super) fn run_shown(&self, args: &[&str], files: &[String]) -> anyhow::Result<bool> {
        let list = FileList::new(files)?;
        let mut command = self.command(args)?;
        command.arg("--files-from-raw").arg(&list.path);
        command.stdin(Stdio::null());
        let terminal = link_format::streams::stderr_is_terminal();
        let Some(name) = &self.beside else {
            if terminal {
                command.arg("--progress");
            }
            return Ok(command.status().within("running rclone")?.success());
        };
        // Progress redraws its lines, which clouds side by side would
        // mix: a line of it every few seconds instead.
        if terminal {
            command.args([
                "--stats",
                "3s",
                "--stats-one-line",
                "--stats-log-level",
                "NOTICE",
                "--log-format",
                "",
            ]);
        }
        let mut child = command
            .stderr(Stdio::piped())
            .spawn()
            .within("running rclone")?;
        let said = child.stderr.take().within("no stderr")?;
        for line in std::io::BufRead::split(std::io::BufReader::new(said), b'\n') {
            let line = line?;
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_start_matches("NOTICE:").trim();
            if !line.is_empty() {
                crate::said::note(link_format::Sentence::adopt(&format!("{name}: {line}")));
            }
        }
        Ok(child.wait().within("running rclone")?.success())
    }

    /// The settings in the Keychain, fetched once per process unless
    /// which they are changes: each fetch asks the Keychain once per key.
    pub(super) fn keychain_settings(&self) -> anyhow::Result<Settings> {
        if !keychain(&self.config) {
            return Ok(Vec::new());
        }
        let listed = std::fs::read_to_string(keychain_list_path(&self.config)).unwrap_or_default();
        let mut cached = self.keychain.borrow_mut();
        match &*cached {
            Some((list, settings)) if *list == listed => Ok(settings.duplicate()),
            _ => {
                let settings = keychain_settings(&self.config)?;
                *cached = Some((listed, settings.duplicate()));
                Ok(settings)
            }
        }
    }

    /// Forgets what's cached about the config: it changed.
    pub(super) fn changed(&self) {
        self.keychain.take();
        self.features.borrow_mut().clear();
    }

    /// `rclone backend features` of `remote`, asked once.
    pub(super) fn features(&self, remote: &str) -> anyhow::Result<serde_json::Value> {
        if let Some(features) = self.features.borrow().get(remote) {
            return Ok(features.clone());
        }
        let output = self.run(&["backend", "features", remote])?;
        let features: serde_json::Value =
            serde_json::from_slice(&output.stdout).within("reading rclone's features")?;
        self.features
            .borrow_mut()
            .insert(remote.to_string(), features.clone());
        Ok(features)
    }

    /// The checksums `remote` has (`md5`, `sha1`, ...).
    pub(super) fn hashes(&self, remote: &str) -> anyhow::Result<Vec<String>> {
        let features = self.features(remote)?;
        Ok(features["Hashes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|h| h.as_str().map(str::to_string))
            .collect())
    }
}

/// Whether config section `section` is cloud `cloud`'s own, or the
/// encryption of one of its buckets (`<cloud>-<folder>-crypt`: your files',
/// or an owner's, by its ID name-safe).
fn is_of(section: &str, cloud: &str) -> bool {
    if section == cloud {
        return true;
    }
    let folder = section
        .strip_prefix(cloud)
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|rest| rest.strip_suffix("-crypt"));
    folder.is_some_and(|folder| {
        folder == crate::files::PERSONAL || crate::files::from_safe(folder).is_some()
    })
}

/// How long a run's config file is kept at most, its process alive or
/// not: past any run's.
const RUN_MOST: Duration = Duration::from_secs(24 * 60 * 60);

/// Removes the config files in `runs` that a run killed before it read
/// them back left there (secrets, from the Keychain too): each whose
/// process is gone, and any past [`RUN_MOST`].
pub(super) fn sweep_runs(runs: &Path) {
    for entry in std::fs::read_dir(runs).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let pid = name
            .to_str()
            .and_then(|n| n.strip_prefix("rclone-"))
            .and_then(|n| n.split('-').next())
            .and_then(|pid| pid.parse::<libc::pid_t>().ok());
        // SAFETY: signal 0 only asks whether the process is there.
        let gone = pid.is_none_or(|pid| {
            pid <= 0
                || (unsafe { libc::kill(pid, 0) } == -1
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH))
        });
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age > RUN_MOST);
        if gone || old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The config sections `args` name (`aws:bucket`, or its encryption's,
/// `aws-personal-crypt:`): a file here whose name has a `:` is no
/// section.
fn named_sections<'a>(args: &[&'a str]) -> Vec<&'a str> {
    args.iter()
        .filter(|arg| !arg.starts_with(['/', '.', '-']) && std::fs::symlink_metadata(arg).is_err())
        .filter_map(|arg| arg.split_once(':').map(|(section, _)| section))
        .filter(|section| !section.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A run's config file left by a process that's gone (killed before it
    /// read its file back) is removed at the next start; a live one's
    /// stays.
    #[test]
    fn a_killed_runs_config_is_swept_at_the_next_start() {
        let dir = link_test_kit::TempDir::new("rclone-sweep");
        let runs = dir.join("runs");
        std::fs::create_dir_all(&runs).unwrap();
        let dead = runs.join("rclone-2147483646-0.conf");
        let live = runs.join(format!("rclone-{}-0.conf", std::process::id()));
        std::fs::write(&dead, "[a]\npassword = theirs\n").unwrap();
        std::fs::write(&live, "[a]\n").unwrap();
        Rclone::with(dir.join("rclone"), &dir).unwrap();
        assert!(!dead.exists());
        assert!(live.exists());
    }

    /// A run is handed the sections it reaches alone, in a private file
    /// of its own: the bucket it names and the cloud that bucket is on,
    /// never another folder's bucket or its password, and none of it in its
    /// environment. A token rclone refreshed in that file is saved.
    #[test]
    fn a_run_is_given_only_the_sections_it_reaches() {
        let dir = link_test_kit::TempDir::new("rclone-run-config");
        let seen = dir.join("seen");
        let program = dir.join("rclone");
        link_test_kit::write_program(
            &program,
            format!(
                "#!/bin/sh\nenv > '{seen}.env'\nwhile [ $# -gt 0 ]; do\n  \
                 if [ \"$1\" = --config ]; then cp \"$2\" '{seen}.conf'; \
                 sed -i 's/^type = local$/type = local\\ntoken = fresh/' \"$2\"; fi\n  \
                 shift\ndone\nexit 0\n",
                seen = seen.display()
            ),
        )
        .unwrap();
        let config_dir = dir.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        let rclone = Rclone::with(program, &config_dir).unwrap();
        std::fs::write(
            config_in(&config_dir),
            "[drive]\ntype = local\n\n[drive-a1-crypt]\ntype = crypt\nremote = drive:a1\n\
             password = mine\n\n[drive-b2-crypt]\ntype = crypt\nremote = drive:b2\n\
             password = theirs\n",
        )
        .unwrap();
        rclone.object_get("drive-a1-crypt:x", 10).unwrap();
        let given = std::fs::read_to_string(dir.join("seen.conf")).unwrap();
        assert!(given.contains("[drive-a1-crypt]") && given.contains("password = mine"));
        assert!(given.contains("[drive]"), "{given}");
        assert!(
            !given.contains("theirs") && !given.contains("drive-b2"),
            "{given}"
        );
        let env = std::fs::read_to_string(dir.join("seen.env")).unwrap();
        assert!(!env.contains("RCLONE_CONFIG_"), "{env}");
        assert!(!env.contains("mine"), "{env}");
        // Read back once done: the token saved, the file gone.
        drop(rclone);
        assert_eq!(
            section(&config_in(&config_dir), "drive").unwrap().unwrap(),
            [
                ("type".to_string(), "local".to_string()),
                ("token".to_string(), "fresh".to_string())
            ]
        );
        assert_eq!(
            std::fs::read_dir(config_dir.join("runs")).unwrap().count(),
            0
        );
    }
}
