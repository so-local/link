//! What a launch may ask for, checked before it runs, and the hint a
//! blocked step leaves.

use super::*;

pub fn check_launch(launch: &Launch, boxed: &Boxed) -> anyhow::Result<()> {
    // Read and write grants are inputs to Link's sandbox. With the
    // owner's explicit --no-sandbox choice there is no file boundary to
    // grant, so a harness installed in the user's home must not be
    // rejected merely for naming its own program or data there. Ports,
    // services and reserved environment remain contract boundaries.
    let mut unsandboxed;
    let launch = if boxed.sandbox {
        launch
    } else {
        unsandboxed = launch.clone();
        unsandboxed.read.clear();
        unsandboxed.write.clear();
        &unsandboxed
    };
    if let Some(port) = launch.served().iter().find(|p| !launch.ports.contains(p)) {
        bail!("port {port} is to be served, but isn't one of its own");
    }
    if let Some(base) = boxed.port {
        let block = base..base.saturating_add(boxed.block);
        if let Some(port) = launch.ports.iter().find(|p| !block.contains(p)) {
            bail!(
                "port {port} isn't in its agent's ({}-{}): this adapter doesn't take \
                 LNK_PORT, so it would take ports another agent, or another user's, has",
                block.start,
                block.end - 1
            );
        }
    }
    // Its teams' shared folders among them, wherever they're kept: what a
    // harness reaches of them is its grant's, never its launch's.
    let guarded: Vec<PathBuf> = [
        boxed.dir.clone(),
        boxed.folder.home.clone(),
        link_files::home_dir(),
        link_files::installation_dir(),
        settings::agents(),
        link_agent_contract::archive_dir(),
        PathBuf::from("/tmp"),
        PathBuf::from("/private/tmp"),
    ]
    .into_iter()
    .chain(boxed.teams.hide.iter().cloned())
    .chain(boxed.teams.write.iter().cloned())
    .chain(boxed.teams.read.iter().cloned())
    .collect();
    let resolved = |paths: Vec<PathBuf>| -> Vec<PathBuf> {
        paths
            .into_iter()
            .flat_map(|p| [p.canonicalize().unwrap_or_else(|_| p.clone()), p])
            .collect()
    };
    let guarded = resolved(guarded.to_vec());
    // Other programs' sockets (a session's bus, Docker's), read or written.
    let sockets = resolved(link_sandbox_client::socket_folders());
    for path in launch.write.iter().chain(&launch.read) {
        let plain = path.is_absolute()
            && link_sandbox_client::plain_path(path)
            && path
                .components()
                .all(|c| !matches!(c, std::path::Component::ParentDir));
        if !plain {
            bail!("{} isn't a plain absolute path", path.display());
        }
        let real = path.canonicalize().unwrap_or_else(|_| path.clone());
        for p in [path, &real] {
            if let Some(g) = guarded
                .iter()
                .chain(&sockets)
                .find(|g| p.starts_with(g) || g.starts_with(p))
            {
                bail!("{} is, holds or is in {}", p.display(), g.display());
            }
        }
    }
    // What's written there runs outside the sandbox, as you.
    let user_path = std::env::var_os("PATH").unwrap_or_default();
    let on_path: Vec<PathBuf> = std::env::split_paths(&user_path)
        .filter(|d| d.is_absolute())
        .flat_map(|d| [d.canonicalize().unwrap_or_else(|_| d.clone()), d])
        .collect();
    for path in &launch.write {
        let real = path.canonicalize().unwrap_or_else(|_| path.clone());
        for p in [path, &real] {
            if p == Path::new("/")
                || link_sandbox_client::SYSTEM_FOLDERS
                    .iter()
                    .any(|s| p.starts_with(s))
            {
                bail!("{}: the system's folders can't be written", p.display());
            }
            if let Some(d) = on_path
                .iter()
                .find(|d| p.starts_with(d) || d.starts_with(p))
            {
                bail!(
                    "{} is, holds or is in {}, on your PATH",
                    p.display(),
                    d.display()
                );
            }
        }
    }
    for service in &launch.services {
        let name = !service.is_empty()
            && !service.starts_with('.')
            && service
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b));
        if !name || crate::service::is_links(service) {
            bail!("{service:?} isn't another service's name");
        }
    }
    for key in launch.env.keys() {
        let reserved = [
            "HOME",
            "TMPDIR",
            "LNK_FILES",
            AGENT_ENV,
            link_agent_contract::identity::AGENT_ID_ENV,
            link_agent_contract::identity::OWNER_ENV,
            link_agent_contract::identity::SERVICE_ENV,
            link_harness_contract::harness::PORT_ENV,
            link_measure_contract::client::ENV,
            link_interactions::STORE_VAR,
            link_files::layout::exec_variable(),
        ]
        .contains(&key.as_str())
            || key.starts_with("DYLD_")
            || key.starts_with("LD_");
        let word = !key.is_empty()
            && !key.starts_with(|c: char| c.is_ascii_digit())
            && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if reserved || !word {
            bail!("it may not set {key:?} in the environment");
        }
    }
    Ok(())
}

