//! Where Hermes lives, installing it and its features, and uninstalling it.

use super::*;

/// `~/.hermes`: its configuration, conversations and other runtime data.
pub(super) fn hermes_home() -> PathBuf {
    hermes_home_in(&link_files::home_dir())
}

/// [`hermes_home`] in home `home`.
fn hermes_home_in(home: &std::path::Path) -> PathBuf {
    home.join(".hermes")
}

pub(super) fn programs_root() -> PathBuf {
    link_files::layout::exec_in(
        link_files::layout::Hidden::HermesPrograms,
        &link_files::home_dir(),
    )
}

/// `uninstall` (`link_core::uninstall`): it runs nothing of its own
/// and keeps nothing outside the agents' folders, which are the harness
/// module's (`agents`); what Hermes holds in each agent's home is said
/// there, for `lnk secret list`.
pub(super) fn uninstall(args: &link_core::uninstall::Flags) -> Result<Done, Report> {
    use link_core::uninstall as u;
    let kept = u::Kept {
        parts: vec![u::Part {
            name: "agents".into(),
            about: "your agents: their Hermes homes".into(),
            credentials: held(&harness::homes("hermes")),
            ..Default::default()
        }],
        ..Default::default()
    };
    u::run(args, &kept, || Ok(()), |_| Ok(()))?;
    Ok(Done::shown())
}

/// What Hermes holds in each of `homes` (an agent's name and its home),
/// as `lnk secret list` shows it: its endpoint's key, Link's key for the
/// start. Its `.env` also holds a copy of the agent's model key, which
/// the harness module lists.
fn held(homes: &[(String, PathBuf)]) -> Vec<link_core::uninstall::Held> {
    use link_core::uninstall::Held;
    let mut held = Vec::new();
    for (agent, home) in homes {
        let env = super::configuring::link_env_in(&hermes_home_in(home));
        if env.is_file() {
            let kept = format!("{} (only yours)", env.display());
            held.push(Held {
                what: link_core::uninstall::CredentialKind::HarnessToken.into(),
                of: format!("{agent}'s Hermes endpoint (ENDPOINT_KEY)"),
                kept,
                may: format!(
                    "Calls {agent}'s Hermes on this machine through its endpoint: chats with it as you"
                ),
            });
        }
    }
    held
}

/// Copies this program into the home, unless it's there as it is: the
/// sandbox runs programs only from there.
fn copy_self() -> anyhow::Result<()> {
    let me = std::env::current_exe().within("finding this program")?;
    let to = super::installed();
    if std::fs::read(&to).ok() == Some(std::fs::read(&me)?) {
        return Ok(());
    }
    std::fs::create_dir_all(to.parent().within("finding its folder")?)?;
    let aside = to.with_extension("new");
    std::fs::copy(&me, &aside).within(format!("copying to {}", aside.display()))?;
    std::fs::rename(&aside, &to).within(format!("writing {}", to.display()))?;
    Ok(())
}

/// The installed `hermes`: the launcher its installer publishes in its
/// code (`.hermes/bin`, bound to the Python its package manager keeps in
/// the payload's `tools`), once the payload in `~/.lnk/exec/harnesses/hermes` is
/// sealed.
pub(super) fn bin() -> Option<PathBuf> {
    crate::workarounds::payload(&programs_root())
        .ok()
        .map(|payload| payload.launcher)
}

/// [`bin`]'s first place: the launcher in Hermes's code, at `code`.
pub(super) fn launcher_in(code: &std::path::Path) -> PathBuf {
    code.join(&crate::workarounds::CONFIG.launcher)
}

fn launcher() -> PathBuf {
    launcher_in(&programs_root().join(&crate::workarounds::CONFIG.code))
}

pub(super) fn hermes() -> anyhow::Result<Command> {
    Ok(crate::workarounds::payload(&programs_root())?.command(&hermes_home()))
}

/// The last `configure`, by the files it writes: Hermes's config and its
/// `.env`.
pub(super) fn configured() -> harness::Configured {
    harness::Configured::new(
        &format!("{}/{VERSION}", env!("CARGO_PKG_VERSION")),
        vec![
            hermes_home().join("config.yaml"),
            hermes_home().join(".env"),
        ],
    )
}

/// The report for a Hermes that isn't installed.
pub(super) fn not_installed() -> Report {
    Report::error(sentence!("Hermes isn't installed."))
        .next(cmd!("lnk agent harness set hermes"), "install it")
}

