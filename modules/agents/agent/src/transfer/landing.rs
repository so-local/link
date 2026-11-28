//! Where a move lands first: `~/.lnk/safe/agents/transfers/receiving/<agent id>`, its
//! record, what it holds, and the seed from what the agent left here.

use super::*;
use link_format::WithinExt;

/// `~/.lnk/safe/agents/transfers/receiving`: what's received of agents moving here,
/// out of every agent's reach, a folder each by the agent's ID.
pub fn moving_dir() -> PathBuf {
    link_files::layout::Hidden::Receiving.path()
}

/// Agent `id`'s staging: `~/.lnk/safe/agents/transfers/receiving/<id>`.
pub fn staging_of(id: &str) -> anyhow::Result<PathBuf> {
    link_machine::machine::check_id(id)?;
    Ok(moving_dir().join(id))
}

/// What a staging folder says of itself, beside what it holds
/// ([`RECORD`]), naming its schema.
#[derive(Debug, Serialize, Deserialize)]
pub struct Record {
    /// The move whose copy it holds.
    #[serde(rename = "move")]
    pub move_id: String,
    /// When that copy came, in Unix seconds.
    pub at: u64,
}

link_files::specification!(Record, include_str!("landing/record.schema.toml"));

/// Each move's staging record, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Record>()
}

/// The record in a staging folder.
pub const RECORD: &str = "move.json";

/// The record of staging folder `staging`, if it has one.
pub fn record_of(staging: &Path) -> Option<Record> {
    let bytes = std::fs::read(staging.join(RECORD)).ok()?;
    link_files::saved::parse_json(&bytes).ok()
}

/// Agent `id`'s staging folder, made private (0700) if it's missing, and
/// seeded from what it left here ([`seed`]); refused if it, or the folder
/// holding it, is a link.
fn staging(id: &str) -> anyhow::Result<PathBuf> {
    staging_in(&moving_dir(), id, true)
}

/// [`staging`] in the folder `moving`, seeded when it's new if `seeded`.
pub(super) fn staging_in(moving: &Path, id: &str, seeded: bool) -> anyhow::Result<PathBuf> {
    link_machine::machine::check_id(id)?;
    let dir = moving.join(id);
    for folder in [moving.to_path_buf(), dir.clone()] {
        if folder
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            bail!("{} is a link: not following it", folder.display());
        }
    }
    let new = !dir.exists();
    link_files::create_private_dir(&dir).within(format!("creating {}", dir.display()))?;
    if new
        && seeded
        && let Err(err) = seed(&dir, id)
    {
        link_format::aside(
            link_format::Report::warning(link_format::sentence!(
                "Not starting from what the agent left here."
            ))
            .why(link_format::Sentence::adopt(&format!("{err:#}"))),
        );
    }
    Ok(dir)
}

/// What agent `id`'s staging holds (`files/...`, `memory/...`), each
/// file's hash: what a copy to this machine needn't send. Made, and
/// seeded, if it's missing.
pub fn have(id: &str) -> anyhow::Result<Have> {
    held(&staging(id)?)
}

/// What the staging folder `dir` holds, each file's hash.
pub(super) fn held(dir: &Path) -> anyhow::Result<Have> {
    let mut have = stream::have_of(&dir.join("files"), "files")?;
    have.extend(stream::have_of(&dir.join("memory"), "memory")?);
    have.extend(stream::have_of(&dir.join("harness"), "harness")?);
    Ok(have)
}

/// How long ago an agent may have left for its files to start its
/// staging when it comes back: 30 days.
pub const SEED_DAYS: u64 = 30;

/// Seeds the new staging folder `dir` of agent `id` with the files it
/// left on this machine, when it left within [`SEED_DAYS`] and they're
/// kept: copies, never following a link.
fn seed(dir: &Path, id: &str) -> anyhow::Result<()> {
    let Some(note) = crate::moved::of_id(id)? else {
        return Ok(());
    };
    if crate::moved::now().saturating_sub(note.at) > SEED_DAYS * 86400 {
        return Ok(());
    }
    let Some(kept) = crate::moved::kept_files(&note) else {
        return Ok(());
    };
    seed_files(&kept, &dir.join("files"))
}

/// Copies what `kept` holds into `files`, as a move carries it: Home but
/// its folders named as one of the agent's own, and its own instructions,
/// skills and conversations beside Home's folders through their stores. Links
/// are copied as links, never followed; a folder more than
/// [`link_files::nofollow::DEEPEST`] deep refuses it.
pub(super) fn seed_files(
    kept: &link_agent_contract::folder::Folder,
    files: &Path,
) -> anyhow::Result<()> {
    link_files::create_private_dir(files)?;
    let into = Dir::open(files)?;
    if link_agent_contract::folder::is_folder(&kept.home) {
        let own = |path: &Path| {
            path.components().count() == 1
                && path
                    .to_str()
                    .is_some_and(|p| link_agent_contract::folder::seen_collection(p).is_some())
        };
        Dir::open(&kept.home)?.copy_into(
            &into,
            &Copying {
                skip: &own,
                ..Copying::private(Links::Keep)
            },
        )?;
    }
    let root = &kept.agent;
    let store = link_store_contract::store::file::Folder::at(root, "seed")?;
    let agent = link_store_contract::store::Agent {
        id: "seed".into(),
        owner: String::new(),
    };
    collections::read(
        &collections::Kept::new(agent, |_| &store),
        false,
        |path, bytes| {
            let Some(path) = path.strip_prefix("files/") else {
                return Ok(());
            };
            let path = files.join(path);
            if let Some(parent) = path.parent() {
                link_files::create_private_dir(parent)?;
            }
            link_files::write_private(&path, bytes)?;
            Ok(())
        },
    )?;
    Ok(())
}
