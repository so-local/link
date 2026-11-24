//! Setting a box up with fakes: `lnk` (for `lnk cloud env`), a cloud adapter
//! (`lnk-fake`) and `ssh`, which runs what it's
//! given in a folder standing for the box's home. `lnk box copy` puts
//! files there.

use std::path::{Path, PathBuf};
use std::process::Command;

use link_test_kit::TempDir;

use link_test_kit::release;

fn script(path: &Path, body: &str) {
    link_test_kit::write_program(path, format!("#!/bin/sh\n{body}\n")).unwrap();
}

const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c";

struct World {
    dir: TempDir,
    bin: PathBuf,
    releases: release::Releases,
}

impl World {
    /// A world with box web-1, made.
    fn new(test: &str) -> Option<Self> {
        Self::serving(test, &[])
    }

    /// A world with box web-1, made, its releases serving `versions` too.
    fn serving(test: &str, versions: &[&str]) -> Option<Self> {
        let dir = TempDir::new(&format!("setup-{test}"));
        let bin = dir.join("bin");
        let there = dir.join("there");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(dir.join("home/.lnk/safe/accounts")).unwrap();
        std::fs::write(dir.join("home/.lnk/safe/accounts/accounts.toml"), "").unwrap();
        std::fs::create_dir_all(there.join(".lnk/exec/bin")).unwrap();
        let at = |name: &str| dir.join(name).display().to_string();
        script(
            &bin.join("lnk"),
            &format!(
                r#"case "$*" in
  "cloud env acct --json") echo '{{"name": "acct", "kind": "fake", "place": "moon-1", "env": {{}}}}' ;;
  "cloud list --json") echo '[{{"name": "acct", "kind": "fake"}}]' ;;
  "module list --json") echo '[{{"name": "login", "installed": true, "answers": ["lnk.account"]}}]' ;;
  box\ *) shift; exec '{lnk_box}' "$@" ;;
  *) echo "lnk: no $*" >&2; exit 1 ;;
esac"#,
                lnk_box = env!("CARGO_BIN_EXE_lnk-box"),
            ),
        );
        script(
            &bin.join("lnk-fake"),
            &format!(
                r#"request=$(cat); printf '%s %s\n' "$1" "$request" >> {adapter}
case "$1" in
  defaults) echo '{{"place": "moon-1", "size": "tiny"}}' ;;
  launch) echo '{{"id": "m-1"}}' ;;
  wait|start) echo '{{"ip": "192.0.2.11"}}' ;;
  state) echo '{{"state": "running"}}' ;;
  hostkeys) echo '{{"host_keys": ["ssh-ed25519 {KEY}"]}}' ;;
  *) echo '{{}}' ;;
esac"#,
                adapter = at("adapter.log"),
            ),
        );
        // The login module, beside lnk: who is signed in, asked directly.
        script(
            &bin.join("lnk-login"),
            r#"case "$*" in
  "--link-answer lnk.account") cat > /dev/null; echo '{"protocol": 1, "answer": {"username": "me", "relay": "https://relay.example", "owner": "github-7"}}' ;;
  *) echo "lnk-login: no $*" >&2; exit 1 ;;
esac"#,
        );
        // The box's lnk: says what it was asked.
        script(
            &there.join(".lnk/exec/bin/lnk"),
            &format!(r#"echo "$*" >> {ran}"#, ran = at("ran-there.log")),
        );
        // ssh: what it's given, run in the box's home; a setup script
        // kept in scripts.log, not run, but the installation ID asked of it.
        script(
            &bin.join("ssh"),
            &format!(
                r#"echo "$*" >> {ssh}
case "$*" in *"-O check"*) exit 255 ;; *"-O "*|*ControlMaster=yes*) exit 0 ;; esac
for last; do :; done
if [ "$last" = -s ]; then
  body=$(cat); printf '%s\n' "$body" >> {scripts}
  case "$body" in *installation.toml*) echo 'lnk-installation abcdefabcdef' ;; esac
  exit 0
