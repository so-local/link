//! `lnk agent backup` and its restore: an agent's files and state in
//! one archive, and back in place from it.

use std::process::Stdio;

use anyhow::{bail, ensure};
use link_agent::adapter::Adapter;
use link_agent::layout;
use link_agent::moved;
use link_agent::service;
use link_agent::settings::{self, Settings};
use link_agent::transfer::{self};
use link_files::nofollow::{Dir, Kind, Links};
use link_format::{
    Done, Exit, Foreign, Name, Note, Out, Path, Question, Report, Sentence, Text, WithinExt, cmd,
    prose, sentence,
};

use super::buckets::a_cloud;
use super::state_of;

/// `lnk agent restore`: agent `name` (`id` when two have it) back here
/// from its bucket, found by its labels in your cloud accounts: a key
/// reaching only its buckets, its files pulled, and its harness's state
/// from its last backup (`Backups/<harness>.tar`) unpacked in the
/// harness's home. Returns its settings, with no model yet.
pub fn restore(
    out: &mut Out,
    name: &str,
    id: Option<&str>,
) -> anyhow::Result<(Settings, Sentence)> {
    link_machine::machine::check_name(name)?;
    if let Some(id) = id {
        link_machine::machine::check_id(id)?;
    }
    ensure!(
        !Settings::path_of(name).exists(),
        "there's an agent named {name} here already"
    );
    // Its secrets are granted by its ID: no other agent here may have it,
    // nor lose them if this restore fails.
    let not_another = |id: &str| -> anyhow::Result<()> {
        for other in settings::names()?.iter().filter(|n| *n != name) {
            ensure!(
                Settings::load(other)?.agent_id() != Some(id),
                "the buckets restored are {other}'s, an agent here already: not restored as \
                 {name}"
            );
        }
        Ok(())
    };
    if let Some(id) = id {
        not_another(id)?;
    }
    link_core::ensure("storage", "lnk agent restore")?;
    // The storage module finds its buckets, asks their password, puts them
    // in place and pulls its files; the rest is the agent's.
    let settings_of = Settings::new(name);
    let files = settings_of.files();
    let made = !settings_of.dir().exists();
    link_files::create_private_dir(&settings_of.dir())?;
    // By its ID when it's known (two agents may have had the name), else
    // by its name: `lnk storage` finds its buckets by their labels.
    let mut restore = link_core::lnk();
    restore
        .args(["storage", "restore", "--owner"])
        .arg(format!(
            "{}:{}",
            link_agent::owners::AGENT,
            id.unwrap_or(name)
        ))
        .arg("--folder")
        .arg(&files);
    out.hand_over();
    let output = restore
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .within("running lnk storage restore")?;
    ensure!(
        output.status.success(),
        "{name} couldn't be restored from its buckets (above)"
    );
    let restored: serde_json::Value =
        serde_json::from_slice(&output.stdout).within("reading lnk storage restore")?;
    let owner: link_storage_contract::Owner = restored["owner"]
        .as_str()
        .within("lnk storage restore named no owner")?
        .parse()?;
    ensure!(
        owner.kind().as_str() == link_agent::owners::AGENT,
        "lnk storage restore restored {owner}, not an agent's buckets"
    );
    let id = owner.id().to_string();
    link_machine::machine::check_id(&id)?;
    if let Err(err) = not_another(&id) {
        // What it pulled is the other agent's: not left here.
        if made {
            let _ = std::fs::remove_dir_all(settings_of.dir());
        }
        return Err(err);
    }
    // A machine that's gone: it runs here from now on.
    if let Some(note) = moved::of_id(&id)? {
        out.block(Report::warning(sentence!(
            "{} went to {} from this computer: it runs here again, and a copy still there would \
             answer its bot too.",
            Name::new("agent", name),
            Foreign::new(&note.place())
        )));
        note.remove();
    }
    link_agent::settings::ids::restore(&id)?;
    if let Some(service) = &settings_of.service {
        link_agent::settings::ids::SERVICES.restore(service)?;
    }
    let mut settings = settings_of;
    settings.id = Some(link_agent::settings::ids::workload(&id)?);
    layout::lay_out(&settings)?;
    settings.save()?;
    let restored = unpack_backup(&settings);
    match restored {
        Ok(harness) => {
            settings.harness = Some(harness.clone());
            settings.save()?;
            out.block(Note(sentence!(
                "Its files are in {}.",
                Path::new(settings.files())
            )));
            let restored = sentence!(
                "Restored {}: its files and {}'s memory are back.",
                Name::new("agent", name),
                Name::new("harness", &harness)
            );
            Ok((settings, restored))
        }
        Err(err) => {
            settings.forget_keys_or_warn();
            let _ = std::fs::remove_file(settings.path());
            Err(err)
        }
    }
}

