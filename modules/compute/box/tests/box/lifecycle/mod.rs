//! A box's life with fakes: `lnk` (for `lnk cloud env`), a cloud adapter
//! (`lnk-fake`) and `ssh`. What each is asked,
//! and what `lnk box` keeps.

use std::path::{Path, PathBuf};

use link_test_kit::TempDir;

use link_test_kit::release;

mod inputs;
mod living;
mod reaching;
mod signing_in;
mod specs;

fn script(path: &Path, body: &str) {
    link_test_kit::write_program(path, format!("#!/bin/sh\n{body}\n")).unwrap();
}

const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c";

struct World {
    dir: PathBuf,
    /// `lnk-box`'s `HOME`.
    home: PathBuf,
    bin: PathBuf,
    /// The `lnk-box` run: the adapters beside it are found first.
    exe: PathBuf,
    /// `dir`, removed when the world goes.
    _temp: TempDir,
    /// The releases its boxes install lnk from.
    releases: release::Releases,
}

impl World {
    /// `console`: whether the adapter reads the box's host keys (else it
    /// fails, as an adapter without `hostkeys` does).
    fn new(test: &str, console: bool) -> Option<Self> {
        let temp = TempDir::new(&format!("lifecycle-{test}"));
        let home = temp.join("home");
        Self::at(temp, home, console)
    }

    fn at(temp: TempDir, home: PathBuf, console: bool) -> Option<Self> {
        let dir = temp.to_path_buf();
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(home.join(".lnk/safe/accounts")).unwrap();
        std::fs::write(home.join(".lnk/safe/accounts/accounts.toml"), "").unwrap();
        let log = |name: &str| dir.join(name).display().to_string();
        script(
            &bin.join("lnk"),
            r#"case "$*" in
  "cloud env acct --json") echo '{"name": "acct", "kind": "fake", "place": "moon-1", "env": {"FAKE_KEY": "k"}}' ;;
  "cloud env awsacct --json") echo '{"name": "awsacct", "kind": "aws", "place": "moon-1", "env": {}}' ;;
  "cloud list --json") echo '[{"name": "acct", "kind": "fake"}]' ;;
  "module list --json") echo '[{"name": "login", "installed": true, "answers": ["lnk.account"]}, {"name": "access", "installed": true}, {"name": "holder", "installed": true, "answers": ["box.removed"]}]' ;;
  # What this machine grants a box: the exact arguments, logged.
  "access machine add --json --name "*)
    echo "$*" >> "$HOME/access.log"
    echo '{"owner": "github-7", "username": "me", "account_id": "7", "machine": {"id": "m-7", "name": "acct-1"}}' ;;
  *) echo "lnk: no $*" >&2; exit 1 ;;
esac"#,
        );
        // The login module, beside lnk: who is signed in, asked directly,
        // signed in while its login is there.
        script(
            &bin.join("lnk-login"),
            r#"case "$*" in
  "--link-answer lnk.account") cat > /dev/null
    if [ -e "$HOME/.lnk/safe/accounts/accounts.toml" ]; then
      echo '{"protocol": 1, "answer": {"username": "me", "relay": "https://relay.example", "owner": "github-7"}}'
    else echo '{"protocol": 1, "answer": {"username": null, "relay": "https://relay.example", "owner": null}}'; fi ;;
  *) echo "lnk-login: no $*" >&2; exit 1 ;;
esac"#,
        );
        // A module that gave boxes something: asked what it takes back
        // once one is deleted, it logs the question and says what stays
        // while `holder-keeps` is there.
        script(
            &bin.join("lnk-holder"),
            &format!(
                r#"case "$*" in
  "--link-answer box.removed") cat >> {removed}; echo >> {removed}
    if [ -e "$(dirname "$0")/../holder-keeps" ]; then
      echo '{{"protocol": 1, "answer": {{"stays": ["The key b1 had may still work."]}}}}'
    else echo '{{"protocol": 1, "answer": {{"stays": []}}}}'; fi ;;
  *) echo "lnk-holder: no $*" >&2; exit 1 ;;
