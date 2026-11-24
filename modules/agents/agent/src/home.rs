//! A hosted agent's Home, on the cloud account that hosts it, behind
//! that cloud's own key: `lnk storage push` and `pull` for it, which ask
//! this module ([`link_storage_contract::HOME`]). It goes through the account's
//! adapter (`agent-receive` and `agent-send` with phase `files`,
//! `link_agent_place`), as a stream of the move's format
//! (`link_agent_contract::moving`), each file with its hash: never
//! rclone, and never a key here. Only what the other end doesn't hold the
//! same is sent.

use link_format::WithinExt;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure};
use link_agent_contract::moving;
use link_agent_contract::moving::Have;
use link_agent_contract::moving::Phase;
use link_agent_place::{self as cloud, HostedAgent, Reply, Request};
use link_secretive::Duplicate;

use link_files::pattern::matches;

/// A hosted agent's Home, or a hosted team's folder (`team`), and the
/// folder here it's pushed from and pulled to.
#[derive(Debug, Clone, PartialEq)]
pub struct Folder {
    /// The cloud account that hosts it.
    pub account: String,
    /// The agent's ID, or the team's.
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub team: bool,
}

pub use link_storage_contract::Moved;
use link_storage_contract::{Home, Way};

/// The request for agent `folder`'s Home on `account`, phase `files`.
fn request(account: &cloud::Account, folder: &Folder) -> anyhow::Result<Request> {
    link_machine::machine::check_id(&folder.id)?;
    if folder.team {
        return Ok(Request {
            account: account.duplicate(),
            team: Some(cloud::teams::Action::Folder(
                cloud::teams::TeamActionFolder {
                    team: folder.id.clone(),
                },
            )),
            phase: Some(Phase::Files.name().into()),
            ..Default::default()
        });
    }
    Ok(Request {
        account: account.duplicate(),
        agent: Some(HostedAgent {
            id: folder.id.clone(),
            name: folder.name.clone(),
            ..HostedAgent::default()
        }),
        phase: Some(Phase::Files.name().into()),
        ..Request::default()
    })
}

/// The cloud account `name`, with its sign-in (`lnk cloud env`).
fn account(name: &str) -> anyhow::Result<cloud::Account> {
    cloud::account(name).within(format!(
        "no cloud account named {name:?} here: lnk cloud list lists yours"
    ))
}

/// Where a large file is copied aside to be hashed, and a pull is
/// unpacked before it's put in place.
fn spool() -> PathBuf {
    link_files::layout::Hidden::StoreScratch.path()
}

/// `lnk storage`'s [`link_storage_contract::HOME`]: `home`'s folder pushed
/// or pulled, and what moved. Its owner is an agent's ID (its Home) or
/// a hosted team's group's (its folder), as this module answers
/// [`link_storage_contract::FOLDERS`].
pub fn run(home: &Home) -> anyhow::Result<Moved> {
    let owned = &home.owned;
    link_machine::machine::check_name(&owned.name)?;
    let account = owned
        .account
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("{} is kept on no cloud account", owned.owner))?;
    link_machine::machine::check_shape(account)?;
    ensure!(
        owned.root.is_absolute(),
        "its folder here is an absolute path"
    );
    let team = match owned.owner.kind().as_str() {
        crate::owners::AGENT => false,
        crate::owners::GROUP => true,
        _ => bail!("{} isn't an owner this module keeps", owned.owner),
    };
    let folder = Folder {
        account: account.to_string(),
        id: owned.owner.id().to_string(),
        name: owned.name.clone(),
        root: owned.root.clone(),
        team,
    };
    Ok(match home.way {
        Way::Push => push(&folder, &home.paths)?,
        Way::Pull => {
            let patterns: Vec<String> = home
                .paths
                .iter()
                .filter(|p| !p.is_empty())
                .cloned()
                .collect();
            pull(&folder, &patterns)?
        }
    })
}

