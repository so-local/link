//! A cloud box's machine inputs: `--env` captured once, set up inside
//! the box and kept for `lnk box start`, and a VM refused visibly, here
//! and on a cloud box: this build has no VM provider.

use super::*;

/// An environment with one stage at every start, which writes `started`
/// in the box's home each time it runs.
const ENV: &str = r#"schema = 1
name = "app"
version = 1

[[stage]]
name = "hello"
run = "echo started >> $HOME/started"
sandbox = "run.toml"
when = "start"
"#;

/// [`World`] with the box's side of an environment: `lnk box copy` and
/// `lnk box run` reach `guest/home`, where `lnk env here` is the
/// workspace's own, and the rest goes to the world's `lnk`.
fn with_guest(test: &str) -> Option<World> {
    let world = World::new(test, true)?;
    let dir = world.dir.display().to_string();
    std::fs::rename(world.bin.join("lnk"), world.bin.join("lnk-base")).unwrap();
    let guest = world.dir.join("guest");
    std::fs::create_dir_all(guest.join("home")).unwrap();
    // What a box's first boot writes, saying it's a box.
    let kind = guest.join("kind.toml");
    std::fs::write(
        &kind,
        "kind = \"box\"\nos = \"ubuntu-24.04\"\narch = \"x86_64\"\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        for (path, mode) in [(guest.as_path(), 0o755), (kind.as_path(), 0o644)] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
    }
    let lnk_env = link_test_kit::programs().join("lnk-env");
    script(
        &world.bin.join("lnk"),
        &format!(
            r#"root={dir}
echo "lnk $*" >> $root/lnk.log
case "$1 $2" in
  "box list") echo '[{{"name": "b1", "account": "acct", "kind": "fake", "place": "moon-1", "id": "m-1", "size": "tiny"}}]' ;;
  "box copy")
    shift 3; for a; do last=$a; done
    mkdir -p "$root/guest/home/$last"
    n=$#; i=1; for a; do [ $i -lt $n ] && cp "$a" "$root/guest/home/$last/"; i=$((i + 1)); done ;;
  "box run")
    shift 4
    case "$1 $2" in
      "env here") cd "$root/guest/home" || exit 1
        HOME="$root/guest/home" LNK_EXE="$root/bin/guest-lnk" LNK_TEST_MACHINE_KIND="$root/guest/kind.toml" exec {lnk_env} here ;;
      *) echo "lnk $*" >> $root/guest.log ;;
    esac ;;
  *) exec "$root/bin/lnk-base" "$@" ;;
esac"#,
            lnk_env = lnk_env.display(),
        ),
    );
    // The box's own lnk: its sandbox, a stand-in its own tests cover,
    // runs the stage's command as it is.
    script(
        &world.bin.join("guest-lnk"),
        r#"case "$1 $2" in
  "sandbox run") shift 2; while [ $# -gt 0 ]; do case "$1" in --) shift; break ;; *) shift ;; esac; done; exec "$@" ;;
  *) echo "box lnk: no $*" >&2; exit 1 ;;
esac"#,
    );
    std::fs::create_dir_all(world.dir.join("project")).unwrap();
    std::fs::write(world.dir.join("project/env.toml"), ENV).unwrap();
    std::fs::write(
        world.dir.join("project/run.toml"),
        "schema = 1\nname = \"s\"\nversion = 1\n",
    )
    .unwrap();
    Some(world)
}

impl World {
    fn env_file(&self) -> String {
        self.dir.join("project/env.toml").display().to_string()
    }
}