pub(super) fn install(out: &mut Out) -> Result<Done, Report> {
    copy_self()?;
    if launcher().is_file() {
        crate::workarounds::payload(&programs_root())?;
        return Ok(Done::shown());
    }
    // A new install is configured anew, whatever the last one left.
    configured().forget();
    out.block(Note(sentence!(
        "Installing Hermes {} into {} (first time only).",
        VERSION.to_string(),
        Path::new(programs_root())
    )));
    // --non-interactive skips its setup wizard and its own gateway service;
    // --skip-browser and --skip-computer-use leave out the Chromium and the
    // desktop driver it would download, which no sandboxed run uses;
    // --commit checks out exactly the pinned release; --dir keeps its code
    // in its home, where a root user's install would otherwise go to
    // /usr/local, outside the sandbox's home and shared by every agent.
    let code = programs_root().join(&crate::workarounds::CONFIG.code);
    let env = installer_env(&programs_root());
    let env: Vec<_> = env
        .iter()
        .map(|(name, folder)| (name.as_str(), folder.as_os_str()))
        .collect();
    // Its installer writes to this terminal; between its lines, for minutes
    // at a time, Link says it's still going.
    out.hand_over();
    let _still = harness::progress::Heartbeat::start("installing Hermes");
    harness::run_installer_with(
        &INSTALLER,
        &[
            "--non-interactive".as_ref(),
            "--skip-browser".as_ref(),
            "--skip-computer-use".as_ref(),
            "--commit".as_ref(),
            COMMIT.as_ref(),
            "--dir".as_ref(),
            code.as_os_str(),
            "--hermes-home".as_ref(),
            programs_root().as_os_str(),
        ],
        &env,
    )?;
    if !launcher().is_file() {
        return Err(Report::error(sentence!(
            "The Hermes installer finished, but no hermes program was found in {}.",
            Path::new(programs_root())
        ))
        .next(
            cmd!("lnk agent harness set hermes"),
            "try the install again",
        ));
    }
    install_features()?;
    // Sealing applies the catalogue fixes too, before the payload is published.
    crate::workarounds::seal(&programs_root())?;
    Ok(Done::shown())
}

/// Where the tools Hermes's installer runs put what they download, all in
/// the staged payload at `root`: the one folder its install may run what it
/// writes in, published read-only with the rest. Left to uv's default, the
/// Python that starts its package manager would land in the agent's home,
/// which no run may execute from.
fn installer_env(root: &std::path::Path) -> Vec<(String, PathBuf)> {
    crate::workarounds::CONFIG
        .installer_folders
        .iter()
        .map(|(name, folder)| (name.clone(), root.join(folder)))
        .collect()
}

/// All supported extras are installed before publication, independent of
/// which provider the agent first configures. PM state belongs to staging.
fn features_command(root: &std::path::Path) -> Command {
    let mut pm =
        link_secretive::process::command(launcher_in(&root.join(&crate::workarounds::CONFIG.code)));
    pm.env("HERMES_HOME", root);
    crate::workarounds::runtime(&mut pm);
    pm.args(["pm", "install"]);
    for feature in &crate::workarounds::CONFIG.extras {
        pm.args(["--extra", feature]);
    }
    pm
}

fn install_features() -> anyhow::Result<()> {
    let mut pm = features_command(&programs_root());
    harness::run_quiet(harness::trusting_machine_cas(&mut pm, harness::SYSTEM_CAS))
        .within("installing Hermes's packages for its model and channels")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uv_installs_its_python_in_the_staged_payload() {
        let root = link_test_kit::TempDir::new("hermes-installer-env");
        let env = installer_env(&root);
        assert_eq!(
            env,
            [(
                "UV_PYTHON_INSTALL_DIR".to_owned(),
                root.join("bootstrap-python")
            )]
        );
        assert!(env.iter().all(|(_, folder)| folder.starts_with(&*root)));
    }

    #[test]
    fn what_it_would_install_on_first_use_is_installed_with_it() {
        // OpenAI and OpenRouter are in the base package; Link's bridge
        // carries channels. Anthropic must be staged even for another provider.
        assert_eq!(crate::workarounds::CONFIG.extras, ["anthropic"]);
        let root = link_test_kit::TempDir::new("hermes-staged-extras");
        let command = features_command(&root);
        assert_eq!(
            command.get_program(),
            launcher_in(&root.join(&crate::workarounds::CONFIG.code))
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["pm", "install", "--extra", "anthropic"]
        );
        assert_eq!(
            command
                .get_envs()
                .find(|(name, _)| *name == "HERMES_HOME")
                .unwrap()
                .1,
            Some(root.as_os_str())
        );
    }
}
