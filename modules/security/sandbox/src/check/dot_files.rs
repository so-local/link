//! The probes of each folder it writes for what runs outside the sandbox
//! at its top, and what they should find.

use std::path::{Path, PathBuf};

use link_sandbox_client::Confinement;
use link_sandbox_confine::Os;

use super::{Bait, DotFiles, Expect, Expected, Probe, disp, tilde};

/// The probes of each folder it writes for what runs outside the sandbox
/// at its top ([`crate::dotfiles`]): its `.git` written in, moved aside
/// and reached through a hard link, `.envrc`, `.vscode` and `.idea`. Each
/// removes what it wrote, and none writes a file that's there, only
/// opens it to add nothing.
pub(super) fn dot_file_probes(bait: &Bait, os: Os) -> Vec<Probe> {
    let mut all = Vec::new();
    let tag = bait.tag;
    let data: Vec<PathBuf> = link_sandbox_contract::dot_files::swept_folders()
        .iter()
        .map(|p| link_sandbox_confine::resolve(p))
        .collect();
    for dots in &bait.dot_files {
        let folder = &dots.folder;
        let name = tilde(folder, &bait.home);
        // In Link's data folders Linux makes nothing where one isn't: a
        // new one can be made, and is removed when the run ends.
        let swept = os == Os::Linux
            && dots.expect == Expect::Blocked
            && link_sandbox_contract::dot_files::in_swept(folder, &data);
        let sweeps = |mut probe: Probe| {
            if swept
                && probe
                    .path
                    .as_ref()
                    .is_some_and(|p| p.symlink_metadata().is_err())
            {
                probe.expect = Expect::Gap(
                    "in Link's data folders on Linux a new one can be made while a run lasts; \
                     it's removed when the run ends, and the file tool never writes one",
                );
            }
            probe
        };
        let probe = |what: String, script, path: PathBuf, arg: String| Probe {
            what,
            script,
            expect: dots.expect,
            lands: None,
            arg: Some(arg),
            path: Some(path),
            required: dots.required,
        };
        let git = folder.join(".git");
        let meta = git.symlink_metadata();
        all.push(sweeps(match &meta {
            Ok(meta) if meta.is_file() => probe(
                format!("change {name}/.git, which says where git's settings are"),
                DOT_FILE,
                git.clone(),
                String::new(),
            ),
            _ => probe(
                format!("write in {name}/.git (git's settings and hooks)"),
                DOT_FOLDER,
                git.clone(),
                format!("lnk-check-{tag}"),
            ),
        }));
        if git.join("config").is_file() {
            all.push(probe(
                format!("change {name}/.git/config through a hard link"),
                DOT_LINK,
                git.join("config"),
                disp(folder.join(format!("lnk-check-link-{tag}"))),
            ));
        }
        if meta.is_ok() {
            all.push(probe(
                format!("move {name}/.git aside"),
                DOT_MOVE,
                git.clone(),
                disp(moved_git(folder, tag)),
            ));
        }
        let envrc = folder.join(".envrc");
        let mut new_envrc = probe(
            format!("write {name}/.envrc (direnv runs it)"),
            DOT_FILE,
            envrc.clone(),
            String::new(),
        );
        // Linux holds a missing one with nothing, as direnv runs a new
        // .envrc only once you allow it.
        if os == Os::Linux && dots.expect == Expect::Blocked && envrc.symlink_metadata().is_err() {
            new_envrc.expect = Expect::Gap(
                "on Linux a new .envrc can be made; direnv runs it only after direnv allow, \
                 which shows it",
            );
        }
        all.push(sweeps(new_envrc));
        for (dir, whose) in [
            (".vscode", "your editor's tasks and settings"),
            (".idea", "JetBrains IDEs' run configurations"),
        ] {
            all.push(sweeps(probe(
                format!("write in {name}/{dir} ({whose})"),
                DOT_FOLDER,
                folder.join(dir),
                format!("lnk-check-{tag}"),
            )));
        }
    }
    all
}