#[test]
fn a_cloud_box_takes_an_environment_set_up_inside_it_and_started_again() {
    let Some(world) = with_guest("cloud-env") else {
        return;
    };
    let env = world.env_file();
    let made = world.run(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "b1",
        "--env",
        &env,
        "--yes",
        "--lnk.login=false",
    ]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&made.stdout),
        String::from_utf8_lossy(&made.stderr)
    );
    assert!(made.status.success(), "{said}");
    assert!(said.contains("It runs the environment app"), "{said}");
    assert!(said.contains("b1 is up as"), "{said}");
    // The machine was launched, readied, and the environment ran inside
    // it, its file copied there, never on this computer.
    assert!(world.read("adapter.log").contains("launch "));
    let guest = world.read("guest.log");
    assert!(guest.contains("lnk module add env sandbox"), "{guest}");
    assert!(guest.contains("lnk sandbox check --ready --yes"), "{guest}");
    assert_eq!(world.read("guest/home/started"), "started\n");
    assert!(!world.home.join("started").exists());
    // What it was made with is kept on its record, by its content.
    let record = std::fs::read_to_string(world.home.join(".lnk/safe/boxes/boxes.toml")).unwrap();
    assert!(record.contains("[box.environment"), "{record}");
    assert!(record.contains(&env), "{record}");

    // Started again, its environment runs from what was captured, even
    // though its source changed since.
    std::fs::write(world.dir.join("project/env.toml"), "not an environment").unwrap();
    let started = world.run(&["box", "start", "b1"]);
    assert!(
        started.status.success(),
        "{}{}",
        String::from_utf8_lossy(&started.stdout),
        String::from_utf8_lossy(&started.stderr)
    );
    assert_eq!(world.read("guest/home/started"), "started\nstarted\n");
}

#[test]
fn a_vm_is_refused_before_anything_is_made_here_or_in_a_cloud() {
    let Some(world) = with_guest("cloud-vm") else {
        return;
    };
    std::fs::write(
        world.dir.join("project/vm.toml"),
        "schema = 1\nversion = 1\n",
    )
    .unwrap();
    std::fs::write(
        world.dir.join("project/box.toml"),
        "schema = 1\n[isolation]\nvm = \"vm.toml\"\n",
    )
    .unwrap();
    let spec = world.dir.join("project/box.toml").display().to_string();
    let vm = world.dir.join("project/vm.toml").display().to_string();
    let env = world.env_file();
    for place in [vec!["--cloud", "acct"], vec!["--here"]] {
        // No flag names a VM.
        let mut create = vec![
            "box",
            "create",
            "--name",
            "b1",
            "--yes",
            "--vm",
            vm.as_str(),
        ];
        create.extend(place.iter().copied());
        let refused = world.lnk_box_fails(&create);
        let said = String::from_utf8_lossy(&refused.stderr);
        assert!(said.contains("--vm"), "{said}");
        // A spec naming one is refused, with or without an environment,
        // and nothing is made in its place.
        for extra in [vec![], vec!["--env", env.as_str()]] {
            let mut create = vec!["box", "create", "--name", "b1", "--yes", "--spec", &spec];
            create.extend(place.iter().copied());
            create.extend(extra);
            let refused = world.lnk_box_fails(&create);
            let said = String::from_utf8_lossy(&refused.stderr);
            assert!(said.contains("no VM provider"), "{said}");
            assert!(said.contains("nothing is made in its place"), "{said}");
        }
    }
    assert!(!world.read("adapter.log").contains("launch "));
    assert!(world.read("guest.log").is_empty());
    assert!(!world.home.join("Link/Boxes/b1").exists());
}

#[test]
fn an_environment_with_an_image_of_its_own_refuses_another_image() {
    let Some(world) = with_guest("cloud-image") else {
        return;
    };
    std::fs::write(
        world.dir.join("project/env.toml"),
        "schema = 1\nname = \"app\"\nversion = 1\n\n[[stage]]\nname = \"deps\"\nrun = \"true\"\nuser = \"root\"\n",
    )
    .unwrap();
    let env = world.env_file();
    let refused = world.lnk_box_fails(&[
        "box", "create", "--cloud", "acct", "--name", "b1", "--env", &env, "--image", "mine",
        "--yes",
    ]);
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        said.contains("--image and --image-hash don't apply"),
        "{said}"
    );
    assert!(!world.read("adapter.log").contains("launch "));
}

#[test]
fn an_environment_for_another_os_than_the_spec_is_refused() {
    let Some(world) = with_guest("cloud-os") else {
        return;
    };
    std::fs::write(
        world.dir.join("project/env.toml"),
        format!("os = \"ubuntu-22.04\"\n{ENV}"),
    )
    .unwrap();
    let env = world.env_file();
    let refused = world.lnk_box_fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "b1",
        "--env",
        &env,
        "--machine.os",
        "ubuntu-24.04",
        "--yes",
    ]);
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("a box of another OS is refused"), "{said}");
    assert!(!world.read("adapter.log").contains("launch "));
}
