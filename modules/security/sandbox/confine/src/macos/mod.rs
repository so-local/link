//! macOS: a Seatbelt profile run by `/usr/bin/sandbox-exec`, denying by
//! default ([`seatbelt`]), and the programs it refused, read from the
//! system log ([`reports`]).

pub mod reports;
pub mod seatbelt;
#[cfg(target_os = "macos")]
pub mod watch;

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use crate::{Backend, Rules};

/// The macOS backend.
pub struct MacOs;

impl Backend for MacOs {
    fn os(&self) -> crate::Os {
        crate::Os::MacOs
    }

    fn wrap(&self, rules: &Rules, kept: &Path, argv: &[String]) -> anyhow::Result<Vec<String>> {
        link_files::layout::prepare()?;
        std::fs::write(kept, seatbelt::profile(&rules.common, &rules.macos))?;
        let mut wrapped = vec![
            "/usr/bin/sandbox-exec".to_string(),
            "-f".into(),
            kept.display().to_string(),
        ];
        wrapped.extend(argv.iter().cloned());
        // Only the programs it lists: a refused one named, from outside,
        // by what Seatbelt reports in the system log.
        if let Some(programs) = &rules.common.commands
            && let Ok(exe) = std::env::current_exe().and_then(|e| e.canonicalize())
        {
            return Ok(refusals(&exe, programs, wrapped));
        }
        Ok(wrapped)
    }

    fn enter(
        &self,
        wall: Option<&crate::Wall>,
        only: Option<&[PathBuf]>,
        argv: &[String],
    ) -> anyhow::Result<Infallible> {
        let _ = (wall, only);
        anyhow::bail!("a sandbox's first program runs on Linux only: {argv:?} not run")
    }

    fn learn(
        &self,
        argv: &[String],
        env: &[(String, String)],
    ) -> anyhow::Result<(ExitStatus, String)> {
        #[cfg(target_os = "macos")]
        return watch::learn(argv, env);
        #[allow(
            unreachable_code,
            reason = "unreachable on macOS, where the call above returns"
        )]
        {
            let _ = (argv, env);
            anyhow::bail!("lnk sandbox learn on macOS reads its log, here on macOS only")
        }
    }

    fn refusals(&self, programs: &[PathBuf], argv: &[String]) -> anyhow::Result<ExitStatus> {
        #[cfg(target_os = "macos")]
        return watch::refusals(programs, argv);
        #[allow(
            unreachable_code,
            reason = "unreachable on macOS, where the call above returns"
        )]
        {
            let _ = (programs, argv);
            anyhow::bail!("sandbox refusals reads macOS's log, here on macOS only")
        }
    }
}

/// `wrapped` (`sandbox-exec` and the program) under `exe`'s `sandbox
/// refusals`, which runs it and says which program `programs` (files, or
/// folders of them) refused, from Seatbelt's reports.
pub fn refusals(exe: &Path, programs: &[PathBuf], wrapped: Vec<String>) -> Vec<String> {
    let mut outer = vec![exe.display().to_string(), "refusals".into()];
    for program in programs {
        outer.extend(["--exec".into(), program.display().to_string()]);
    }
    outer.push("--".into());
    outer.extend(wrapped);
    outer
}
