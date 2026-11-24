//! Linux: bubblewrap ([`bwrap`]), the root read only, the home folder and
//! `/run` replaced by empty ones with only the allowed folders mounted
//! back, `/etc` shown in part ([`etc`]), its own processes, IPC and
//! (but for [`Rules::network`]) network; inside, Landlock ([`landlock`])
//! and a seccomp filter ([`seccomp`]) that hands each exec to the watch
//! outside ([`execs`]); and what a mount needs made first, made and
//! removed around the run ([`mounts`]).

pub mod bwrap;
pub mod caps;
#[cfg(target_os = "linux")]
pub mod enter;
pub mod etc;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub mod execs;
#[cfg(target_os = "linux")]
pub mod home_sockets;
pub mod kept;
pub mod landlock;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub mod learn;
pub mod mounts;
#[cfg(target_os = "linux")]
mod pins;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub mod seccomp;
#[cfg(target_os = "linux")]
pub mod session;
#[cfg(target_os = "linux")]
pub mod workers;

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use crate::{Backend, Rules};

pub use caps::Capabilities;

/// The Linux backend.
pub struct Linux;

impl Backend for Linux {
    fn os(&self) -> crate::Os {
        crate::Os::Linux
    }

    fn wrap(&self, rules: &Rules, kept: &Path, argv: &[String]) -> anyhow::Result<Vec<String>> {
        link_files::layout::prepare()?;
        anyhow::ensure!(
            rules.linux.confine.is_some(),
            "the sandbox's first program is missing; target not started"
        );
        crate::dotfiles::refuse_links(&rules.common.outside)?;
        // Every closed folder in Link's own roots exists, so a mount
        // covers it however late what's in it is made.
        let roots = [
            link_files::layout::Root::Home.try_path()?,
            link_files::layout::Root::Hidden.try_path()?,
        ]
        .map(|root| crate::resolve(&root));
        bwrap::make_closed(&rules.common.closed, &roots)?;
        let args = bwrap::args(bwrap::bwrap()?, &rules.common, &rules.linux, argv);
        #[cfg(target_os = "linux")]
        let args = workers::wrapping(rules, kept, args)?;
        #[cfg(not(target_os = "linux"))]
        let _ = kept;
        // A mount covers only what's there: what isn't is made empty for
        // the run, by this program in front of bubblewrap, and removed
        // after.
        let wanted = crate::dotfiles::wanted(&rules.common.outside);
        // In Link's data folders nothing is made: what a program made
        // there is removed by the same stage when the command ends.
        let swept = crate::dotfiles::swept(&rules.common.outside);
        // The launch stage also pins mount sources, including commands
        // saved before an attacker changed a path to a symbolic link.
        let exe = std::env::current_exe()?.canonicalize()?;
        let dir = link_files::layout::Hidden::Sandboxes.try_path()?;
        Ok(mounts::wrapping(&exe, &dir, &wanted, &swept, args))
    }

    fn enter(
        &self,
        wall: Option<&crate::Wall>,
        only: Option<&[PathBuf]>,
        argv: &[String],
    ) -> anyhow::Result<Infallible> {
        #[cfg(target_os = "linux")]
        return enter::enter(wall, only, argv);
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (wall, only);
            anyhow::bail!("a sandbox's first program runs on Linux only: {argv:?} not run")
        }
    }

    fn learn(
        &self,
        argv: &[String],
        env: &[(String, String)],
    ) -> anyhow::Result<(ExitStatus, String)> {
        #[cfg(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        return learn::learn(argv, env);
        #[allow(
            unreachable_code,
            reason = "unreachable on Linux on x86_64 and arm64, where the call above returns"
        )]
        {
            let _ = (argv, env);
            anyhow::bail!("lnk sandbox learn runs on macOS, and Linux on x86_64 and arm64")
        }
    }

    fn refusals(&self, _programs: &[PathBuf], _argv: &[String]) -> anyhow::Result<ExitStatus> {
        anyhow::bail!("sandbox refusals reads macOS's log; on Linux the sandbox names them itself")
    }
}

/// Whether the seccomp filter (and the exec watch, which rides on it)
/// is built for this machine: Linux on x86_64 and aarch64. Elsewhere on
/// Linux a sandbox's first program still closes what it was handed
/// ([`confiner`]), and `[commands]` lists are refused.
pub const FILTERED: bool = cfg!(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
));

/// Refuse an effective command policy unless this process can enforce
/// it. `Programs::Any` needs no executable restriction. An empty
/// `Programs::Only` is deny by default and still needs enforcement.
pub fn check_commands(
    commands: &link_sandbox_client::Programs,
    confiner: bool,
) -> anyhow::Result<()> {
    check_commands_with(commands, FILTERED, confiner, landlock::probe)
}

