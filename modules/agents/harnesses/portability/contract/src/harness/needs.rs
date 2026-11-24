//! What a harness needs beyond Link's defaults ([`Needs`]): permissions,
//! no sandbox at all, or a risk of its own, each with why.

use super::*;

/// What `lnk-<name> needs` prints: what its harness needs beyond Link's
/// defaults, each a sandbox's spec ([`crate::sandbox::Spec`]) with why,
/// in the harness's words. Link asks the user for them before the
/// harness runs, and remembers the answer. It may depend on the settings
/// (one model provider's runtime and not another's), which it reads on
/// stdin as `configure` does, without keys or tokens.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Needs {
    /// What it needs, each a spec's sections: `{"spec": {"network":
    /// {"mode": "host"}}, "why": "..."}`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<Need>,
    /// Only for a harness that can't run in any sandbox: why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsandboxed: Option<Why>,
    /// What running it risks that no spec names (a port of its own that
    /// asks for no credential, where every user of the machine reaches
    /// it), each by a name of the harness's own. Link asks the user as it
    /// asks for a need, and hands the adapter those allowed
    /// ([`Settings::allowed`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<Risk>,
    /// Whether its install runs the programs it downloads into its own
    /// folder while it installs (a runtime it unpacks there, then runs).
    /// Link then lets that one step run what it writes in the folder it
    /// installs into, and nothing else it writes; once installed, the
    /// folder is read only to every run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub install_runs_downloads: bool,
}

/// A risk a harness names in [`Needs::risks`], and why.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Risk {
    /// Lowercase letters, digits and dashes, at most [`MAX_RISK_NAME`]
    /// long, and no name `lnk agent permission add` or `lnk agent harness-permission remove`
    /// takes for anything else (a permission, `unsandboxed`, `host`, `tcp`,
    /// `tool`, `calls`).
    pub name: String,
    pub why: String,
    /// For a risk the harness runs without, what it does then (`its
    /// browser stays off`): it runs whatever the answer, and without a
    /// yes the adapter leaves out what risks it. Without this, the
    /// harness doesn't run until the user allows it, as for a permission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub without: Option<String>,
}

/// The longest name of a [`Risk`].
pub const MAX_RISK_NAME: usize = 32;

/// The names no [`Risk`] takes: `unsandboxed`, never a grant, and the
/// sandbox's own settings' names, so `allow lan` is never a harness's
/// own risk.
const NOT_RISKS: &[&str] = &[
    "unsandboxed",
    "network",
    "developer-tools",
    "open",
    "read-home",
    "localhost",
    "lan",
    "host",
    "tcp",
    "tool",
    "calls",
];

/// Whether `name` can name a [`Risk`].
pub fn is_risk_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_RISK_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !NOT_RISKS.contains(&name)
}

/// One thing a harness needs, as a sandbox's spec, and why.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Need {
    pub spec: crate::sandbox::Spec,
    pub why: String,
}

/// The toolchains a harness's tools' commands run, as `needs.toml`
/// declares them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tools {
    toolchains: Vec<crate::sandbox::spec::Toolchain>,
}

fn tools() -> Tools {
    toml::from_str(include_str!("needs.toml")).expect("the harness needs.toml")
}

impl Need {
    /// The programs of a harness that runs what it installed and the
    /// commands its tools run: everything in `installed` (its own folder,
    /// absolute), and the toolchains its tools use (`needs.toml`), each
    /// named, so the grant says what it lets run.
    pub fn runs(installed: &std::path::Path, why: &str) -> Self {
        let mut folder = installed.display().to_string();
        if !folder.ends_with('/') {
            folder.push('/');
        }
        Need {
            spec: crate::sandbox::Spec {
                commands: Some(crate::sandbox::spec::Commands {
                    allow: Some(vec![folder]),
                    toolchains: Some(tools().toolchains),
                    ..Default::default()
                }),
                ..Default::default()
            },
            why: why.into(),
        }
    }
}

/// Why a harness needs what it does.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Why {
    pub why: String,
}

/// The longest `why` Link shows: a line, not a page.
pub const MAX_WHY: usize = 200;

impl Needs {
    /// What it needs, each spec as it is on this machine ([`Os::here`]),
    /// with why, a spec that opens nothing here left out; an error for a
    /// spec with a relative path, a name or a version (a need is
    /// sections, read from no folder), one that opens nothing on any OS,
    /// one with a limit of calls (`per_minute`: a need opens, and a limit
    /// is the agent's own), a `why` (or a risk's `without`) that's empty,
    /// longer than [`MAX_WHY`] or holds a control character, a risk named
    /// twice, or a risk whose name [`is_risk_name`] refuses.
    ///
    /// [`Os::here`]: crate::sandbox::spec::Os::here
    pub fn checked(&self) -> anyhow::Result<Vec<(crate::sandbox::Spec, String)>> {
        let why = |why: &str| -> anyhow::Result<()> {
            anyhow::ensure!(
                !why.trim().is_empty()
                    && why.chars().count() <= MAX_WHY
                    && !why.chars().any(char::is_control),
                "a need's why is one line of at most {MAX_WHY} characters"
            );
            Ok(())
        };
        if let Some(unsandboxed) = &self.unsandboxed {
            why(&unsandboxed.why)?;
        }
        let mut out = Vec::new();
        for need in &self.needs {
            why(&need.why)?;
            let spec = &need.spec;
            anyhow::ensure!(
                spec.name.is_none() && spec.version.is_none(),
                "a need's spec is its sections, with no name or version"
            );
            if let Some(path) = spec.relative_path() {
                anyhow::bail!(
                    "a need's spec names {:?}: each path absolute, or from ~",
                    link_format::clean(path)
                );
            }
            use crate::sandbox::spec::Os;
            let (linux, macos) = (spec.on(Os::Linux)?, spec.on(Os::MacOs)?);
            // Each list as a sandbox takes it, and no program standing
            // for any.
            spec.check_any_alone()?;
            anyhow::ensure!(
                [&linux, &macos]
                    .iter()
                    .any(|s| **s != crate::sandbox::Spec::default()),
                "a need's spec opens nothing"
            );
            anyhow::ensure!(
                [&linux, &macos]
                    .iter()
                    .all(|s| s.internet.as_ref().is_none_or(|i| i.per_minute.is_none())),
                "a need's spec has a limit of calls (per_minute), which is the agent's own \
                 (lnk agent permission add --internet.per_minute): a need names what it opens"
            );
            let here = match Os::here() {
                Os::Linux => linux,
                Os::MacOs => macos,
            };
            if here == crate::sandbox::Spec::default() {
                continue;
            }
            out.push((here, need.why.clone()));
        }
        for (i, risk) in self.risks.iter().enumerate() {
            anyhow::ensure!(
                is_risk_name(&risk.name),
                "it names a risk {:?}, which isn't a name Link takes for one",
                link_format::clean(&risk.name)
            );
            why(&risk.why)?;
            if let Some(without) = &risk.without {
                why(without)?;
            }
            anyhow::ensure!(
                !self.risks[..i].iter().any(|r| r.name == risk.name),
                "it names {} twice",
                risk.name
            );
        }
        Ok(out)
    }
}
