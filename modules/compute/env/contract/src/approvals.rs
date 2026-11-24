//! Your yes to giving an environment you run a secret it asks for: an
//! approval, kept by this module in the Safe (`~/.lnk/safe/envs/
//! approvals.toml`), never by the secret module, which names no
//! environment. An environment you run acts as you, so it's given what
//! its spec asks for of the secrets you hold, each only once you've said
//! yes for that spec.
//!
//! A spec asks for a secret by name (`NPM_TOKEN`), which travels with the
//! project; the approval keeps the secret's ID, tied to the spec's real
//! path and its `name`. So a different spec at that path or a changed
//! `name` asks again, an edit to the rest of the spec doesn't, a renamed
//! secret keeps its approval and one made again with the name asks
//! again. A secret deleted since takes its approvals with it: each read
//! here leaves out, and the next write drops, an approval whose secret is
//! no longer kept.

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure};
use link_format::WithinExt;
use serde::{Deserialize, Serialize};

/// The kind a secret's ID is written with.
const SECRET: &str = "secret";

/// One yes: the secret `resource` (`secret:<id>`), which the spec at `spec`
/// named `name` asks for as `asks`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    /// The spec's real path.
    pub spec: PathBuf,
    /// The spec's `name`.
    pub name: String,
    /// The secret given, `secret:<id>`.
    pub resource: String,
    /// The name the spec asks for it by, a variable's.
    pub asks: String,
}

impl Approval {
    /// The secret's bare ID.
    pub fn secret_id(&self) -> &str {
        self.resource
            .strip_prefix(SECRET)
            .and_then(|rest| rest.strip_prefix(':'))
            .unwrap_or_default()
    }

    /// The command that gives it ahead, for a run no one can answer.
    pub fn command(asks: &str, spec: &Path) -> String {
        format!(
            "lnk env approval add {asks} --spec {}",
            crate::shell_quote(&spec.display().to_string())
        )
    }

    /// Whether it's for the spec at `spec` named `name`.
    pub fn is_for(&self, spec: &Path, name: &str) -> bool {
        self.spec == spec && self.name == name
    }
}

/// The file as it's kept.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept {
    #[serde(default, rename = "approval", skip_serializing_if = "Vec::is_empty")]
    approvals: Vec<Approval>,
}

/// Where the approvals are kept.
pub fn path() -> anyhow::Result<PathBuf> {
    Ok(link_files::layout::Hidden::Envs
        .try_path()?
        .join("approvals.toml"))
}

/// A spec's path as an approval keeps it: its real path, else as given
/// (a box's environment, read back from its capture, keeps its source's).
pub fn spec_path(spec: &Path) -> PathBuf {
    std::fs::canonicalize(spec).unwrap_or_else(|_| spec.to_path_buf())
}

/// The approvals at `file`, those whose secret `kept` says is still kept.
pub fn read_at(file: &Path, kept: &dyn Fn(&str) -> bool) -> anyhow::Result<Vec<Approval>> {
    Ok(load(file)?
        .approvals
        .into_iter()
        .filter(|a| kept(a.secret_id()))
        .collect())
}

fn load(file: &Path) -> anyhow::Result<Kept> {
    match std::fs::read_to_string(file) {
        Ok(text) => parse_kept(&text).within(format!("reading {}", file.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Kept::default()),
        Err(err) => Err(err).within(format!("reading {}", file.display())),
    }
}

/// The approvals `text` keeps, as `approvals.toml` holds them: each its
/// secret's ID, `secret:<id>`.
pub fn parse(text: &str) -> anyhow::Result<Vec<Approval>> {
    Ok(parse_kept(text)?.approvals)
}

link_files::specification!(Kept, include_str!("approvals.schema.toml"));

/// The approvals' file, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Kept>()
}

fn parse_kept(text: &str) -> anyhow::Result<Kept> {
    let kept: Kept = link_files::saved::parse(text)?;
    for approval in &kept.approvals {
        ensure!(
            link_id::is_candidate(approval.secret_id()),
            "{} isn't a secret's ID, secret:<id>",
            link_format::clean(&approval.resource)
        );
    }
    Ok(kept)
}

/// `change` made to the approvals at `file`, under its lock, those whose
/// secret is gone dropped: whether `change` changed one.
pub fn change_at(
    file: &Path,
    kept: &dyn Fn(&str) -> bool,
    change: impl FnOnce(&mut Vec<Approval>) -> anyhow::Result<bool>,
) -> anyhow::Result<bool> {
    if let Some(parent) = file.parent() {
        link_files::create_private_dir(parent).within(format!("making {}", parent.display()))?;
    }
    let _lock = link_files::lock_file(&file.with_extension("lock"))?;
    let before = load(file)?.approvals;
    let mut approvals: Vec<Approval> = before
        .iter()
        .filter(|a| kept(a.secret_id()))
        .cloned()
        .collect();
    let changed = change(&mut approvals)?;
    approvals.sort();
    approvals.dedup();
    if approvals != before {
        let text = link_files::saved::print(&Kept { approvals }).within("writing the approvals")?;
        link_files::write_private(file, text.as_bytes())
            .within(format!("writing {}", file.display()))?;
    }
    Ok(changed)
}