fn check_commands_with(
    commands: &link_sandbox_client::Programs,
    filtered: bool,
    confiner: bool,
    probe: impl FnOnce() -> Result<std::num::NonZeroU32, landlock::ProbeError>,
) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use link_sandbox_client::Programs;

    let Programs::Only(programs) = commands else {
        return Ok(());
    };
    let policy = if programs.is_empty() {
        "effective command policy grants no programs (deny by default)"
    } else {
        "effective command policy permits only listed programs"
    };
    ensure!(
        confiner,
        "{policy}; executable restrictions need the in-sandbox confiner, which is missing; target not started"
    );
    ensure!(
        filtered,
        "{policy}; executable restrictions need the seccomp filter, which is unsupported on this architecture; target not started"
    );
    probe().with_context(|| format!("{policy}; executable restrictions need Landlock, which is unavailable; target not started"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_sandbox_client::{
        Confinement, Programs, Spec,
        confinement::{At, Way},
    };

    fn available() -> Result<std::num::NonZeroU32, landlock::ProbeError> {
        Ok(std::num::NonZeroU32::new(6).unwrap())
    }

    fn unavailable() -> Result<std::num::NonZeroU32, landlock::ProbeError> {
        Err(std::io::Error::from_raw_os_error(libc::ENOSYS).into())
    }

    #[test]
    fn every_wrapped_launch_requires_the_descriptor_closing_confiner() {
        let error = Linux
            .wrap(&Rules::default(), Path::new("/unused"), &["true".into()])
            .unwrap_err();
        assert!(error.to_string().contains("first program is missing"));
        assert!(error.to_string().contains("target not started"));
    }

    fn commands(spec: serde_json::Value) -> anyhow::Result<Programs> {
        let spec: Spec = serde_json::from_value(spec)?;
        // A file goes through the shared strict reader too: an empty
        // section is invalid, while a section omitted from the spec is
        // a valid deny-by-default policy.
        let spec: Spec = link_spec::parse(&link_spec::print(&spec)?)?;
        // Resolve OS overrides explicitly so these cases test the same
        // effective Linux policy on any host.
        let spec = spec.on(link_sandbox_contract::spec::Os::Linux)?;
        Ok(Confinement::of(
            &spec,
            At {
                name: "capability-test",
                dir: Path::new("/project"),
                home: Path::new("/home/test-user"),
                path: None,
                way: Way::Loopback("/run/sandbox-test".into()),
            },
        )?
        .commands)
    }

    #[test]
    fn effective_policies_require_only_the_capabilities_they_use() {
        use serde_json::json;

        // Every grant names its programs, so every policy is restricted.
        for (spec, restricted, none) in [
            (json!({"commands":{"allow":["/usr/bin/true"]}}), true, false),
            (json!({}), true, true),
            (
                json!({"commands":{"allow":["/bin/sh"],"linux":{"allow":["/usr/bin/true"]}}}),
                true,
                false,
            ),
            (
                json!({"commands":{"macos":{"allow":["/usr/bin/true"]}}}),
                true,
                true,
            ),
        ] {
            let policy = commands(spec.clone()).unwrap();
            assert_eq!(
                matches!(&policy, Programs::Only(p) if p.is_empty()),
                none,
                "{spec}"
            );
            check_commands_with(&policy, true, true, available).unwrap();
            let result = check_commands_with(&policy, true, true, unavailable);
            assert_eq!(result.is_err(), restricted, "{spec}: {result:?}");
            if let Err(error) = result {
                let error = format!("{error:#}");
                assert!(
                    error.contains(if none {
                        "grants no programs"
                    } else {
                        "only listed programs"
                    }),
                    "{error}"
                );
                assert!(
                    error.contains("syscall unavailable") && error.contains("target not started"),
                    "{error}"
                );
            }
        }
        for invalid in [
            json!({"commands":{"allow":[]}}),
            json!({"commands":{}}),
            json!({"commands":{"developer_tools":true}}),
        ] {
            assert!(commands(invalid.clone()).is_err(), "{invalid}");
        }
    }

    #[test]
    fn missing_confiner_or_filter_is_not_a_landlock_failure() {
        for policy in [
            Programs::Only(vec![]),
            Programs::Only(vec!["/usr/bin/true".into()]),
        ] {
            for (filtered, confiner, wanted) in
                [(true, false, "confiner"), (false, true, "architecture")]
            {
                let error = check_commands_with(&policy, filtered, confiner, || {
                    panic!("no Landlock probe needed")
                })
                .unwrap_err()
                .to_string();
                assert!(
                    error.contains(wanted) && !error.contains("Landlock"),
                    "{error}"
                );
            }
        }
        check_commands_with(&Programs::Any, false, false, || {
            panic!("unrestricted programs need no Landlock probe")
        })
        .unwrap();
    }
}
