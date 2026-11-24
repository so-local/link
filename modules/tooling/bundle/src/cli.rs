//! `lnk bundle`'s command line, its bundle spec and its outputs,
//! generated from the module's declarations (`actions.toml` and
//! `types.toml`), read from flags or a spec file alike (`link_spec`).

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

// A bundle with no build named is a dev build.
#[expect(
    clippy::derivable_impls,
    reason = "the enum is generated, so derives nothing"
)]
impl Default for Build {
    fn default() -> Self {
        Build::Dev
    }
}

impl Build {
    pub fn word(self) -> &'static str {
        match self {
            Build::Dev => "dev",
            Build::Staging => "staging",
            Build::Prod => "prod",
        }
    }
}

impl Target {
    /// This computer's target, which `host` stands for.
    pub fn here() -> anyhow::Result<Target> {
        let here = link_core::release::target()?;
        <Target as link_actions::value::Words>::from_word(here)
            .ok_or_else(|| anyhow::anyhow!("{here} isn't a target a bundle builds"))
    }

    /// `host` as this computer's target; any other as itself.
    pub fn resolved(self) -> anyhow::Result<Target> {
        match self {
            Target::Host => Target::here(),
            target => Ok(target),
        }
    }

    /// Link's release targets, each an archive's name ends with: every
    /// target but `host`.
    pub fn released() -> impl Iterator<Item = Target> {
        <Target as link_actions::value::Words>::all().filter(|t| *t != Target::Host)
    }

    /// The architecture it builds for; none for `host` until it's
    /// resolved.
    pub fn architecture(self) -> Option<Architecture> {
        match self {
            Target::Host => None,
            Target::Aarch64AppleDarwin | Target::Aarch64UnknownLinuxMusl => {
                Some(Architecture::Arm64)
            }
            Target::X8664AppleDarwin | Target::X8664UnknownLinuxMusl => Some(Architecture::X8664),
        }
    }
}

impl Architecture {
    /// Its Linux release target, which boxes run.
    pub fn linux(self) -> Target {
        match self {
            Architecture::X8664 => Target::X8664UnknownLinuxMusl,
            Architecture::Arm64 => Target::Aarch64UnknownLinuxMusl,
        }
    }
}

/// Reads this process's command line: its command and the field flags of
/// the spec it binds.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk bundle`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}
