//! Files copied to a box: each opened unlinked so a swapped link is
//! refused, sent in batches over one session into the folder asked.

use super::*;

/// `folder` (a folder on the box, as `lnk box copy` is given it) as its
/// shell reads it: `~` and `~/...` from the home folder, any other path
/// quoted whole, so nothing in it runs.
pub fn remote_folder(folder: &str) -> anyhow::Result<String> {
    ensure!(
        !folder.is_empty() && !folder.contains(['\n', '\r', '\0']),
        "{:?} isn't a folder on the box",
        link_format::clean(folder)
    );
    Ok(match folder {
        "~" | "~/" => "\"$HOME\"".to_string(),
        _ => match folder.strip_prefix("~/") {
            Some(rest) => format!("\"$HOME\"/{}", crate::setup::quote(rest)),
            None => crate::setup::quote(folder),
        },
    })
}

/// At most this many files go in one SSH session of [`copy`]: each is
/// held open until sent, and named on its command line.
const COPY_BATCH: usize = 64;

/// Copies each of `files` to `folder` (as [`remote_folder`] reads it) on
/// the box at `ip`, keeping its name and permissions, over SSH: up to
/// [`COPY_BATCH`] of them in one session, their bytes one after another
/// on ssh's input, each written beside its place first, its length
/// checked, and moved into place once whole. The folder is made if it's
/// missing. Every file is opened and checked before any is sent.
pub fn copy(ip: &str, files: &[PathBuf], folder: &str) -> anyhow::Result<()> {
    let mut options: Vec<String> = match shared(ip) {
        Shared::Open(options) => options,
        Shared::Failed | Shared::Off => Vec::new(),
    };
    options.extend(["-o".into(), "BatchMode=yes".into()]);
    let options: Vec<&str> = options.iter().map(String::as_str).collect();
    let dir = remote_folder(folder)?;
    let opened = files
        .iter()
        .map(|file| Opened::of(file))
        .collect::<anyhow::Result<Vec<_>>>()?;
    for batch in opened.chunks(COPY_BATCH) {
        copy_batch(ip, &options, batch, &dir)?;
    }
    Ok(())
}

/// A file [`copy`] sends: opened, never through a link, and checked.
struct Opened<'a> {
    file: &'a std::path::Path,
    name: &'a str,
    input: std::fs::File,
    size: u64,
    mode: u32,
}

impl<'a> Opened<'a> {
    fn of(file: &'a std::path::Path) -> anyhow::Result<Self> {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .within(format!("{} has no name to copy it as", file.display()))?;
        ensure!(
            !name.contains(['\n', '\r']),
            "{} has a line break in its name",
            file.display()
        );
        // A link is refused, not followed: one planted where you copy from
        // would send its target (an SSH key) to the box. Opened before
        // anything else, so what's sent is what was checked.
        let input = open_unlinked(file, &link_files::home_dir())?;
        let meta = input
            .metadata()
            .within(format!("reading {}", file.display()))?;
        ensure!(
            meta.is_file(),
            "{} isn't a file: lnk box copy copies files (a folder: tar it first, and copy that)",
            file.display()
        );
        #[cfg(unix)]
        let mode = std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o777;
        #[cfg(not(unix))]
        let mode = 0o644;
        Ok(Self {
            file,
            name,
            input,
            size: meta.len(),
            mode,
        })
    }
}

/// At most this much of what ssh says is kept for [`copy_batch`]'s error.
const SAID_LIMIT: u64 = 64 << 10;

/// The last lines ssh said, cleaned, after a colon; nothing when it said
/// nothing.
fn last_lines(said: &str) -> String {
    let lines: Vec<&str> = said
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    match lines.len() {
        0 => String::new(),
        n => format!(
            ": {}",
            link_format::clean(&lines[n.saturating_sub(3)..].join(" / "))
        ),
    }
}

/// The command [`copy_batch`] runs on the box for `files`, into `dir` (as
/// its shell reads it): each file's bytes, exactly its size, taken from
/// the input in turn.
fn copy_command(files: &[Opened], dir: &str) -> String {
    let mut remote = format!("umask 077 && mkdir -p -- {dir}");
    for opened in files {
        let to = format!("{dir}/{}", crate::setup::quote(opened.name));
        let part = format!(
            "{dir}/{}",
            crate::setup::quote(&format!("{}.lnk-copy", opened.name))
        );
        let (size, mode) = (opened.size, opened.mode);
        remote += &format!(
            " && head -c {size} > {part} && [ $(wc -c < {part}) -eq {size} ] && \
             chmod {mode:o} {part} && mv -f -- {part} {to}"
        );
    }
    remote
}