/// Pushes `paths` of hosted agent `folder` (relative to its folder here,
/// `""` for all of it) to its Home on the account that hosts it: the
/// adapter first says what Home holds, then only what's new or changed
/// goes.
pub fn push(folder: &Folder, paths: &[String]) -> anyhow::Result<Moved> {
    let account = account(&folder.account)?;
    ensure!(
        folder.root.is_dir(),
        "{} isn't there: put what {}'s Home gets in it, then push",
        folder.root.display(),
        folder.name
    );
    let mut child = cloud::spawn(&account.kind, "agent-receive", &request(&account, folder)?)?;
    let pushed = (|| -> anyhow::Result<Moved> {
        let mut output = std::io::BufReader::new(child.stdout.take().within("no output")?);
        let mut line = String::new();
        std::io::Read::take(&mut output, moving::MAX_HAVE).read_line(&mut line)?;
        let first: Reply = serde_json::from_str(&line).within(format!(
            "reading what lnk-{} agent-receive said Home holds",
            account.kind
        ))?;
        let have = first.have;
        moving::check_have(&have)?;
        let input = child.stdin.take().within("no input")?;
        let moved = write_files(input, folder, paths, &have)?;
        let mut last = String::new();
        output.read_line(&mut last)?;
        Ok(moved)
    })();
    let status = child.wait()?;
    let moved = pushed?;
    ensure!(
        status.success(),
        "lnk-{} agent-receive failed (above)",
        account.kind
    );
    Ok(moved)
}

/// Whose files a stream of phase `files` carries, as its manifest's
/// `agent` names them: an agent's Home by its workload, `agent:<id>`, or
/// a team's folder by its resource, `group:<id>`.
fn owner_json(folder: &Folder) -> serde_json::Value {
    match folder.team {
        true => serde_json::json!({"resource": link_agent_team::resource(&folder.id)}),
        false => serde_json::json!({"workload": crate::settings::ids::written(&folder.id)}),
    }
}

/// The stream of `paths` of `folder`, phase `files`, to `out`, but what
/// `have` holds the same.
fn write_files(
    out: impl Write,
    folder: &Folder,
    paths: &[String],
    have: &Have,
) -> anyhow::Result<Moved> {
    let manifest = serde_json::json!({
        "format": moving::PROTOCOL.current,
        "phase": Phase::Files,
        "move": moving::new_move_id()?,
        "version": env!("CARGO_PKG_VERSION"),
        "agent": owner_json(folder),
    });
    let only: Vec<PathBuf> = paths
        .iter()
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .collect();
    let only = (!only.is_empty() && paths.iter().all(|p| !p.is_empty())).then_some(only);
    let mut tar = tar::Builder::new(std::io::BufWriter::new(out));
    tar.follow_symlinks(false);
    moving::append_manifest(&mut tar, &serde_json::to_vec(&manifest)?)?;
    let files = moving::append_folder(
        &mut tar,
        &folder.root,
        "files",
        have,
        only.as_deref(),
        &spool(),
    )?;
    moving::append_end(&mut tar)?;
    tar.into_inner()?.flush()?;
    let here = moving::have_of(&folder.root, "files")?;
    let (same, sent): (Vec<String>, Vec<String>) = files.into_iter().partition(|path| {
        have.get(path)
            .is_some_and(|hash| here.get(path) == Some(hash))
    });
    Ok(Moved {
        sent: sent.iter().map(|p| strip(p)).collect(),
        same: same.len(),
    })
}

/// `files/a/b` as `a/b`.
fn strip(path: &str) -> String {
    path.strip_prefix("files/").unwrap_or(path).to_string()
}

/// Pulls hosted agent `folder`'s Home from the account that hosts it into
/// its folder here, what `patterns` match (none: all of it): the adapter
/// sends what differs from what's here, unpacked aside, checked, then put
/// in place, replacing what differs.
pub fn pull(folder: &Folder, patterns: &[String]) -> anyhow::Result<Moved> {
    let account = account(&folder.account)?;
    let here = moving::have_of(&folder.root, "files")?;
    let mut request = request(&account, folder)?;
    request.have = here.clone();
    let mut child = cloud::spawn(&account.kind, "agent-send", &request)?;
    drop(child.stdin.take());
    let aside = spool().join("pulling").join(&folder.id);
    link_files::create_private_dir(&spool())?;
    let _ = std::fs::remove_dir_all(&aside);
    let output = child.stdout.take().within("no output")?;
    let unpacked = moving::unpack(output, |manifest, head| {
        ensure!(head.phase == Phase::Files, "not a stream of Home's files");
        ensure!(
            manifest["agent"] == owner_json(folder),
            "the stream is another agent's"
        );
        Ok(aside.clone())
    });
    let status = child.wait()?;
    let placed = unpacked.and_then(|_| {
        ensure!(
            status.success(),
            "lnk-{} agent-send failed (above)",
            account.kind
        );
        place(&aside.join("files"), &folder.root, patterns)
    });
    let _ = std::fs::remove_dir_all(&aside);
    let sent = placed?;
    let same = here
        .keys()
        .map(|p| strip(p))
        .filter(|p| patterns.is_empty() || patterns.iter().any(|m| matches(m, p)))
        .filter(|p| !sent.contains(p))
        .count();
    Ok(Moved { sent, same })
}