esac"#,
                removed = log("removed.log"),
            ),
        );
        // A module granting machines access, found beside the rest; `lnk` answers for it.
        script(&bin.join("lnk-access"), "exit 1");
        // The adapter logs each command and its request.
        script(
            &bin.join("lnk-fake"),
            &format!(
                r#"request=$(cat); printf '%s %s\n' "$1" "$request" >> {adapter}
case "$1" in
  defaults) echo '{{"place": "moon-1", "size": "tiny"}}' ;;
  launch) if [ -f "$(dirname "$0")/../slow" ]; then sleep 1; echo '{{"id": "m-'$$'"}}'; else echo '{{"id": "m-1"}}'; fi ;;
  wait|start) if [ -f "$(dirname "$0")/../slow" ]; then sleep 1; fi; echo '{{"ip": "192.0.2.10"}}' ;;
  state) cat "$(dirname "$0")/../state.json" 2>/dev/null || echo '{{"state": "running"}}' ;;
  hostkeys) {hostkeys} ;;
  list) cat "$(dirname "$0")/../cloud.json" 2>/dev/null || echo '{{}}' ;;
  sizes) if [ -f "$(dirname "$0")/../no-sizes" ]; then echo 'Error: lnk-fake has no command "sizes"' >&2; exit 1;
    elif [ -f "$(dirname "$0")/../sizes-fail" ]; then echo 'Error: AccessDenied: not allowed to list types' >&2; exit 1;
    else cat "$(dirname "$0")/../sizes.json" 2>/dev/null || echo '{{}}'; fi ;;
  describe) cat "$(dirname "$0")/../describe.json" 2>/dev/null || echo '{{}}' ;;
  add-key) touch "$(dirname "$0")/../let-in"; echo '{{"temporary": true}}' ;;
  *) echo '{{}}' ;;
esac"#,
                adapter = log("adapter.log"),
                hostkeys = if console {
                    format!(r#"echo '{{"host_keys": ["ssh-ed25519 {KEY}"]}}'"#)
                } else {
                    "echo 'no hostkeys here' >&2; exit 1".into()
                },
            ),
        );
        // ssh: logs its arguments and whatever it's given to run; a box
        // made elsewhere lets this computer in only once its key is added
        // (`let-in`); a script giving a box its ID is answered with one. A
        // shared connection has a `master/<destination>` entry; the
        // directory goes once the last connection closes.
        script(
            &bin.join("ssh"),
            &format!(
                r#"[ "$1" = -G ] && exit 0
echo "$*" >> {ssh}
destination=''
for argument do
  case "$argument" in ubuntu@*) destination=$argument; break ;; esac
done
[ -n "$destination" ] || exit 255
masters="{dir}/master"
# A certificate alone gets in once the box trusts this computer's
# authority: its first boot's (not modelled), or the script giving it.
case "$*" in *cert-v01@openssh.com*) [ -f {dir}/trusted ] || exit 255; exit 0 ;; esac
master="$masters/$destination"
case "$*" in
  *"-O check"*) [ -f "$master" ] || exit 255; exit 0 ;;
  *"-O exit"*) rm -f "$master"; rmdir "$masters" 2>/dev/null || :; exit 0 ;;
  *ControlMaster=yes*192.0.2.20) [ -f {dir}/let-in ] || exit 255; mkdir -p "$masters"; touch "$master"; exit 0 ;;
  *ControlMaster=yes*) mkdir -p "$masters"; touch "$master"; exit 0 ;;
esac
case "$*" in *192.0.2.20\ true) [ -f {dir}/let-in ] || exit 255 ;; esac
if [ ! -t 0 ]; then input=$(cat); printf '%s
' "$input" >> {ssh}; case "$input" in *"getent group lnk-users"*) [ -f {dir}/users ] && echo alice ;; *"auth key"*) echo '{{"public_key": "ssh-ed25519 PUB"}}' ;; *installation.toml*) echo 'lnk-installation abcdefabcdef' ;; *user-cas.pub*) touch {dir}/trusted ;; esac; fi
exit 0"#,
                ssh = log("ssh.log"),
                dir = dir.display(),
            ),
        );
        let releases = release::serve(&temp.file_name().unwrap().to_string_lossy(), &["0.0.0"])?;
        Some(World {
            dir,
            home,
            bin,
            exe: PathBuf::from(env!("CARGO_BIN_EXE_lnk-box")),
            releases,
            _temp: temp,
        })
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.run_with(args, &[])
    }

    /// [`World::run`], with these variables set too.
    fn run_with(&self, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
        link_test_kit::env::command(&self.exe, &self.home)
            // lnk hands lnk-box the words after its namespace.
            .args(args.strip_prefix(&["box"][..]).unwrap_or(args))
            .env("HOME", &self.home)
            .env("LNK_SERVICES", "off")
            .envs(link_test_kit::keychain::isolated(&self.home))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("LNK_BOX_SSH")
            .env_remove("LNK_RELEASES_URL")
            .env_remove("LNK_VERSION")
            .env("LNK_EXE", self.bin.join("lnk"))
            .env("LNK_SSH", self.bin.join("ssh"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .envs(self.releases.env())
            .envs(env.iter().copied())
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }

    fn lnk_box(&self, args: &[&str]) -> std::process::Output {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "lnk box {args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    /// [`World::lnk_box`], for a command that must fail.
    fn lnk_box_fails(&self, args: &[&str]) -> std::process::Output {
        let output = self.run(args);
        assert!(!output.status.success(), "lnk box {args:?} succeeded");
        output
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }
}