/// [`copy`] of `files`, in one SSH session with ssh's `options`.
fn copy_batch(ip: &str, options: &[&str], files: &[Opened], dir: &str) -> anyhow::Result<()> {
    use std::io::Read;
    let mut child = command(ip, options)?
        .arg(copy_command(files, dir))
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .within("running ssh")?;
    // What ssh says, read as it's written so it never fills the pipe, kept
    // for the error when the copy fails.
    let said = child.stderr.take().map(|err| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = err.take(SAID_LIMIT).read_to_string(&mut text);
            text
        })
    });
    let mut to = child.stdin.take().within("no input to ssh")?;
    let mut sent = Ok(());
    for opened in files {
        let mut input = &opened.input;
        let copied = std::io::copy(&mut input.take(opened.size), &mut to);
        // Exactly the size it had when checked: one that changed since
        // would run into the next file's bytes.
        let changed = match copied {
            Ok(copied) => copied != opened.size || !matches!(input.read(&mut [0u8]), Ok(0)),
            Err(err) => {
                sent = Err(err);
                break;
            }
        };
        if changed {
            drop(to);
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "{} changed while it was copied: copy it again",
                opened.file.display()
            );
        }
    }
    drop(to);
    let status = child.wait().within("running ssh")?;
    let said = said
        .and_then(|reading| reading.join().ok())
        .unwrap_or_default();
    let names: Vec<String> = files.iter().map(|o| o.file.display().to_string()).collect();
    ensure!(
        status.success(),
        "copying {} to the box failed ({status}){}",
        names.join(", "),
        last_lines(&said)
    );
    sent.within("sending to ssh")?;
    Ok(())
}

/// `file`, opened to read without following a link: neither its own
/// name nor any folder on the way to it. Each folder is opened from the
/// one before (`openat`, `O_NOFOLLOW`), so a folder swapped for a link
/// after any earlier check is refused too, never followed. `..` is taken
/// off the path first, as written. Below `home` the walk starts there;
/// elsewhere it starts at `/`, and follows only the system's own links,
/// those root owns (`/tmp` on a Mac), which no one else can make.
#[cfg(unix)]
pub fn open_unlinked(
    file: &std::path::Path,
    home: &std::path::Path,
) -> anyhow::Result<std::fs::File> {
    use link_files::nofollow::{Dir, Kind};
    use std::path::Component;

    let reading = || format!("reading {}", file.display());
    let mut path = PathBuf::new();
    for part in std::path::absolute(file).within(reading())?.components() {
        match part {
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            part => path.push(part),
        }
    }
    let name = path
        .file_name()
        .within(format!("{} has no name to copy it as", file.display()))?
        .to_owned();
    let parent = path.parent().unwrap_or(&path);
    let root = std::path::Path::new("/");
    let (start, below, in_home) = match parent.strip_prefix(home) {
        Ok(below) => (home, below, true),
        Err(_) => (root, parent.strip_prefix(root).unwrap_or(parent), false),
    };
    let mut folder = Dir::open(start).within(reading())?;
    let mut on_the_way = start.to_path_buf();
    for part in below.components() {
        on_the_way.push(part);
        let part = part.as_os_str();
        let opened = match folder.folder(part) {
            // A link root owns, outside home: the system's own (a Mac's
            // `/var`, `/etc` and `/tmp`). Opened as a folder without
            // following, a link is refused with ELOOP or ENOTDIR, as the
            // system chooses.
            Err(err)
                if !in_home
                    && matches!(err.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR))
                    && folder
                        .stat(part)
                        .is_ok_and(|stat| stat.kind == Kind::Link && stat.uid == 0) =>
            {
                folder.folder_through_link(part)
            }
            other => other,
        };
        folder = match opened {
            Ok(next) => next,
            Err(err) if matches!(err.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => bail!(
                "{} is a link, or no folder, on the way to {}: lnk box copy never follows one \
                 (copy the file by its own path)",
                on_the_way.display(),
                file.display()
            ),
            Err(err) => return Err(err).within(reading()),
        };
    }
    // Without blocking: a pipe put in its place opens at once, and is
    // then refused as no file.
    match folder.open_entry(&name) {
        Ok(opened) => {
            // Blocking again, as ssh reads it.
            link_files::nofollow::set_blocking(&opened).within(reading())?;
            // A file with another name (a hard link) may be one anywhere
            // else on the disk, as a link would be.
            let meta = opened.metadata().within(reading())?;
            if meta.is_file() && std::os::unix::fs::MetadataExt::nlink(&meta) > 1 {
                bail!(
                    "{} has another name (a hard link): lnk box copy copies only a file with \
                     one (copy it to a new file first, and copy that)",
                    file.display()
                );
            }
            Ok(opened)
        }
        Err(err) if err.raw_os_error() == Some(libc::ELOOP) => bail!(
            "{} is a link: lnk box copy copies files, never a link's target (copy the file it \
             points to by its own path)",
            file.display()
        ),
        Err(err) => Err(err).within(reading()),
    }
}