/// Moves each file in `from` that `patterns` match (none: every one)
/// into the same place in `to`, replacing what's there; its folders
/// made. A link that came is left out; one on the way in `to` is
/// refused, never followed.
fn place(from: &Path, to: &Path, patterns: &[String]) -> anyhow::Result<Vec<String>> {
    let mut placed = Vec::new();
    if !from.is_dir() {
        return Ok(placed);
    }
    std::fs::create_dir_all(to)?;
    walk(from, from, to, patterns, &mut placed)?;
    placed.sort();
    Ok(placed)
}

fn walk(
    root: &Path,
    dir: &Path,
    to: &Path,
    patterns: &[String],
    placed: &mut Vec<String>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(root)?.to_path_buf();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(root, &path, to, patterns, placed)?;
            continue;
        }
        // Only files come: a link that came is never planted in Home.
        if !kind.is_file() {
            continue;
        }
        let shown = rel.to_string_lossy().into_owned();
        if !(patterns.is_empty() || patterns.iter().any(|m| matches(m, &shown))) {
            continue;
        }
        // Each folder on the way here, a folder: never a link.
        let mut at = to.to_path_buf();
        for part in rel.parent().into_iter().flat_map(|p| p.components()) {
            at.push(part);
            match std::fs::symlink_metadata(&at) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => bail!(
                    "{} is in the way of {shown} (a link? not following it)",
                    at.display()
                ),
                Err(_) => std::fs::create_dir(&at)?,
            }
        }
        let target = to.join(&rel);
        if std::fs::symlink_metadata(&target).is_ok_and(|m| m.is_dir()) {
            bail!("{} is a folder, where {shown} goes", target.display());
        }
        put(&path, &target).within(format!("putting {}", target.display()))?;
        placed.push(shown);
    }
    Ok(())
}

/// Moves file `from` to `to`, replacing it: renamed, or, from another
/// file system (the spool is in `~/.config`, Home may not be), copied
/// beside `to` first and renamed there, so `to` is never half written.
fn put(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            let beside = to.with_file_name(link_files::layout::temporary_name());
            std::fs::copy(from, &beside)?;
            std::fs::rename(&beside, to).inspect_err(|_| {
                let _ = std::fs::remove_file(&beside);
            })?;
            std::fs::remove_file(from)
        }
        done => done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_test_kit::TempDir;

    #[test]
    fn what_came_is_put_in_place_never_through_a_link() {
        let dir = TempDir::new("hosted-place");
        let (from, to, outside) = (dir.join("from"), dir.join("to"), dir.join("outside"));
        std::fs::create_dir_all(from.join("taxes")).unwrap();
        std::fs::create_dir_all(from.join("notes")).unwrap();
        std::fs::write(from.join("taxes/2025.txt"), "came").unwrap();
        std::fs::write(from.join("notes/a.md"), "came").unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(to.join("kept.txt"), "here").unwrap();
        assert_eq!(
            place(&from, &to, &["taxes".to_string()]).unwrap(),
            ["taxes/2025.txt"]
        );
        assert_eq!(
            std::fs::read_to_string(to.join("taxes/2025.txt")).unwrap(),
            "came"
        );
        assert!(!to.join("notes").exists());
        assert_eq!(
            std::fs::read_to_string(to.join("kept.txt")).unwrap(),
            "here"
        );
        // A link that came is left out, never planted.
        std::os::unix::fs::symlink(&outside, from.join("taxes/out")).unwrap();
        assert_eq!(
            place(&from, &to, &["taxes".to_string()]).unwrap(),
            Vec::<String>::new()
        );
        assert!(!to.join("taxes/out").exists() && to.join("taxes/out").symlink_metadata().is_err());
        std::os::unix::fs::symlink(&outside, to.join("notes")).unwrap();
        assert!(place(&from, &to, &[]).is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    }
}
