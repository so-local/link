//! A stand-in for the macOS Keychain, so no test reaches a person's own.
//!
//! Link keeps secrets in the Keychain on a Mac, always, through Apple's
//! `security` tool; `LNK_SECURITY` names another program to run in its
//! place (`link-keychain`). The stand-in is that program: a script keeping
//! each item as a file in a folder of the test's, on any Unix. Elsewhere
//! there is no Keychain, and Link keeps secrets in its private files.

use std::path::{Path, PathBuf};

/// The variable naming the `security` Link runs, as `link-keychain` reads
/// it.
pub const SECURITY: &str = "LNK_SECURITY";

/// The variable a child of `home` needs so no Keychain but a stand-in is
/// reached: on a Mac, [`SECURITY`] as a stand-in in `home/.keychain`;
/// elsewhere none, for there is no Keychain to reach. Give it with
/// `command.envs(...)`.
pub fn isolated(home: &Path) -> Option<(&'static str, PathBuf)> {
    #[cfg(target_os = "macos")]
    return Some((SECURITY, stand_in(&home.join(".keychain"))));
    #[cfg(not(target_os = "macos"))]
    {
        let _ = home;
        None
    }
}

/// A stand-in for `security` in `dir`, keeping items as files in
/// `dir/items`, made once: its path, for [`SECURITY`]. It answers
/// `find-generic-password` as `security` does: with `-w`, a secret with
/// an unprintable byte as bare hex; with `-g`, `password: ` and either
/// `0x` and hex (where `security` then adds the readable characters, the
/// stand-in `"..."`) or the text in quotes.
#[cfg(unix)]
pub fn stand_in(dir: &Path) -> PathBuf {
    let script = dir.join("security");
    let store = dir.join("items");
    std::fs::create_dir_all(&store)
        .unwrap_or_else(|err| panic!("making {}: {err}", store.display()));
    if script.exists() {
        return script;
    }
    crate::write_program(
        &script,
        format!(
            r#"#!/bin/bash
store={store:?}
item() {{ echo "$store/$1"; }}
if [ "$1" = -i ]; then
  read -r cmd rest
  set -- $rest
  while [ $# -gt 0 ]; do
case "$1" in -a) acct=$2; shift 2 ;; -X) hex=$2; shift 2 ;; -s|-l) shift 2 ;; *) shift ;; esac
  done
  mkdir -p "$(dirname "$(item "$acct")")"
  printf "$(printf '%s' "$hex" | sed 's/../\\x&/g')" > "$(item "$acct")"
  exit 0
fi
cmd=$1; shift
while [ $# -gt 0 ]; do case "$1" in -a) acct=$2; shift 2 ;; -s) shift 2 ;; -w) out=w; shift ;; -g) out=g; shift ;; *) shift ;; esac; done
f=$(item "$acct")
hex() {{ od -An -tx1 -v "$f" | tr -d ' \n'; }}
case "$cmd" in
  find-generic-password) [ -f "$f" ] || exit 44
    if [ "$out" = w ]; then
      if [ "$(LC_ALL=C tr -d ' -~' < "$f" | wc -c)" -gt 0 ]; then hex; else cat "$f"; fi; echo
    elif [ "$out" = g ]; then
      printf 'keychain: "fake"\n'
      if [ ! -s "$f" ]; then echo 'password: ' >&2
      elif [ "$(LC_ALL=C tr -d ' -[]-~' < "$f" | wc -c)" -gt 0 ]; then
        printf 'password: 0x%s  "..."\n' "$(hex | tr a-f A-F)" >&2
      else printf 'password: "%s"\n' "$(cat "$f")" >&2; fi
    fi ;;
  delete-generic-password) [ -f "$f" ] || exit 44; rm "$f" ;;
  dump-keychain) (cd "$store" && find . -type f | sed 's|^\./||') | while read -r a; do
      printf 'keychain: "fake"\nattributes:\n    "acct"<blob>="%s"\n    "svce"<blob>="link"\n' "$a"
    done ;;
esac
"#
        ),
    )
    .unwrap_or_else(|err| panic!("writing {}: {err}", script.display()));
    script
}

/// A `security` in `dir` that fails every command, as a locked Keychain
/// or one that asks for a person does: its path, for [`SECURITY`]. A
/// test of a caller proves that a Keychain that can't keep or give a
/// secret fails the caller's command, never falling back to a file.
#[cfg(unix)]
pub fn refusing(dir: &Path) -> PathBuf {
    let script = dir.join("security-refusing");
    std::fs::create_dir_all(dir).unwrap_or_else(|err| panic!("making {}: {err}", dir.display()));
    crate::write_program(
        &script,
        // It reads the whole request first, as `security -i` does, so a
        // caller writing to it never meets a closed pipe.
        "#!/bin/sh\n[ \"$1\" = -i ] && cat > /dev/null\n\
         echo 'security: User interaction is not allowed.' >&2\nexit 36\n",
    )
    .unwrap_or_else(|err| panic!("writing {}: {err}", script.display()));
    script
}
