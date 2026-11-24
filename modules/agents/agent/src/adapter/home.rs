//! What a home holds before a harness runs in it: the shims, the `PATH`
//! and the measures folder, and whether it is fresh.

use super::*;
use link_format::WithinExt;

/// The variable holding the agent's name, in every adapter command's
/// environment.
pub const AGENT_ENV: &str = "LNK_AGENT";

/// Where a harness whose home is `home` appends its own measures
/// (`LNK_MEASURES`): in its home, the one folder it writes.
pub fn measures_of(home: &Path) -> PathBuf {
    link_files::state::path(home, link_files::state::Area::Measures)
}

/// The system's PATH, with (macOS, `developer_tools`) the real developer
/// tools first: `/usr/bin/git` is a launcher that runs git through xcrun,
/// which tries to write a cache in the user's temp folder the sandbox
/// keeps closed (a harness could otherwise point the user's own `git`
/// anywhere), and complains.
/// `mktemp`, first on a sandboxed harness's PATH on macOS. There, a call
/// without a template makes its file in the per-user temp folder
/// (`/var/folders/.../T`), whatever `TMPDIR` says, and the sandbox refuses
/// that folder, which every app shares: installers calling `mktemp -d`
/// (uv's, in one harness's) failed. This makes it in `TMPDIR`, the home's
/// `tmp`; a call with a template or `-p` goes to `/usr/bin/mktemp` as is.
const MKTEMP: &str = r#"#!/bin/sh
# Written by lnk: see "mktemp" in the harness module's adapter.rs.
real=/usr/bin/mktemp
case "$TMPDIR" in /*) ;; *) exec "$real" "$@" ;; esac
flags='' prefix=tmp next=''
for arg in "$@"; do
  if [ -n "$next" ]; then prefix=$arg; next=; continue; fi
  case "$arg" in
    -t) next=1 ;;
    -t?*) prefix=${arg#-t} ;;
    -[dqu] | -[dqu][dqu] | -[dqu][dqu][dqu]) flags="$flags $arg" ;;
    *) exec "$real" "$@" ;;
  esac
done
[ -z "$next" ] || exec "$real" "$@"
# shellcheck disable=SC2086
exec "$real" $flags "${TMPDIR%/}/$prefix.XXXXXXXXXX"
"#;

/// Writes Link's shims into `dir/exec/agents/shims` (unless they're there as
/// they are), and returns that folder. The harness writes `dir`, maybe
/// while this runs, and `lnk` runs outside its sandbox: no link there is
/// followed. Each folder is opened from the one holding it, a link in
/// its place refused, and a shim is written as a new file then renamed
/// over whatever is at its name, a link included, never through it.
/// Whether harness home `dir` holds nothing but what Link puts there
/// ([`Boxed::new`]: `tmp`, and `.lnk` for its shims): its harness was
/// never installed in it.
pub(super) fn fresh(dir: &Path) -> bool {
    std::fs::read_dir(dir).map_or(true, |entries| {
        entries.flatten().all(|e| e.file_name() == "tmp")
    })
}

/// Whether harness `name`'s download of `what` may go ahead: with `yes`,
/// unasked; in a `terminal`, as `ask` answers; else not, saying how.
pub(super) fn runtime_asked(
    name: &str,
    what: &str,
    yes: bool,
    terminal: bool,
    ask: impl FnOnce() -> anyhow::Result<bool>,
) -> anyhow::Result<()> {
    if yes {
        return Ok(());
    }
    ensure!(
        terminal,
        "{name} downloads {what} the first time: run this in a terminal to be asked, or add --yes"
    );
    ensure!(
        ask()?,
        "{name} isn't installed: lnk agent harness set {name} asks again"
    );
    Ok(())
}

pub(super) fn write_shims(dir: &Path) -> anyhow::Result<PathBuf> {
    use link_files::nofollow::Dir;
    link_files::create_private_dir(dir)?;
    let bin = link_files::layout::Hidden::Shims.at(dir);
    let refused = |err: std::io::Error| {
        anyhow::Error::from(err).context(format!(
            "making {} (a link on the way? not following it: remove it to go on)",
            bin.display()
        ))
    };
    let home = Dir::open(dir).within(format!("opening {}", dir.display()))?;
    let mut folder = home;
    for part in std::path::Path::new(link_files::layout::Hidden::Shims.relative()).components() {
        folder = folder
            .create_folder(part.as_os_str(), 0o755)
            .map_err(refused)?;
    }
    let name = std::path::Path::new(link_files::layout::Hidden::Mktemp.relative())
        .file_name()
        .expect("the shim has a name");
    let current = folder.open_entry(name).ok().and_then(|file| {
        let meta = file.metadata().ok()?;
        (meta.is_file() && meta.len() == MKTEMP.len() as u64).then_some(())?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::Read::take(file, meta.len()), &mut text)
            .ok()?;
        Some(text)
    });
    if current.as_deref() == Some(MKTEMP) {
        return Ok(bin);
    }
    let temp = std::ffi::OsString::from(format!(
        ".mktemp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let written = (|| -> std::io::Result<()> {
        link_files::write_program(&bin.join(&temp), MKTEMP)?;
        folder.rename(&temp, name)
    })();
    if written.is_err() {
        let _ = folder.remove_file(&temp);
    }
    written.map_err(refused)?;
    Ok(bin)
}

pub(super) fn path(developer_tools: bool) -> String {
    let system = "/usr/bin:/bin:/usr/sbin:/sbin";
    if !cfg!(target_os = "macos") || !developer_tools {
        return system.into();
    }
    // Asked once per process.
    static DEVELOPER: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    let developer = DEVELOPER.get_or_init(|| {
        link_secretive::process::command("/usr/bin/xcode-select")
            .arg("-p")
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()).join("usr/bin"))
            .filter(|bin| bin.join("git").is_file())
    });
    match developer {
        Some(bin) => format!("{}:{system}", bin.display()),
        None => system.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A harness's runtime download is asked first in a terminal, needs
    /// --yes without one, and a no installs nothing.
    #[test]
    fn a_runtime_is_asked_for_first() {
        let asked = std::cell::Cell::new(false);
        let ask = |answer: bool| {
            let asked = &asked;
            move || {
                asked.set(true);
                Ok(answer)
            }
        };
        runtime_asked("deepseek", "packages", false, true, ask(true)).unwrap();
        assert!(asked.replace(false));
        let no = runtime_asked("deepseek", "packages", false, true, ask(false)).unwrap_err();
        assert!(
            no.to_string()
                .contains("lnk agent harness set deepseek asks again"),
            "{no}"
        );
        assert!(asked.replace(false));
        let script = runtime_asked("deepseek", "packages", false, false, ask(true)).unwrap_err();
        assert!(script.to_string().contains("or add --yes"), "{script}");
        assert!(!asked.get());
        runtime_asked("deepseek", "packages", true, false, ask(false)).unwrap();
        assert!(!asked.get());
    }

    #[test]
    fn a_home_with_only_links_own_is_fresh() {
        let dir = link_test_kit::TempDir::new("fresh-home");
        assert!(fresh(&dir.join("none")));
        std::fs::create_dir_all(dir.join("tmp")).unwrap();
        assert!(fresh(&dir));
        std::fs::create_dir_all(dir.join(".deepseek")).unwrap();
        assert!(!fresh(&dir));
    }

    /// The harness writes its home: a link it put where a shim goes, or
    /// on the way there, is never written through, and what it points to
    /// is left as it was.
    #[test]
    fn a_shim_is_never_written_through_a_link() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = link_test_kit::TempDir::new("shim-links");
        let yours = root.join("yours");
        std::fs::create_dir_all(&yours).unwrap();
        let key = yours.join("authorized_keys");
        let untouched = |home: &Path| {
            assert_eq!(std::fs::read_to_string(&key).unwrap(), "ssh-ed25519 mine");
            assert_eq!(
                std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::read_dir(&yours).unwrap().count(),
                1,
                "nothing made in the folder a link points to ({})",
                home.display()
            );
        };
        let fresh = |name: &str| {
            std::fs::write(&key, "ssh-ed25519 mine").unwrap();
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
            let home = root.join(name);
            std::fs::create_dir_all(&home).unwrap();
            home
        };

        // The shim itself: the link is replaced, its target isn't.
        let home = fresh("file");
        std::fs::create_dir_all(home.join("exec/agents/shims")).unwrap();
        symlink(&key, home.join("exec/agents/shims/mktemp")).unwrap();
        let bin = write_shims(&home).unwrap();
        untouched(&home);
        let shim = bin.join("mktemp");
        assert!(std::fs::symlink_metadata(&shim).unwrap().is_file());
        assert_eq!(std::fs::read_to_string(&shim).unwrap(), MKTEMP);
        assert_eq!(
            std::fs::metadata(&shim).unwrap().permissions().mode() & 0o777,
            0o755
        );
        // Written once: a second start leaves it.
        write_shims(&home).unwrap();

        // A folder on the way: refused.
        for (name, link) in [
            ("exec", "exec"),
            ("owner", "exec/agents"),
            ("shims", "exec/agents/shims"),
        ] {
            let home = fresh(name);
            std::fs::create_dir_all(home.join(link).parent().unwrap()).unwrap();
            symlink(&yours, home.join(link)).unwrap();
            assert!(write_shims(&home).is_err(), "{link}");
            untouched(&home);
        }
    }

    /// The macOS `mktemp` shim makes what's asked in `TMPDIR`, and leaves a
    /// call with a template as it is (run here with this system's mktemp).
    #[test]
    #[cfg(unix)]
    fn mktemp_makes_its_files_in_tmpdir() {
        let root = link_test_kit::TempDir::new("shim");
        let bin = write_shims(&root).unwrap();
        let tmp = root.join("tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let mktemp = |args: &[&str], tmpdir: Option<&Path>| {
            let mut command = link_test_kit::env::command(bin.join("mktemp"), &root);
            command.args(args).env_remove("TMPDIR");
            if let Some(dir) = tmpdir {
                command.env("TMPDIR", dir);
            }
            let out = command.output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            PathBuf::from(String::from_utf8_lossy(&out.stdout).trim())
        };
        let made = mktemp(&["-d"], Some(&tmp));
        assert!(made.starts_with(&tmp) && made.is_dir(), "{made:?}");
        let file = mktemp(&[], Some(&tmp));
        assert!(file.starts_with(&tmp) && file.is_file(), "{file:?}");
        let named = mktemp(&["-t", "x"], Some(&tmp));
        let name = named.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            named.parent() == Some(&*tmp) && name.starts_with("x."),
            "{named:?}"
        );
        let template = root.join("keep.XXXXXX");
        let kept = mktemp(&["-d", template.to_str().unwrap()], Some(&tmp));
        let name = kept.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            kept.parent() == Some(&*root) && name.starts_with("keep.") && kept.is_dir(),
            "{kept:?}"
        );
    }
}
