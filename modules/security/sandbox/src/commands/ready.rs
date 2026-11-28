//! `lnk sandbox check`'s first step: this machine set up to sandbox.

use link_format::{
    Foreign, Out, Path, Question, Report, Verbatim, WithinExt, cmd, prose, sentence,
};
use link_sandbox_confine::linux::bwrap::Blocked;

/// Makes sure the sandbox can run here, asking again: what was kept of
/// this machine forgotten first (`lnk sandbox check --ready`).
pub(crate) fn ready_again(out: &mut Out, ask: bool) -> Result<(), Report> {
    link_sandbox_confine::linux::kept::forget();
    match ask {
        true => ready(out, true),
        false => ready_unasked(out),
    }
}

/// A report for a machine the sandbox can't run on, and where one can.
fn no_sandbox(headline: link_format::Sentence) -> Report {
    Report::error(headline).next(cmd!("lnk box create"), "make a Linux box to run it on")
}

/// Bubblewrap isn't installed: installed with the system's package, where
/// sudo asks your password, never only where it needs none.
fn missing() -> Report {
    Report::error(sentence!(
        "The sandbox needs bubblewrap, which isn't installed."
    ))
    .why(prose!("Install your system's package for it (bubblewrap)."))
    .next(cmd!("sudo apt install bubblewrap"), "install it")
    .next(
        cmd!("lnk sandbox check --ready"),
        "check again once you have",
    )
}

/// Bubblewrap can't make the sandbox, and what it said.
fn blocked(why: &str) -> Report {
    Report::error(sentence!("Bubblewrap can't make the sandbox here."))
        .why(prose!("It said: {}.", Foreign::new(why)))
        .next(
            cmd!("lnk sandbox check --ready"),
            "allow it, in a terminal, where sudo asks your password",
        )
}

/// Makes sure the sandbox can run here. On Linux, where Ubuntu's AppArmor
/// rule stops bubblewrap, offers (`ask`, in a terminal) to allow it.
pub(crate) fn ready(out: &mut Out, ask: bool) -> Result<(), Report> {
    if cfg!(target_os = "macos") {
        let exec = std::path::Path::new("/usr/bin/sandbox-exec");
        if !exec.is_file() {
            return Err(no_sandbox(sentence!(
                "The sandbox needs {}, which this Mac doesn't have.",
                Path::new(exec)
            )));
        }
        return Ok(());
    }
    if !cfg!(target_os = "linux") {
        return Err(no_sandbox(sentence!("Link has no sandbox for this OS.")));
    }
    let bwrap = link_sandbox_confine::linux::bwrap::bwrap().map_err(|_| missing())?;
    // Asked once per boot, kernel and bubblewrap: each sandbox started
    // after reads that it worked.
    match link_sandbox_confine::linux::bwrap::worked(bwrap) {
        Ok(()) if !link_sandbox_confine::linux::bwrap::unproven_for_users() => return Ok(()),
        Ok(()) => {}
        Err(Blocked::Other(why)) => return Err(blocked(&why)),
        Err(Blocked::AppArmor) => {}
    }
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let allowed = ask
        && terminal
        && out.ask(Question::confirm(sentence!("Add it?"), true).why(prose!(
            "This system (Ubuntu 23.10 or newer) lets only programs AppArmor names make \
                 the user namespaces a sandbox needs. Link can name bubblewrap, which makes \
                 the sandbox, in {} (it needs sudo).",
            Path::new(link_sandbox_confine::linux::bwrap::APPARMOR_PROFILE)
        )))?;
    if !allowed {
        let mut lines = Verbatim::new();
        for line in link_sandbox_confine::linux::bwrap::apparmor_commands(bwrap).lines() {
            lines = lines.line(line);
        }
        out.block(lines);
        return Err(Report::error(sentence!(
            "The sandbox can't run until bubblewrap may make user namespaces."
        ))
        .why(prose!("Run the lines above, then try again."))
        .next(
            cmd!("lnk sandbox check --ready"),
            "check again once you have",
        ));
    }
    link_sandbox_confine::linux::bwrap::allow_bwrap(bwrap).within("allowing bubblewrap")?;
    link_sandbox_confine::linux::bwrap::works(bwrap).map_err(still_blocked)
}

/// Bubblewrap can't make the sandbox even after it was allowed.
fn still_blocked(why: Blocked) -> Report {
    Report::error(sentence!(
        "Bubblewrap still can't make the sandbox after that."
    ))
    .why(prose!("It said: {}.", Foreign::new(&format!("{why:?}"))))
}

/// [`ready`] without asking, where sudo needs no password (`sudo -n`), as
/// on a box: bubblewrap installed with apt if it's missing, apt fetching
/// a dropped download again ([`super::apt`]), and allowed
/// where AppArmor stops it.
pub(crate) fn ready_unasked(out: &mut Out) -> Result<(), Report> {
    if cfg!(target_os = "linux") && link_sandbox_confine::linux::bwrap::bwrap().is_err() {
        // apt writes to this terminal itself.
        let _hand = out.hand_over();
        let installed = link_secretive::process::command("sudo")
            .args(super::apt::install("bubblewrap").within("reading apt's options")?)
            .env("DEBIAN_FRONTEND", "noninteractive")
            .stdin(std::process::Stdio::null())
            .status()
            .within("running sudo apt-get")?;
        if !installed.success() {
            return Err(Report::error(sentence!("Couldn't install bubblewrap."))
                .why(prose!("The install with apt did not succeed."))
                .next(cmd!("sudo apt install bubblewrap"), "install it yourself"));
        }
    }
    if !cfg!(target_os = "linux") {
        return ready(out, false);
    }
    let bwrap = link_sandbox_confine::linux::bwrap::bwrap()?;
    let tried = link_sandbox_confine::linux::bwrap::works(bwrap);
    let unproven = tried.is_ok() && link_sandbox_confine::linux::bwrap::unproven_for_users();
    match tried {
        Err(Blocked::AppArmor) => allow_unasked(bwrap),
        Ok(()) if unproven => allow_unasked(bwrap),
        _ => ready(out, false),
    }
}

/// Link's AppArmor profile added with `sudo -n`, then bubblewrap tried again.
fn allow_unasked(bwrap: &str) -> Result<(), Report> {
    link_sandbox_confine::linux::bwrap::allow_bwrap_unasked(bwrap).within("allowing bubblewrap")?;
    link_sandbox_confine::linux::bwrap::works(bwrap).map_err(still_blocked)
}

#[cfg(test)]
mod tests {
    use link_format::render;

    /// What a missing or blocked bubblewrap says to do works where sudo
    /// asks a password: never only `--yes`, which needs one that doesn't.
    #[test]
    fn a_missing_bubblewrap_is_installed_where_sudo_asks() {
        for report in [super::missing(), super::blocked("denied")] {
            let said = render::report(&report, None, &render::Ctx::plain());
            assert!(!said.contains("--yes"), "{said}");
            assert!(said.contains("lnk sandbox check --ready"), "{said}");
        }
        let said = render::report(&super::missing(), None, &render::Ctx::plain());
        assert!(said.contains("sudo apt install bubblewrap"), "{said}");
    }
}