#[cfg(not(unix))]
pub fn open_unlinked(
    file: &std::path::Path,
    _home: &std::path::Path,
) -> anyhow::Result<std::fs::File> {
    std::fs::File::open(file).within(format!("reading {}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed copy says why, in ssh's own last words.
    #[test]
    fn a_failed_copy_ends_with_what_ssh_said() {
        let said = "Warning: Permanently added '34.60.244.124'\n\nme@34.60.244.124: Permission denied (publickey).\n";
        assert_eq!(
            last_lines(said),
            ": Warning: Permanently added '34.60.244.124' / me@34.60.244.124: Permission denied (publickey)."
        );
        assert_eq!(last_lines(" \n"), "");
    }

    /// A folder on the way to a file, checked by `lnk box copy`, then
    /// swapped for a link to the home folder before the file is opened:
    /// the open refuses it, as it does the file itself as a link. Above
    /// the home folder, and `..` as written, it follows.
    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_after_the_check_is_refused() {
        use std::io::Read;
        use std::os::unix::fs::symlink;
        let root = link_test_kit::TempDir::new("box-swap");
        let home = root.join("home");
        let app = home.join("files/app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(home.join(".env"), "SECRET").unwrap();
        std::fs::write(app.join(".env"), "the harness's").unwrap();
        let file = app.join(".env");
        let read = |path: &std::path::Path| {
            let mut text = String::new();
            open_unlinked(path, &home)
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            text
        };

        // What `lnk box copy` checks first: no link on the way.
        assert!(!std::fs::symlink_metadata(&app).unwrap().is_symlink());
        std::fs::rename(&app, home.join("files/real")).unwrap();
        symlink(&home, &app).unwrap();
        let refused = open_unlinked(&file, &home).unwrap_err().to_string();
        assert!(refused.contains("app is a link"), "{refused}");

        std::fs::remove_file(&app).unwrap();
        std::fs::rename(home.join("files/real"), &app).unwrap();
        std::fs::remove_file(&file).unwrap();
        symlink(home.join(".env"), &file).unwrap();
        let refused = open_unlinked(&file, &home).unwrap_err().to_string();
        assert!(refused.contains(".env is a link:"), "{refused}");

        std::fs::remove_file(&file).unwrap();
        std::fs::write(&file, "mine").unwrap();
        assert_eq!(read(&home.join("files/gone/../app/.env")), "mine");
        // Outside the home folder too, a link of yours on the way is
        // refused: only root's are the system's.
        symlink(home.join("files"), root.join("elsewhere")).unwrap();
        // Run as root, the link is root's: give it to someone else, as
        // yours would be.
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            let name =
                std::ffi::CString::new(root.join("elsewhere").as_os_str().as_encoded_bytes())
                    .unwrap();
            // SAFETY: a NUL-terminated path.
            assert_eq!(unsafe { libc::lchown(name.as_ptr(), 65534, 65534) }, 0);
        }
        let refused = open_unlinked(&root.join("elsewhere/app/.env"), &home)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("elsewhere is a link"), "{refused}");
        assert_eq!(read(&root.join("home/files/app/.env")), "mine");

        // A hard link to a file elsewhere is refused, as a link is.
        let hard = app.join("hard");
        std::fs::hard_link(home.join(".env"), &hard).unwrap();
        let refused = open_unlinked(&hard, &home).unwrap_err().to_string();
        assert!(refused.contains("hard link"), "{refused}");
        std::fs::remove_file(&hard).unwrap();
        assert_eq!(read(&home.join(".env")), "SECRET");

        // A pipe opens without waiting for a writer, and is no file.
        let pipe = app.join("pipe");
        let name = std::ffi::CString::new(pipe.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let opened = open_unlinked(&pipe, &home).unwrap();
        assert!(!opened.metadata().unwrap().is_file());
    }

    /// A file outside home reached through the system's own link, as a
    /// Mac's temporary folder is (`/var` -> `private/var`), opens: what
    /// `lnk box create` copies a release from.
    #[cfg(unix)]
    #[test]
    fn a_file_through_the_systems_own_link_opens() {
        use std::os::unix::fs::MetadataExt;
        // A Mac's `/etc`, and a merged-usr Linux's `/lib`, are root's links.
        let through_link = ["/etc/hosts", "/lib/os-release"].into_iter().find(|file| {
            let file = std::path::Path::new(file);
            let top = file.ancestors().nth(file.components().count() - 2).unwrap();
            std::fs::symlink_metadata(top).is_ok_and(|m| m.is_symlink() && m.uid() == 0)
                && std::fs::symlink_metadata(file).is_ok_and(|m| m.is_file() && m.nlink() == 1)
        });
        let Some(file) = through_link else {
            link_test_kit::gate::lacks(
                "a file below a root-owned link at /",
                "neither /etc nor /lib is one here",
                None,
            );
            return;
        };
        let home = link_test_kit::TempDir::new("box-system-link");
        open_unlinked(std::path::Path::new(file), &home).unwrap();
    }

    #[test]
    fn a_folder_on_the_box_is_read_whole_by_its_shell() {
        assert_eq!(remote_folder("~").unwrap(), "\"$HOME\"");
        assert_eq!(remote_folder("~/").unwrap(), "\"$HOME\"");
        assert_eq!(
            remote_folder("~/set up/$(x)").unwrap(),
            "\"$HOME\"/'set up/$(x)'"
        );
        assert_eq!(remote_folder("setup").unwrap(), "'setup'");
        assert_eq!(remote_folder("/opt/it's").unwrap(), r"'/opt/it'\''s'");
        assert!(remote_folder("").is_err());
        assert!(remote_folder("a\nb").is_err());
    }

    /// Several files in one session: each its exact size from the one
    /// input, put in place whole with its mode; input that falls short
    /// leaves the file it was for out of place.
    #[cfg(unix)]
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn files_in_one_session_each_take_their_size() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let root = link_test_kit::TempDir::new("box-batch");
        let (a, b) = (root.join("a.sh"), root.join("it's b"));
        std::fs::write(&a, "echo a\n").unwrap();
        std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o750)).unwrap();
        std::fs::write(&b, "bee").unwrap();
        std::fs::write(root.join("empty"), "").unwrap();
        let files = [a.clone(), root.join("empty"), b.clone()];
        let opened: Vec<Opened> = files.iter().map(|f| Opened::of(f).unwrap()).collect();
        let there = root.join("there");
        let dir = remote_folder(&there.join("sub dir").to_string_lossy()).unwrap();
        let run = |input: &[u8]| {
            let mut child = Command::new("sh")
                .arg("-c")
                .arg(copy_command(&opened, &dir))
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            child.wait().unwrap().success()
        };
        assert!(run(b"echo a\nbee"));
        let sub = there.join("sub dir");
        assert_eq!(std::fs::read(sub.join("a.sh")).unwrap(), b"echo a\n");
        assert_eq!(std::fs::read(sub.join("empty")).unwrap(), b"");
        assert_eq!(std::fs::read(sub.join("it's b")).unwrap(), b"bee");
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&sub.join("a.sh")), 0o750);
        std::fs::remove_dir_all(&there).unwrap();
        // Short by a byte: the last file isn't put in place.
        assert!(!run(b"echo a\nbe"));
        assert!(sub.join("a.sh").is_file());
        assert!(!sub.join("it's b").exists());
    }
}