/// Unpacks the harness's state from the files folder's backup
/// (`Backups/<harness>.tar`) into its home, and the agent's own
/// folder (`Backups/Agent.tar`, when the backup holds one); returns
/// the harness.
///
/// The files folder came from a bucket, written by the agent's harness,
/// so the archive's name picks only a harness there is to use here
/// ([`settings::harnesses`]: Link's, or one added from outside Link; the
/// first by name when several are there),
/// and of the archive only what that harness's adapter says is its state
/// comes, through staging ([`transfer::replace_state`]): never its
/// install or `.env`. It's opened without following a link.
pub(crate) fn unpack_backup(settings: &Settings) -> anyhow::Result<String> {
    let folder = settings.files().join(BACKUP);
    let none = || format!("no backup of its harness in {}", folder.display());
    let dir = open_backup_folder(&settings.files())
        .within(format!("{} (a folder, not a link)", none()))?;
    let names = dir.names().within(none())?;
    let harnesses: Vec<String> = settings::harnesses()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let harness = names
        .iter()
        .filter_map(|n| n.to_str()?.strip_suffix(".tar"))
        .find(|h| harnesses.iter().any(|k| k == h))
        .within(format!(
            "{}: restoring takes the backup of a harness there is here ({}); one from \
                 outside Link is added first, with lnk module add --url <its URL>",
            none(),
            harnesses.join(", ")
        ))?
        .to_string();
    let name = format!("{harness}.tar");
    let archive = folder.join(&name);
    let file = open_archive(&dir, &name, &archive)?;
    let state = state_of(&Adapter::find_or_add(&harness)?, settings)?;
    transfer::replace_state(file, &settings.box_of(&harness), &state, Links::Keep)
        .within(format!("unpacking {}", archive.display()))?;
    if names.iter().any(|n| n == OWN_ARCHIVE) {
        let archive = folder.join(OWN_ARCHIVE);
        let file = open_archive(&dir, OWN_ARCHIVE, &archive)?;
        transfer::replace_state(file, &settings.folder().agent, &own_state(), Links::Refuse)
            .within(format!("unpacking {}", archive.display()))?;
    }
    Ok(harness)
}

/// Where `lnk agent backup` puts the harness's state in the files
/// folder, so `lnk storage push` carries it with the files.
pub(crate) const BACKUP: &str = "Backups";

/// The agent's own folder (`agent/`) in [`BACKUP`], beside its harness's
/// state: named as no harness is (a harness's name is lowercase).
pub(crate) const OWN_ARCHIVE: &str = "Agent.tar";

/// What of the agent's own folder a backup carries, as a harness's state
/// is said: its instructions, skills and conversations, which were in its
/// files before, plus the file driver's declared revision history.
/// Memory's index is made again, and its jobs aren't
/// restored: a backup sits where the agent writes, so nothing restored
/// from it acts by itself.
pub(crate) fn own_state() -> link_harness_contract::harness::State {
    link_harness_contract::harness::State {
        include: link_agent_contract::folder::SEEN
            .iter()
            .map(|c| format!("~/{}", c.directory()))
            .chain(
                link_store_contract::store::file::state_paths()
                    .into_iter()
                    .map(|path| format!("~/{}", path.display())),
            )
            .collect(),
        exclude: Vec::new(),
    }
}

/// Writes archive `name` in the backup folder `open` (at `folder`) with
/// `write`, through a file of its own beside it, then put in its place:
/// its path. The harness may have left anything there, a link included:
/// a new file, never what a name points to, all through the folder's
/// handle.
pub(crate) fn write_archive(
    open: &Dir,
    folder: &std::path::Path,
    name: &str,
    write: impl FnOnce(std::fs::File) -> anyhow::Result<()>,
) -> anyhow::Result<std::path::PathBuf> {
    let archive = folder.join(name);
    let partial = archive.with_extension("tar.partial");
    let names = (
        std::ffi::OsString::from(name),
        std::ffi::OsString::from(format!("{name}.partial")),
    );
    match open.kind(&names.1) {
        Ok(Kind::Folder) => bail!("{} is a folder: remove it to go on", partial.display()),
        Ok(_) => open.remove_file(&names.1)?,
        Err(_) => {}
    }
    write(
        open.create_new(&names.1, 0o600)
            .within(format!("writing {}", partial.display()))?,
    )?;
    open.rename(&names.1, &names.0)
        .within(format!("writing {}", archive.display()))?;
    Ok(archive)
}

