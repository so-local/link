//! OS overrides and authored paths.
use super::*;

impl Spec {
    /// Its first path that's relative or climbs with `..`, in any table: a
    /// folder of `[files]` or a program's path in `[commands] allow`. A
    /// spec sent to run elsewhere has none,
    /// since there it's read from wherever it's written, which is no
    /// folder of the work's, and a `..` names what its writer can't see.
    pub fn relative_path(&self) -> Option<&str> {
        self.paths().into_iter().find(|p| {
            !(*p == "~" || p.starts_with("~/") || p.starts_with('/'))
                || p.split('/').any(|part| part == "..")
        })
    }

    /// Every path it names, as written, in any table: each folder of
    /// `[files]` and each program's path in `[commands] allow`.
    pub fn paths(&self) -> Vec<&str> {
        let mut paths: Vec<&String> = Vec::new();
        let mut files = self.files.as_ref();
        let mut os_files = Vec::new();
        while let Some(f) = files {
            for list in [&f.write, &f.read, &f.write_dot_files, &f.hide] {
                paths.extend(list.iter().flatten());
            }
            os_files.extend(
                [f.macos.as_deref(), f.linux.as_deref()]
                    .into_iter()
                    .flatten(),
            );
            files = os_files.pop();
        }
        let mut commands = self.commands.as_ref();
        let mut os_commands = Vec::new();
        while let Some(c) = commands {
            paths.extend(c.allow.iter().flatten().filter(|a| a.contains('/')));
            os_commands.extend(
                [c.macos.as_deref(), c.linux.as_deref()]
                    .into_iter()
                    .flatten(),
            );
            commands = os_commands.pop();
        }
        paths.into_iter().map(String::as_str).collect()
    }

    /// The tables it holds for another OS than `os` (`[files.linux]` on
    /// a Mac), which a spec made [`Spec::on`] `os` leaves out.
    pub fn others_than(&self, os: Os) -> Vec<String> {
        let other = match os {
            Os::MacOs => "linux",
            Os::Linux => "macos",
        };
        let table = toml::Table::try_from(self).unwrap_or_default();
        table
            .iter()
            .filter(|(_, v)| v.as_table().is_some_and(|t| t.contains_key(other)))
            .map(|(section, _)| format!("[{section}.{other}]"))
            .collect()
    }

    /// The spec as it is on `os`: each section's OS table applied, and
    /// none left.
    pub fn on(&self, os: Os) -> anyhow::Result<Spec> {
        Ok(Spec {
            name: self.name.clone(),
            version: self.version,
            files: on_os!(
                &self.files,
                os,
                "files",
                Files,
                [write, read, write_dot_files],
                [hide]
            )?,
            network: on_os!(&self.network, os, "network", Network, [mode], [])?,
            internet: on_os!(
                &self.internet,
                os,
                "internet",
                Internet,
                [hosts, tcp, per_minute],
                []
            )?,
            lan: on_os!(&self.lan, os, "lan", Lan, [hosts], [])?,
            localhost: on_os!(
                &self.localhost,
                os,
                "localhost",
                Localhost,
                [ports, models],
                []
            )?,
            tools: on_os!(&self.tools, os, "tools", Tools, [names], [])?,
            browser: on_os!(&self.browser, os, "browser", Browser, [hosts], [])?,
            commands: on_os!(
                &self.commands,
                os,
                "commands",
                Commands,
                [allow, toolchains, developer_tools],
                []
            )?,
            teams: self.teams.clone(),
        })
    }

    /// This spec with `flags`' sections over its own: a key a flag gives
    /// replaces the file's, but `hide`, which adds up. Both already
    /// [`Spec::on`] one OS.
    pub fn with(&self, flags: &Spec) -> Spec {
        fn over<T: Clone>(
            base: &Option<T>,
            flag: &Option<T>,
            merge: impl Fn(T, &T) -> T,
        ) -> Option<T> {
            match (base, flag) {
                (Some(b), Some(f)) => Some(merge(b.clone(), f)),
                (None, Some(f)) => Some(f.clone()),
                (b, None) => b.clone(),
            }
        }
        Spec {
            name: flags.name.clone().or_else(|| self.name.clone()),
            version: flags.version.or(self.version),
            files: over(&self.files, &flags.files, |b, f| {
                layer!(b, f, [write, read, write_dot_files], [hide])
            }),
            network: over(&self.network, &flags.network, |b, f| {
                layer!(b, f, [mode], [])
            }),
            internet: over(&self.internet, &flags.internet, |b, f| {
                layer!(b, f, [hosts, tcp, per_minute], [])
            }),
            lan: over(&self.lan, &flags.lan, |b, f| layer!(b, f, [hosts], [])),
            localhost: over(&self.localhost, &flags.localhost, |b, f| {
                layer!(b, f, [ports, models], [])
            }),
            tools: over(&self.tools, &flags.tools, |b, f| layer!(b, f, [names], [])),
            browser: over(&self.browser, &flags.browser, |b, f| {
                layer!(b, f, [hosts], [])
            }),
            commands: over(&self.commands, &flags.commands, |b, f| {
                layer!(b, f, [allow, toolchains, developer_tools], [])
            }),
            // A team the flags name replaces the file's, key by key.
            teams: self
                .teams
                .clone()
                .into_iter()
                .chain(flags.teams.clone())
                .collect(),
        }
    }

    /// The same spec with its folders absolute (from `dir`, the spec's
    /// folder, and `home`), as `lnk sandbox spec show` prints it.
    pub fn absolute(&self, dir: &Path, home: &Path) -> anyhow::Result<Spec> {
        let paths = |ps: &Option<Vec<String>>| -> anyhow::Result<Option<Vec<String>>> {
            ps.as_ref()
                .map(|ps| {
                    ps.iter()
                        .map(|p| match p.as_str() {
                            "~" => Ok(p.clone()),
                            _ => link_spec::path(Path::new(p), dir, home)
                                .map(|p| p.display().to_string()),
                        })
                        .collect()
                })
                .transpose()
        };
        let mut spec = self.clone();
        if let Some(files) = &mut spec.files {
            files.write = paths(&files.write)?;
            files.read = paths(&files.read)?;
            files.write_dot_files = paths(&files.write_dot_files)?;
            files.hide = paths(&files.hide)?;
        }
        if let Some(commands) = &mut spec.commands
            && let Some(allow) = &commands.allow
        {
            let allow = allow
                .iter()
                .map(|entry| match entry.contains('/') {
                    true => link_spec::path(Path::new(entry), dir, home).map(|p| {
                        let mut text = p.display().to_string();
                        if entry.ends_with('/') {
                            text.push('/');
                        }
                        text
                    }),
                    false => Ok(entry.clone()),
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            commands.allow = Some(allow);
        }
        Ok(spec)
    }
}
