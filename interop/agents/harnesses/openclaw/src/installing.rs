//! Where OpenClaw lives, installing it and uninstalling it.

use super::*;

/// `~/.openclaw`: its install prefix and its state, one folder.
pub(super) fn prefix() -> PathBuf {
    prefix_in(&link_files::home_dir())
}

/// [`prefix`] in home `home`.
fn prefix_in(home: &Path) -> PathBuf {
    home.join(".openclaw")
}

pub(super) fn programs_root() -> PathBuf {
    link_files::layout::exec_in(
        link_files::layout::Hidden::OpenclawPrograms,
        &link_files::home_dir(),
    )
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

pub(super) fn bin() -> PathBuf {
    programs_root().join("bin/openclaw")
}

/// The last structural `configure`, by the OpenClaw config it writes.
/// Its `.env` is refreshed cheaply on every configure: rotating Link's
/// endpoint key or a model key must not rerun OpenClaw's CLI once for
/// every config section.
pub(super) fn configured() -> harness::Configured {
    harness::Configured::new(
        &format!("{}/{VERSION}", env!("CARGO_PKG_VERSION")),
        vec![prefix().join("openclaw.json")],
    )
}

/// The release `openclaw doctor --fix` last set up this install for,
/// in the home's `.lnk`: a new machine's install runs it again.
pub(super) fn doctored() -> PathBuf {
    link_files::state::path(
        &link_files::home_dir(),
        link_files::state::Area::OpenclawDoctorRelease,
    )
}

pub(super) fn install(out: &mut Out) -> Result<Done, Report> {
    copy_self()?;
    if bin().is_file() {
        return Ok(Done::shown());
    }
    // A new install is configured anew, whatever the last one left.
    configured().forget();
    let _ = std::fs::remove_file(doctored());
    out.block(Note(sentence!(
        "Installing OpenClaw {} into {} (first time only).",
        VERSION.to_string(),
        Shown::new(prefix())
    )));
    // Its installer writes to this terminal; between its lines, for minutes
    // at a time, Link says it's still going.
    out.hand_over();
    let _still = harness::progress::Heartbeat::start("installing OpenClaw");
    // --runtime-only: no onboarding and no service of its own.
    harness::run_installer(
        &INSTALLER,
        &[
            "--prefix".as_ref(),
            programs_root().as_os_str(),
            "--runtime-only".as_ref(),
            "--version".as_ref(),
            VERSION.as_ref(),
        ],
    )?;
    if !bin().is_file() {
        return Err(Report::error(sentence!(
            "The OpenClaw installer finished, but {} is missing.",
            Shown::new(bin())
        ))
        .next(
            cmd!("lnk agent harness set openclaw"),
            "try the install again",
        ));
    }
    Ok(Done::shown())
}

/// `uninstall` (`link_core::uninstall`): it runs nothing of its own
/// and keeps nothing outside the agents' folders, which are the harness
/// module's (`agents`); what OpenClaw holds in each agent's home is
/// said there, for `lnk secret list`.
pub(super) fn uninstall(args: &link_core::uninstall::Flags) -> Result<Done, Report> {
    use link_core::uninstall as u;
    let kept = u::Kept {
        parts: vec![u::Part {
            name: "agents".into(),
            about: "your agents: their OpenClaw homes".into(),
            credentials: held(&harness::homes("openclaw")),
            ..Default::default()
        }],
        ..Default::default()
    };
    u::run(args, &kept, || Ok(()), |_| Ok(()))?;
    Ok(Done::shown())
}

/// What OpenClaw holds in each of `homes` (an agent's name and its
/// home), as `lnk secret list` shows it: its gateway's token, which
/// `configure` made. Its `.env` also holds copies of the agent's model key and
/// connections' tokens, which the harness module lists.
fn held(homes: &[(String, PathBuf)]) -> Vec<link_core::uninstall::Held> {
    use link_core::uninstall::Held;
    let mut held = Vec::new();
    for (agent, home) in homes {
        let prefix = prefix_in(home);
        let env = prefix.join(".env");
        if env.is_file() {
            held.push(Held {
                what: link_core::uninstall::CredentialKind::HarnessToken.into(),
                of: format!("{agent}'s OpenClaw gateway (LNK_OPENCLAW_GATEWAY_TOKEN)"),
                kept: format!("{} (only yours)", env.display()),
                may: format!(
                    "Calls {agent}'s OpenClaw gateway on this machine as its operator: chats, \
                     reads its conversations and changes its settings"
                ),
            });
        }
    }
    held
}