/// When a sandboxed step died from a signal, how to see what the sandbox
/// refused it (a killed process says nothing itself).
pub(super) fn blocked_hint(boxed: &Boxed, status: &std::process::ExitStatus) -> String {
    #[cfg(unix)]
    let signaled = std::os::unix::process::ExitStatusExt::signal(status).is_some();
    #[cfg(not(unix))]
    let signaled = false;
    if !boxed.sandbox || !signaled {
        return String::new();
    }
    if cfg!(target_os = "macos") {
        format!(
            "; the sandbox may have stopped it: log show --last 5m --style compact \
             --predicate 'sender==\"Sandbox\"' lists what it refused, and lnk agent harness \
             permissions --harness {} what it may do",
            boxed.name
        )
    } else {
        format!(
            "; the sandbox may have stopped it (lnk agent harness-permission list --harness {})",
            boxed.name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_can_only_ask_for_what_is_safe() {
        let mut boxed = Boxed {
            name: "h".into(),
            agent: "main".into(),
            identity: None,
            service: None,
            programs: None,
            profile: "main-h".into(),
            folders: None,
            teams: Default::default(),
            port: None,
            block: settings::PORT_BLOCK,
            dir: "/nonexistent/home/.lnk/data/agents/main/harnesses/h".into(),
            folder: link_agent_contract::folder::Folder {
                home: "/nonexistent/files".into(),
                agent: "/nonexistent/agent".into(),
            },
            sandbox: true,
            ports: BTreeSet::new(),
            models: BTreeSet::new(),
            path: String::new(),
            net: "/nonexistent/net".into(),
            wires: None,
            spec: Spec::default(),
            proxied: Spec::default(),
        };
        let ok = Launch {
            argv: vec!["/usr/bin/true".into()],
            env: BTreeMap::from([("PATH".into(), "/usr/bin".into())]),
            read: vec!["/opt/tools".into()],
            services: vec!["ai.otter.gateway".into(), "heron-gateway.service".into()],
            ..Launch::default()
        };
        check_launch(&ok, &boxed).unwrap();
        let refused = |change: fn(&mut Launch)| {
            let mut launch = ok.clone();
            change(&mut launch);
            check_launch(&launch, &boxed).is_err()
        };
        assert!(refused(|l| l.write.push(
            "/nonexistent/home/.lnk/data/agents/main/harnesses/h/x".into()
        )));
        assert!(refused(|l| l.write.push("/nonexistent/files".into())));
        assert!(refused(|l| l.read.push("/".into())));
        assert!(refused(|l| l.read.push("/tmp/h-cache".into())));
        assert!(refused(|l| l.read.push("relative".into())));
        assert!(refused(|l| l
            .read
            .push("/opt/x\")\n(allow default)".into())));
        assert!(refused(|l| l.read.push("/opt/../etc".into())));
        assert!(refused(|l| l.services.push("../../.zshrc".into())));
        // Other programs' sockets, read or written, as no spec may.
        assert!(refused(|l| l.read.push("/run/user/1000/bus".into())));
        assert!(refused(|l| l.read.push("/var/run/docker.sock".into())));
        assert!(refused(|l| l.write.push("/run/h".into())));
        // No write to the system's folders, or to one on PATH; reading
        // them is fine.
        for system in ["/usr/local/bin", "/opt/homebrew/bin", "/etc/h", "/"] {
            let mut launch = ok.clone();
            launch.write.push(system.into());
            assert!(check_launch(&launch, &boxed).is_err(), "{system}");
        }
        let path = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path).filter(|d| d.is_absolute()) {
            let mut launch = ok.clone();
            launch.write.push(dir.clone());
            assert!(check_launch(&launch, &boxed).is_err(), "{}", dir.display());
        }
        let mut launch = ok.clone();
        launch.read.push("/usr/local/bin".into());
        launch.write.push("/srv/h-data".into());
        check_launch(&launch, &boxed).unwrap();
        // Serving only its own ports.
        assert!(refused(|l| l.serve = Some(vec![18789])));
        for links in [
            "link.agent.main",
            "lnk-agent-other.service",
            "link.exit",
            "link.harness",
        ] {
            let mut launch = ok.clone();
            launch.services.push(links.into());
            assert!(check_launch(&launch, &boxed).is_err(), "{links}");
        }
        assert!(refused(|l| {
            l.env.insert("HOME".into(), "/".into());
        }));
        assert!(refused(|l| {
            l.env.insert("DYLD_INSERT_LIBRARIES".into(), "x".into());
        }));
        assert!(refused(|l| {
            l.env.insert("LNK_PORT".into(), "1".into());
        }));
        assert!(refused(|l| {
            l.env.insert("LNK_AGENT".into(), "other".into());
        }));
        // A team's shared folder kept outside the home, granted or not.
        boxed.teams.hide.push("/srv/team-research".into());
        boxed.teams.write.push("/srv/team-press/drafts".into());
        for team in ["/srv/team-research/x", "/srv/team-press/drafts", "/srv"] {
            let mut launch = ok.clone();
            launch.read.push(team.into());
            assert!(check_launch(&launch, &boxed).is_err(), "{team}");
        }
        // With a base port, only its block.
        let second = Boxed {
            port: Some(20200),
            ..boxed
        };
        let at = |ports: Vec<u16>| {
            let launch = Launch {
                ports,
                ..ok.clone()
            };
            check_launch(&launch, &second)
        };
        at(vec![20200, 20399]).unwrap();
        let err = at(vec![20200, 18789]).unwrap_err();
        assert!(err.to_string().contains("LNK_PORT"), "{err}");

        // With no sandbox, file grants have no boundary to widen. A
        // homepage-installed harness can name a program in the user's
        // home, while the other contract checks still apply.
        let mut direct = second;
        direct.sandbox = false;
        let mut in_home = ok.clone();
        in_home
            .read
            .push("/nonexistent/home/.local/bin/harness".into());
        in_home
            .write
            .push("/nonexistent/home/.cache/harness".into());
        check_launch(&in_home, &direct).unwrap();
        in_home.env.insert("HOME".into(), "/tmp/other".into());
        assert!(check_launch(&in_home, &direct).is_err());
    }
}