/// `files`' backup folder, made if it's missing: its path, and the
/// folder, open. The harness writes the files folder, and a process of it
/// can outlive `lnk agent stop` (a `setsid` child on macOS): a link or a
/// file in its place (to `~/.ssh`, ...) is removed, never written
/// through, and the folder is opened without following a link, so what's
/// written through that handle lands in it, whatever takes its name after.
pub(crate) fn backup_folder(files: &std::path::Path) -> anyhow::Result<(std::path::PathBuf, Dir)> {
    let folder = files.join(BACKUP);
    std::fs::create_dir_all(files).within(format!("creating {}", files.display()))?;
    let parent = Dir::open(files).within(format!("opening {}", files.display()))?;
    let name = std::ffi::OsStr::new(BACKUP);
    match parent.kind(name) {
        Ok(Kind::Folder) | Err(_) => {}
        Ok(_) => parent.remove_file(name)?,
    }
    let open = parent.create_folder(name, 0o755).within(format!(
        "creating {} (a link? not following it)",
        folder.display()
    ))?;
    Ok((folder, open))
}

/// `files`' backup folder, opened without following a link: the harness
/// writes the files folder, and a link there to another agent's backup
/// folder is refused, whenever it's put there.
pub(crate) fn open_backup_folder(files: &std::path::Path) -> std::io::Result<Dir> {
    Dir::open(files)?.folder(std::ffi::OsStr::new(BACKUP))
}

/// The archive `name` in the backup folder `folder` (at `shown`), a file
/// of its own: not a link the harness made to another tar of the user's,
/// for `lnk` to unpack into its home, nor a pipe to hold it. Opened
/// without following a link, and checked on the handle it's read from.
pub(crate) fn open_archive(
    folder: &Dir,
    name: &str,
    shown: &std::path::Path,
) -> anyhow::Result<std::fs::File> {
    let file = folder
        .open_entry(std::ffi::OsStr::new(name))
        .within(format!("opening {} (a file, not a link)", shown.display()))?;
    ensure!(
        file.metadata().is_ok_and(|m| m.is_file()),
        "{} isn't a file: not opening it",
        shown.display()
    );
    Ok(file)
}

/// `lnk agent backup`: the files folder to your clouds, with the harness's
/// state (memory, conversations, skills) in it as `Backups/<harness>.tar`,
/// the agent stopped meanwhile. A bucket its first push makes is billed
/// by its cloud: `lnk storage push` asks, or `yes` (`--yes`) answers it.
pub fn backup(out: &mut Out, settings: &Settings, yes: bool) -> Result<Done, Report> {
    link_core::ensure("storage", "lnk agent backup")?;
    a_cloud(out, "Backing up")?;
    if service::foreground_running(&settings.name) {
        return Err(Report::error(sentence!(
            "{} runs in a terminal.",
            Name::new("agent", &settings.name)
        ))
        .why(prose!("Stop it there first."))
        .exit(Exit::Usage));
    }
    let running = service::running(&settings.name);
    if running {
        out.block(Note(sentence!(
            "Stopping your agent while its files are copied."
        )));
        service::stop(&settings.name)?;
    }
    let files = settings.files();
    let name = settings.harness();
    out.hand_over();
    let pushed = (|| -> anyhow::Result<std::path::PathBuf> {
        let state = state_of(&Adapter::find(name)?, settings)?;
        let (folder, open) = backup_folder(&files)?;
        let archive = write_archive(&open, &folder, &format!("{name}.tar"), |file| {
            transfer::archive_state(file, &settings.box_of(name), &state)
        })?;
        // Its own folder's instructions, skills and conversations, beside
        // it.
        let own = settings.folder().agent;
        let own_archive = write_archive(&open, &folder, OWN_ARCHIVE, |file| {
            transfer::archive_state(file, &own, &own_state())
        })?;
        // A backup is this machine's copy, the latest: what changed since
        // the last one (the state, a conversation that grew) replaces it.
        // The state goes first, so it's kept even if a file fails.
        let state_pushed = link_core::lnk()
            .args(["storage", "push", "--replace"])
            .args(yes.then_some("--yes"))
            .arg(&archive)
            .arg(&own_archive)
            .status()
            .within("running lnk storage push")?;
        ensure!(
            state_pushed.success(),
            "backing up {name}'s state failed (above)"
        );
        let files_pushed = link_core::lnk()
            .args(["storage", "push", "--replace"])
            .args(yes.then_some("--yes"))
            .arg(&files)
            .status()
            .within("running lnk storage push")?;
        ensure!(
            files_pushed.success(),
            "backing up {} failed (above)",
            files.display()
        );
        Ok(archive)
    })();
    if running {
        start_again(out, settings);
    }
    let archive = pushed?;
    out.block(Note(sentence!(
        "Its files, {}, and {}'s state and its own folder in them, {}, are in your clouds.",
        Path::new(&files),
        Name::new("harness", name),
        Path::new(&archive)
    )));
    Ok(Done::success(sentence!(
        "Backed up {}.",
        Name::new("agent", &settings.name)
    )))
}

