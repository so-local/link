//! Box's VM provider, found by the question it answers (`box.machine`),
//! never by its name. `lnk module list --json` names the modules that
//! answer it, and each stand-in logs the operation it was asked and
//! answers as a provider would. So what's proved is Box's side: whom it
//! asks, what it asks, and that it never makes a box in a VM's place.

use std::path::{Path, PathBuf};
use std::process::Output;

use link_test_kit::TempDir;

fn script(path: &Path, body: &str) {
    link_test_kit::write_program(path, format!("#!/bin/sh\n{body}\n")).unwrap();
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A provider's answer to each operation, in its envelope at
/// `$PROTOCOL` (the box.machine answers protocol 1 unless set): its host
/// requires a VM, and it has no boxes yet.
const PROVIDER: &str = r#"[ "$*" = "--link-answer box.machine" ] || { echo "$0: no $*" >&2; exit 1; }
request=$(cat)
op=$(printf '%s' "$request" | sed 's/.*"op":"\([a-z-]*\)".*/\1/')
echo "$0 $op" >> "$HOME/provider.log"
case "$request" in '{"protocol":1,"question":{'*) ;; *) echo "$0: not enveloped: $request" >&2; exit 1 ;; esac
case "$op" in
  policy) reply='{"version":1,"result":{"Ok":{"kind":"policy","value":{"revision":1,"vm_required":true}}}}' ;;
  list) reply='{"version":1,"result":{"Ok":{"kind":"bindings","value":{"bindings":[],"pending":[]}}}}' ;;
  lookup) reply='{"version":1,"result":{"Ok":{"kind":"not-managed"}}}' ;;
  *) reply='{"version":1,"result":{"Err":"not in this stand-in"}}' ;;
esac
echo '{"protocol":'"${PROTOCOL:-1}"',"answer":'"$reply"'}'"#;

struct World {
    _temp: TempDir,
    home: PathBuf,
    bin: PathBuf,
}

impl World {
    /// A world whose installed modules are `providers`, each answering
    /// `box.machine` with its program beside `lnk`, `lnk-<provider>`.
    fn new(test: &str, providers: &[&str]) -> Self {
        let temp = TempDir::new(&format!("provider-{test}"));
        let home = temp.join("home");
        let bin = temp.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let listed = providers
            .iter()
            .map(|p| format!(r#"{{"name":"{p}","installed":true,"answers":["box.machine"]}}"#))
            .collect::<Vec<_>>()
            .join(",");
        for p in providers {
            script(&bin.join(format!("lnk-{p}")), PROVIDER);
        }
        script(
            &bin.join("lnk"),
            &format!(
                r#"case "$*" in
  "module list --json") echo '[{listed}]' ;;
  box\ *) shift; exec '{lnk_box}' "$@" ;;
  *) echo "lnk: no $*" >&2; exit 1 ;;
esac"#,
                lnk_box = env!("CARGO_BIN_EXE_lnk-box"),
            ),
        );
        World {
            _temp: temp,
            home,
            bin,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, &[])
    }

    fn run_with(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        link_test_kit::env::command(self.bin.join("lnk"), &self.home)
            .args(args)
            .envs(env.iter().copied())
            .env("LNK_EXE", self.bin.join("lnk"))
            .env("LNK_SERVICES", "off")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }

    fn asked(&self) -> String {
        std::fs::read_to_string(self.home.join("provider.log")).unwrap_or_default()
    }
}

/// With no module answering `box.machine`, nothing requires a VM: a box
/// here is Box's own, and listing asks no provider.
#[test]
fn with_no_provider_a_box_here_is_boxs_own() {
    let world = World::new("none", &[]);
    let made = world.run(&["box", "create", "--here", "--name", "h1", "--yes"]);
    assert!(made.status.success(), "{}", text(&made));
    assert!(world.home.join("Link/Boxes/h1/home").is_dir());
    let listed = world.run(&["box", "list", "--here", "--json"]);
    assert!(listed.status.success(), "{}", text(&listed));
    assert!(world.asked().is_empty(), "{}", world.asked());
}

/// The module that answers `box.machine` is asked, whatever its name:
/// its host's policy requires a VM, so a box here is a VM box, and one
/// asked for with no sandbox is refused, never made without its VM.
#[test]
fn the_module_answering_the_question_is_asked_and_its_policy_kept() {
    let world = World::new("one", &["vmfake"]);
    let listed = world.run(&["box", "list", "--here", "--json"]);
    assert!(listed.status.success(), "{}", text(&listed));
    assert!(world.asked().contains("list"), "{}", world.asked());
    let refused = world.run(&[
        "box",
        "create",
        "--here",
        "--name",
        "h1",
        "--isolation.no_sandbox",
        "--yes",
    ]);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("Couldn't make a VM box with those options"),
        "{}",
        text(&refused)
    );
    assert!(world.asked().contains("policy"), "{}", world.asked());
    assert!(!world.home.join("Link/Boxes/h1").exists());
}

/// Two modules answering the question are refused, rather than either
/// chosen.
#[test]
fn two_providers_are_refused() {
    let world = World::new("two", &["vm-one", "vm-two"]);
    let refused = world.run(&["box", "create", "--here", "--name", "h1", "--yes"]);
    assert!(!refused.status.success(), "{}", text(&refused));
    let said = text(&refused);
    assert!(
        said.contains("more than one module answers box.machine"),
        "{said}"
    );
    assert!(said.contains("vm-one, vm-two"), "{said}");
    assert!(world.asked().is_empty(), "{}", world.asked());
    assert!(!world.home.join("Link/Boxes/h1").exists());
}

/// A provider speaking a box.machine protocol below the oldest Box
/// accepts is refused by name, its answer unread.
#[test]
fn a_provider_below_the_oldest_protocol_is_refused_by_name() {
    let world = World::new("older", &["vmfake"]);
    let refused = world.run_with(&["box", "list", "--here", "--json"], &[("PROTOCOL", "0")]);
    assert!(!refused.status.success(), "{}", text(&refused));
    let said = text(&refused);
    assert!(
        said.contains("the vmfake module speaks the box machine answers protocol 0"),
        "{said}"
    );
    assert!(said.contains("lnk upgrade"), "{said}");
}