/// Adds `approval` at `file`: whether it's new.
pub fn add_at(
    file: &Path,
    kept: &dyn Fn(&str) -> bool,
    approval: Approval,
) -> anyhow::Result<bool> {
    ensure!(
        link_id::is_candidate(approval.secret_id()),
        "{} isn't a secret's ID",
        link_format::clean(&approval.resource)
    );
    change_at(file, kept, |approvals| {
        // One secret for each name a spec asks: a new yes replaces the
        // one before.
        let new = !approvals.contains(&approval);
        approvals
            .retain(|a| !(a.is_for(&approval.spec, &approval.name) && a.asks == approval.asks));
        approvals.push(approval);
        Ok(new)
    })
}

/// Removes the approvals at `file` of the spec at `spec` named `name` for
/// the secret whose ID is `secret`: whether there was one.
pub fn remove_at(
    file: &Path,
    kept: &dyn Fn(&str) -> bool,
    spec: &Path,
    name: &str,
    secret: &str,
) -> anyhow::Result<bool> {
    change_at(file, kept, |approvals| {
        let before = approvals.len();
        approvals.retain(|a| !(a.is_for(spec, name) && a.secret_id() == secret));
        Ok(approvals.len() != before)
    })
}

/// One of your secrets, as an approval names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Yours {
    pub id: String,
    pub name: String,
}

/// Your named secrets kept now, by the secret module.
pub fn yours() -> anyhow::Result<Vec<Yours>> {
    Ok(link_secret_contract::list()?
        .into_iter()
        .filter(|l| l.kept)
        .filter_map(|l| {
            Some(Yours {
                name: l.name?,
                id: l.id,
            })
        })
        .collect())
}

/// The secret of `yours` that `text`, a name or `secret:<id>`, names.
pub fn find<'a>(yours: &'a [Yours], text: &str) -> anyhow::Result<&'a Yours> {
    let by_id = text
        .strip_prefix(SECRET)
        .and_then(|rest| rest.strip_prefix(':'));
    let found = yours.iter().find(|y| match by_id {
        Some(id) => y.id == id,
        None => y.name == text,
    });
    match found {
        Some(found) => Ok(found),
        None => bail!(
            "no secret {} is kept here: lnk secret list lists them",
            link_format::clean(text)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(spec: &Path, name: &str, id: &str, asks: &str) -> Approval {
        Approval {
            spec: spec.to_path_buf(),
            name: name.into(),
            resource: format!("secret:{id}"),
            asks: asks.into(),
        }
    }

    /// An approval is tied to the spec's path, its name and the secret:
    /// another of any of them isn't approved, and a secret deleted since
    /// takes its approvals with it.
    #[test]
    fn an_approval_is_its_spec_name_and_secret() {
        let root = link_test_kit::TempDir::new("env-approvals");
        let file = root.join("approvals.toml");
        let spec = root.join("env.toml");
        let (id, other) = (link_id::candidate().unwrap(), link_id::candidate().unwrap());
        let all = |_: &str| true;
        assert!(add_at(&file, &all, approval(&spec, "web", &id, "NPM_TOKEN")).unwrap());
        assert!(!add_at(&file, &all, approval(&spec, "web", &id, "NPM_TOKEN")).unwrap());
        let read = read_at(&file, &all).unwrap();
        assert_eq!(read, [approval(&spec, "web", &id, "NPM_TOKEN")]);
        assert!(read[0].is_for(&spec, "web"));
        assert!(!read[0].is_for(&spec, "api"));
        assert!(!read[0].is_for(&root.join("copy.toml"), "web"));
        // A new yes for the name replaces the one before.
        add_at(&file, &all, approval(&spec, "web", &other, "NPM_TOKEN")).unwrap();
        assert_eq!(
            read_at(&file, &all).unwrap(),
            [approval(&spec, "web", &other, "NPM_TOKEN")]
        );
        // A deleted secret's approval is left out, and dropped at the
        // next write.
        let gone = |kept: &str| kept != other;
        assert!(read_at(&file, &gone).unwrap().is_empty());
        add_at(&file, &gone, approval(&spec, "web", &id, "API_KEY")).unwrap();
        assert_eq!(
            read_at(&file, &all).unwrap(),
            [approval(&spec, "web", &id, "API_KEY")]
        );
        assert!(remove_at(&file, &all, &spec, "web", &id).unwrap());
        assert!(!remove_at(&file, &all, &spec, "web", &id).unwrap());
        assert!(read_at(&file, &all).unwrap().is_empty());
        // Never an ID Link didn't mint.
        assert!(add_at(&file, &all, approval(&spec, "web", "x", "A")).is_err());
    }

    #[test]
    fn a_secret_is_found_by_its_name_or_its_id() {
        let id = link_id::candidate().unwrap();
        let yours = [Yours {
            id: id.clone(),
            name: "NPM_TOKEN".into(),
        }];
        assert_eq!(find(&yours, "NPM_TOKEN").unwrap().id, id);
        assert_eq!(
            find(&yours, &format!("secret:{id}")).unwrap().name,
            "NPM_TOKEN"
        );
        assert!(find(&yours, "OTHER").is_err());
    }
}