/// Where the probe moving `folder`'s `.git` aside puts it.
pub(super) fn moved_git(folder: &Path, tag: u32) -> PathBuf {
    folder.join(format!("lnk-check-git-{tag}"))
}

/// A file `$LNK_PROBE_ARG` written in the folder `$LNK_PROBE_PATH` (made when it
/// isn't there), then removed, and the folder too if it was made.
const DOT_FOLDER: &str = r#"made=
if [ ! -e "$LNK_PROBE_PATH" ]; then mkdir "$LNK_PROBE_PATH" 2>/dev/null || exit 1; made=1; fi
: > "$LNK_PROBE_PATH/$LNK_PROBE_ARG" 2>/dev/null; code=$?
rm -f "$LNK_PROBE_PATH/$LNK_PROBE_ARG" 2>/dev/null
[ -z "$made" ] || rmdir "$LNK_PROBE_PATH" 2>/dev/null
[ "$code" = 0 ]"#;

/// The file `$LNK_PROBE_PATH` opened to write, adding nothing; when it isn't
/// there, made and removed again. An empty folder in its place (Linux's,
/// where the sandbox kept it read only before) is taken away first, and
/// put back.
const DOT_FILE: &str = r#"if [ -f "$LNK_PROBE_PATH" ]; then : >>"$LNK_PROBE_PATH" 2>/dev/null; exit $?; fi
held=
if [ -d "$LNK_PROBE_PATH" ]; then rmdir "$LNK_PROBE_PATH" 2>/dev/null || exit 1; held=1; fi
[ ! -e "$LNK_PROBE_PATH" ] || exit 1
: > "$LNK_PROBE_PATH" 2>/dev/null; code=$?
rm -f "$LNK_PROBE_PATH"
[ -z "$held" ] || mkdir "$LNK_PROBE_PATH"
[ "$code" = 0 ]"#;

/// A hard link `$LNK_PROBE_ARG` to the file `$LNK_PROBE_PATH`, opened to write
/// (adding nothing), then removed.
const DOT_LINK: &str = r#"ln "$LNK_PROBE_PATH" "$LNK_PROBE_ARG" 2>/dev/null || exit 1
: >>"$LNK_PROBE_ARG" 2>/dev/null; code=$?
rm -f "$LNK_PROBE_ARG"
[ "$code" = 0 ]"#;

/// `$LNK_PROBE_PATH` renamed to `$LNK_PROBE_ARG` and back (from outside too, after
/// the probes, when it can't come back from inside).
const DOT_MOVE: &str = r#"mv "$LNK_PROBE_PATH" "$LNK_PROBE_ARG" 2>/dev/null || exit 1
mv "$LNK_PROBE_ARG" "$LNK_PROBE_PATH" 2>/dev/null
exit 0"#;

/// The folders `policy` writes that are there, each with what its dot
/// files' probes should find: written where the policy names it in
/// `write_dot_files`, else not; as `expected` asks, where it does, and
/// then required.
pub(super) fn dot_files(
    policy: &Confinement,
    expected: &Expected,
) -> anyhow::Result<Vec<DotFiles>> {
    let real = link_sandbox_confine::resolve;
    let open: Vec<PathBuf> = policy.write_dot_files.iter().map(|p| real(p)).collect();
    let mut all: Vec<DotFiles> = Vec::new();
    for folder in policy.write.iter().map(|p| real(p)) {
        if !folder.is_dir() || all.iter().any(|d| d.folder == folder) {
            continue;
        }
        let (expect, required) = if expected.dot_files_open.contains(&folder) {
            (Expect::Allowed, true)
        } else if expected.dot_files_closed.contains(&folder) {
            (Expect::Blocked, true)
        } else if open.contains(&folder) {
            (Expect::Allowed, false)
        } else {
            (Expect::Blocked, false)
        };
        all.push(DotFiles {
            folder,
            expect,
            required,
        });
    }
    for asked in expected
        .dot_files_open
        .iter()
        .chain(&expected.dot_files_closed)
    {
        anyhow::ensure!(
            all.iter().any(|d| d.folder == *asked),
            "{}: not a folder it writes that's there",
            asked.display()
        );
    }
    Ok(all)
}