/// Starts agent `settings` again after a backup or a restore stopped it.
pub(crate) fn start_again(out: &mut Out, settings: &Settings) {
    out.hand_over();
    let started = link_core::lnk()
        .arg("agent")
        .args(super::naming(&settings.name))
        .arg("start")
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !started {
        out.block(
            Report::warning(sentence!("Your agent didn't start again."))
                .next(cmd!("lnk agent start"), "start it"),
        );
    }
}

/// `lnk agent restore` of an agent that's here: its harness's state
/// (memory, conversations, skills) replaced by the one in its last
/// backup, `Backups/<harness>.tar` pulled from your clouds, and its own
/// folder's instructions, skills and conversations by `Backups/Agent.tar`
/// beside it, the agent stopped meanwhile. Its files, settings and keys
/// stay as they are.
pub fn restore_state(
    out: &mut Out,
    settings: &Settings,
    yes: bool,
    no_start: bool,
) -> Result<Done, Report> {
    link_core::ensure("storage", "lnk agent restore")?;
    let agent = Name::new("agent", &settings.name);
    if service::foreground_running(&settings.name) {
        return Err(Report::error(sentence!("{} runs in a terminal.", &agent))
            .why(prose!("Stop it there first."))
            .exit(Exit::Usage));
    }
    let harness = settings.harness();
    let state = state_of(&Adapter::find(harness)?, settings)?;
    out.block(Text(prose!(
        "This replaces what {} knows as {} (its memory, conversations and skills) with its last \
         backup: what it learned since is lost. Its files, settings and keys stay.",
        Name::new("harness", harness),
        &agent
    )));
    let restore = out.ask(Question::confirm(sentence!("Restore?"), false).yes(yes))?;
    if !restore {
        return Ok(Done::success(sentence!("Nothing restored.")));
    }
    let name = format!("{harness}.tar");
    let archive = settings.files().join(BACKUP).join(&name);
    let own_archive = settings.files().join(BACKUP).join(OWN_ARCHIVE);
    // Stopped first: its owner running, storage pulls nothing into its
    // folder, which the agent could change meanwhile.
    let running = service::running(&settings.name);
    if running {
        out.block(Note(sentence!(
            "Stopping your agent while its state is replaced."
        )));
        service::stop(&settings.name)?;
    }
    // The clouds' copy is the last backup: it replaces any here.
    out.hand_over();
    let pulled = link_core::lnk()
        .args(["storage", "pull", "--replace"])
        .arg(&archive)
        .arg(&own_archive)
        .stdin(Stdio::inherit())
        .status()
        .within("running lnk storage pull");
    if !pulled
        .as_ref()
        .is_ok_and(|p| p.success() && archive.is_file())
    {
        if running && !no_start {
            start_again(out, settings);
        }
        pulled?;
        return Err(Report::error(sentence!(
            "No backup of {}'s state came back from your clouds (above).",
            Name::new("harness", harness)
        )));
    }
    // Opened only now, by handle: the harness ran until `stop`, and could
    // have put a link to another agent's backup in place of the folder.
    // Its own folder's conversations go back with the harness's state:
    // what was said since the backup is lost from both, as asked.
    let replaced = open_backup_folder(&settings.files())
        .within(format!(
            "opening {} (a folder, not a link)",
            settings.files().join(BACKUP).display()
        ))
        .and_then(|folder| {
            let file = open_archive(&folder, &name, &archive)?;
            transfer::replace_state(file, &settings.box_of(harness), &state, Links::Keep)?;
            if folder.names()?.iter().any(|n| n == OWN_ARCHIVE) {
                let file = open_archive(&folder, OWN_ARCHIVE, &own_archive)?;
                transfer::replace_state(
                    file,
                    &settings.folder().agent,
                    &own_state(),
                    Links::Refuse,
                )
                .within(format!("unpacking {}", own_archive.display()))?;
            }
            Ok(())
        });
    if running && !no_start {
        start_again(out, settings);
    }
    replaced?;
    out.block(Note(sentence!(
        "Restored from {}, pulled from your clouds.",
        Path::new(&archive)
    )));
    Ok(Done::success(sentence!(
        "{}'s {} knows what it knew at its last backup.",
        &agent,
        Name::new("harness", harness)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backups_keep_the_file_drivers_declared_revision_history() {
        use link_store_contract::store::{Agent, Store as _, Written, file};
        let root = link_test_kit::TempDir::new("backup-revisions");
        let source = root.join("source");
        let agent = Agent {
            id: "aaaaaaaaaaaa".into(),
            owner: "me@box".into(),
        };
        let driver = file::Folder::at(&source, &agent.id).unwrap();
        let path = "instructions/instruction.md";
        let Written::Version(first) = driver.file_write(&agent, path, b"A", 0).unwrap() else {
            panic!("the first version was not written");
        };
        let last = driver.file_append(&agent, path, b"B").unwrap();
        let mut bytes = Vec::new();
        transfer::archive_state(&mut bytes, &source, &own_state()).unwrap();
        let mut archive = tar::Archive::new(std::io::Cursor::new(&bytes));
        let archived: Vec<_> = archive
            .entries()
            .unwrap()
            .map(|entry| entry.unwrap().path().unwrap().into_owned())
            .collect();
        for declared in file::state_paths() {
            assert!(
                archived.contains(&declared.join("next")),
                "missing declared revision state: {archived:?}"
            );
        }
        // The actual owner archive, expanded in another fresh driver root,
        // preserves its high-water mark; inode changes receive new versions.
        let copied = root.join("copied");
        tar::Archive::new(std::io::Cursor::new(bytes))
            .unpack(&copied)
            .unwrap();
        let copied = file::Folder::at(&copied, &agent.id).unwrap();
        let (body, version) = copied.file_read(&agent, path).unwrap().unwrap();
        assert_eq!(body, b"AB");
        assert!(version > last && last > first);
        assert!(matches!(
            copied.file_write(&agent, path, b"stale", first).unwrap(),
            Written::Stale(_)
        ));
    }

    /// A link the harness left for the backup folder is removed, never
    /// written through, and a linked archive is never opened.
    #[test]
    fn backups_never_follow_the_harness_links() {
        let root = link_test_kit::TempDir::new("backup-links");
        let (files, outside) = (root.join("files"), root.join("outside"));
        std::fs::create_dir_all(&files).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keys"), "mine").unwrap();

        std::os::unix::fs::symlink(&outside, files.join(BACKUP)).unwrap();
        let (folder, _) = backup_folder(&files).unwrap();
        assert!(std::fs::symlink_metadata(&folder).unwrap().is_dir());
        assert_eq!(
            std::fs::read_to_string(outside.join("keys")).unwrap(),
            "mine"
        );

        std::os::unix::fs::symlink(outside.join("keys"), folder.join("h.tar")).unwrap();
        let open = |name: &str| {
            let dir = open_backup_folder(&files)?;
            open_archive(&dir, name, &folder.join(name))
        };
        assert!(open("h.tar").is_err());
        std::fs::write(folder.join("real.tar"), "").unwrap();
        open("real.tar").unwrap();
        // A pipe is refused without waiting on it.
        use std::os::unix::ffi::OsStrExt;
        let pipe = std::ffi::CString::new(folder.join("p.tar").as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid C string.
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        assert!(open("p.tar").is_err());

        // The backup folder swapped for a link to another agent's, after
        // the pull: its archive is never opened.
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("real.tar"), "theirs").unwrap();
        std::fs::rename(&folder, root.join("moved")).unwrap();
        std::os::unix::fs::symlink(&other, &folder).unwrap();
        assert!(open("real.tar").is_err());
    }
}