fi
cd {there} && HOME={there} exec sh -c "$last""#,
                ssh = at("ssh.log"),
                scripts = at("scripts.log"),
                there = there.display(),
            ),
        );
        let releases = release::serve(&format!("setup-{test}"), versions)?;
        let world = World { dir, bin, releases };
        world.ok(&[
            "box",
            "create",
            "--cloud",
            "acct",
            "--name",
            "web-1",
            "--yes",
            "--lnk.login=false",
        ]);
        Some(world)
    }

    /// The folder standing for the box's home.
    fn there(&self) -> PathBuf {
        self.dir.join("there")
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.run_with(args, &[])
    }

    /// [`World::run`], with `env` set over the world's.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn run_with(&self, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_lnk-box"))
            // lnk hands lnk-box the words after its namespace.
            .args(args.strip_prefix(&["box"][..]).unwrap_or(args))
            .current_dir(&self.dir)
            .env("HOME", self.dir.join("home"))
            .env("LNK_SERVICES", "off")
            .envs(link_test_kit::keychain::isolated(&self.dir.join("home")))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("LNK_BOX_SSH")
            .env_remove("LNK_BOX")
            .env("LNK_EXE", self.bin.join("lnk"))
            .env("LNK_SSH", self.bin.join("ssh"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env_remove("LNK_VERSION")
            .envs(self.releases.env())
            .envs(env.iter().copied())
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }

    /// What a command that must succeed printed, stdout then stderr.
    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "lnk {args:?}: {said}");
        said
    }

    /// What a command that must fail printed, stdout then stderr.
    fn fails(&self, args: &[&str]) -> String {
        let output = self.run(args);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.status.success(), "lnk {args:?} succeeded: {said}");
        said
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.dir.join(name), text).unwrap();
    }
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn files_are_copied_to_a_box_with_their_names_and_permissions() {
    let Some(world) = World::new("copy") else {
        return;
    };
    world.write("deps.toml", "name = \"deps\"\n");
    world.write("install me.sh", "echo hi\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = world.dir.join("install me.sh");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o750)).unwrap();
        std::fs::set_permissions(
            world.dir.join("deps.toml"),
            std::fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }
    let said = world.ok(&[
        "box",
        "copy",
        "web-1",
        "deps.toml",
        "install me.sh",
        "setup",
    ]);
    assert!(said.contains("Copying 2 files to setup on web-1"), "{said}");
    let there = world.there();
    assert_eq!(
        std::fs::read_to_string(there.join("setup/deps.toml")).unwrap(),
        "name = \"deps\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(there.join("setup/install me.sh")).unwrap(),
        "echo hi\n"
    );
    #[cfg(unix)]
    {
        assert_eq!(mode(&there.join("setup/install me.sh")), 0o750);
        assert_eq!(mode(&there.join("setup/deps.toml")), 0o640);
    }
    // Nothing half-written is left beside them.
    let names: Vec<String> = std::fs::read_dir(there.join("setup"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    // Over the box's own SSH, as `lnk box ssh` reaches it: both files in
    // one session.
    let ssh = world.read("ssh.log");
    assert_eq!(
        ssh.lines().filter(|l| l.contains("lnk-copy")).count(),
        1,
        "{ssh}"
    );
    assert!(
        ssh.lines()
            .any(|l| l.contains("HostKeyAlias=lnk-fake-m-1") && l.contains("ubuntu@192.0.2.11")),
        "{ssh}"
    );

    // `~/` is the box's home; a folder's name is never run.
    world.ok(&["box", "copy", "web-1", "deps.toml", "~/more/$(touch pwned)"]);
    assert!(there.join("more/$(touch pwned)/deps.toml").is_file());
    assert!(!there.join("pwned").exists() && !world.dir.join("pwned").exists());
    // Copied again, it's replaced.
    world.write("deps.toml", "name = \"deps\"\nversion = 2\n");
    world.ok(&["box", "copy", "web-1", "deps.toml", "setup"]);
    assert!(
        std::fs::read_to_string(there.join("setup/deps.toml"))
            .unwrap()
            .contains("version = 2")
    );

    // A folder, two files of one name, or a missing file: refused, and
    // nothing copied.
    std::fs::create_dir_all(world.dir.join("folder/sub")).unwrap();
    world.write("folder/deps.toml", "other\n");
    let before = world.read("ssh.log").lines().count();
    let folder = world.fails(&["box", "copy", "web-1", "folder", "setup"]);
    assert!(folder.contains("It isn't a file"), "{folder}");
    let twice = world.fails(&[
        "box",
        "copy",
        "web-1",
        "deps.toml",
        "folder/deps.toml",
        "setup",
    ]);
    assert!(twice.contains("named like another file"), "{twice}");
    let missing = world.fails(&["box", "copy", "web-1", "nope.toml", "setup"]);
    assert!(
        missing.contains("Couldn't copy nope.toml") && missing.contains("It isn't a file"),
        "{missing}"
    );
    // A link is refused, never followed to what it points to.
    #[cfg(unix)]
    {
        world.write("id_ed25519", "PRIVATE KEY\n");
        std::os::unix::fs::symlink(world.dir.join("id_ed25519"), world.dir.join("notes.md"))
            .unwrap();
        let link = world.fails(&["box", "copy", "web-1", "notes.md", "setup"]);
        assert!(
            link.contains("Couldn't copy notes.md") && link.contains("It is a link"),
            "{link}"
        );
        // So is a folder on the way to it, planted in your home.
        std::fs::create_dir_all(world.dir.join("home/files")).unwrap();
        std::os::unix::fs::symlink(&world.dir, world.dir.join("home/files/app")).unwrap();
        let through = world.fails(&["box", "copy", "web-1", "home/files/app/id_ed25519", "setup"]);
        assert!(
            through.contains("files/app is a link on the way to it"),
            "{through}"
        );
    }
    assert_eq!(world.read("ssh.log").lines().count(), before);
    // A box this computer doesn't know.
    let unknown = world.fails(&["box", "copy", "web-9", "deps.toml", "setup"]);
    assert!(unknown.contains("web-9"), "{unknown}");
}

/// A box follows this machine's releases: one made installs the version
/// asked of the releases asked, and an upgrade run on it is from them too.
#[test]
fn a_setups_boxes_follow_this_machines_releases() {
    // A staging build of the next version: what a test machine follows.
    let version = "99.0.0-staging.202610141200";
    let Some(world) = World::serving("follows", &[version]) else {
        return;
    };
    for args in [
        &[
            "box",
            "create",
            "--cloud",
            "acct",
            "--name",
            "web-2",
            "--yes",
            "--lnk.login=false",
        ][..],
        &["box", "run", "web-2", "--", "upgrade", version],
    ] {
        let said = world.run_with(args, &[("LNK_VERSION", version)]);
        assert!(
            said.status.success(),
            "{}{}",
            String::from_utf8_lossy(&said.stdout),
            String::from_utf8_lossy(&said.stderr)
        );
    }
    let url = &world.releases.url;
    let installed = world.read("scripts.log");
    assert!(
        installed.contains(&format!(
            "LNK_RELEASES_URL='{url}' LNK_VERSION='{version}' sh <<'LNK_CHECKED_INSTALL_SH'"
        )),
        "{installed}"
    );
    let ssh = world.read("ssh.log");
    assert!(
        ssh.contains(&format!(
            "LNK_RELEASES_URL='{url}' \"$HOME/.lnk/exec/bin/lnk\" 'upgrade' '{version}'"
        )),
        "{ssh}"
    );
}

/// A box installs public programs without an account module.
#[test]
fn a_box_installs_public_programs_without_an_account_module() {
    let Some(world) = World::new("public-programs") else {
        return;
    };
    world.ok(&[
        "box", "create", "--cloud", "acct", "--name", "web-2", "--yes",
    ]);
    let scripts = world.read("scripts.log");
    assert!(scripts.contains("module add secret sandbox"), "{scripts}");
    assert!(!scripts.contains("auth"), "{scripts}");
    assert!(!world.read("ssh.log").contains("auth login"));
}

/// Releases where only this machine reads them, a bucket of yours read
/// with its cloud's CLI as you're logged in to it: the box is given a
/// copy, each file checked here against the signed sums, and installs
/// from that, never told where the bucket is. A file that doesn't match
/// its signed sum, or a bucket that can't be read, stops it before any
/// box is made.
#[test]
fn a_box_is_given_the_releases_only_this_machine_reads() {
    let version = link_version::version().to_string();
    let Some(world) = World::serving("bucket", &[&version]) else {
        return;
    };
    // aws: `s3 cp` of an object of s3://my-builds/dev to its output, from
    // the folder the releases are served from, as a bucket holding them.
    script(
        &world.bin.join("aws"),
        &format!(
            r#"echo "$* ${{AWS_SECRET_ACCESS_KEY:-no login}}" >> {log}
[ "$1 $2 $3 $5" = "s3 cp --only-show-errors -" ] || exit 2
path=${{4#s3://my-builds/dev/}}
case "$path" in *..*) exit 2 ;; esac
[ "$path" != "$4" ] && [ -f "{root}/$path" ] || {{ echo 'An error occurred (404) when calling the HeadObject operation: Not Found' >&2; exit 1; }}
cat "{root}/$path""#,
            log = world.dir.join("aws.log").display(),
            root = world.releases.root.display(),
        ),
    );
    let bucket = [
        ("LNK_RELEASES_URL", "s3://my-builds/dev"),
        ("AWS_SECRET_ACCESS_KEY", "the-login"),
    ];
    let create = |name: &str| {
        world.run_with(
            &[
                "box",
                "create",
                "--cloud",
                "acct",
                "--name",
                name,
                "--yes",
                "--lnk.login=false",
            ],
            &bucket,
        )
    };
    let said = create("web-2");
    assert!(
        said.status.success(),
        "{}{}",
        String::from_utf8_lossy(&said.stdout),
        String::from_utf8_lossy(&said.stderr)
    );
    // Its copy: the signed sums and signature, VERSION, install.sh and
    // the archives for its processor, as they are in the bucket.
    let given = world
        .there()
        .join(format!(".lnk/safe/releases/download/v{version}"));
    let from = world.releases.root.join(format!("download/v{version}"));
    let mut names: Vec<String> = std::fs::read_dir(&given)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "SHA256SUMS",
            "SHA256SUMS.sig",
            "VERSION",
            "install.sh",
            "lnk-agent-x86_64-unknown-linux-musl.tar.gz",
            "lnk-core-x86_64-unknown-linux-musl.tar.gz",
        ]
    );
    for name in &names {
        assert_eq!(
            std::fs::read(given.join(name)).unwrap(),
            std::fs::read(from.join(name)).unwrap(),
            "{name}"
        );
    }
    // It installs from that copy, and every lnk run there reads it.
    let staged = "LNK_RELEASES_URL=\"file://$HOME/.lnk/safe/releases\" ";
    let installed = world.read("scripts.log");
    assert!(
        installed.contains(&format!(
            "{staged}LNK_VERSION='{version}' sh <<'LNK_CHECKED_INSTALL_SH'"
        )),
        "{installed}"
    );
    assert!(
        installed.contains(&format!("{staged}\"$HOME/.lnk/exec/bin/lnk\" module add")),
        "{installed}"
    );
    let _ = world.run_with(&["box", "run", "web-2", "--", "agent", "show"], &bucket);
    let ssh = world.read("ssh.log");
    assert!(
        ssh.contains(&format!(
            "{staged}\"$HOME/.lnk/exec/bin/lnk\" 'agent' 'show'"
        )),
        "{ssh}"
    );
    assert!(!format!("{installed}{ssh}").contains("my-builds"));
    assert!(
        world
            .read("aws.log")
            .contains("s3://my-builds/dev/download/v")
    );

    // An archive that isn't what was signed: no box.
    let launches = || world.read("adapter.log").matches("launch ").count();
    let before = launches();
    let archive = from.join("lnk-agent-x86_64-unknown-linux-musl.tar.gz");
    let kept = std::fs::read(&archive).unwrap();
    std::fs::write(&archive, b"tampered").unwrap();
    let tampered = create("web-3");
    let said = String::from_utf8_lossy(&tampered.stderr);
    assert!(!tampered.status.success());
    assert!(said.contains("checksum mismatch"), "{said}");
    std::fs::write(&archive, kept).unwrap();
    // A bucket that isn't there, or isn't yours: what aws said, and no box.
    let missing = world.run_with(
        &[
            "box",
            "create",
            "--cloud",
            "acct",
            "--name",
            "web-4",
            "--yes",
            "--lnk.login=false",
        ],
        &[("LNK_RELEASES_URL", "s3://my-builds/elsewhere")],
    );
    let said = String::from_utf8_lossy(&missing.stderr);
    assert!(!missing.status.success());
    assert!(said.contains("Not Found"), "{said}");
    // Nor a location lnk doesn't read.
    let other = world.run_with(
        &[
            "box",
            "create",
            "--cloud",
            "acct",
            "--name",
            "web-5",
            "--yes",
            "--lnk.login=false",
        ],
        &[("LNK_RELEASES_URL", "s3://my-builds/../dev")],
    );
    let said = String::from_utf8_lossy(&other.stderr);
    assert!(!other.status.success());
    assert!(said.contains("LNK_RELEASES_URL"), "{said}");
    assert_eq!(launches(), before);
}
