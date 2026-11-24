//! Building an environment's image, within its limits.
use link_env_fixture::*;

/// An image is a box's: `lnk env image create --here` is refused on a
/// computer before anything runs, naming `lnk env image create <cloud>`; on a box
/// (what `/etc/lnk/kind.toml` says, here a stand-in), the stages saved
/// in the image run there, its root stage as root, and it says how the
/// computer that made it saves it.
#[test]
fn an_image_is_built_here_on_a_box_alone() {
    if !box_identity() {
        return;
    }
    let world = World::new("image-here");
    let out = world.run(&[
        "env",
        "image",
        "create",
        "--here",
        "--spec",
        &world.env_arg(),
    ]);
    assert!(!out.status.success());
    let said = text(&out.stderr);
    assert!(
        said.contains("which is a computer") && said.contains("lnk env image create <cloud>"),
        "{said}"
    );
    assert_eq!(world.read("lnk.log"), "");
    assert_eq!(world.read("home/stages"), "");
    let out = world.run(&["env", "image", "create", "--spec", &world.env_arg()]);
    assert!(
        text(&out.stderr).contains("--cloud <cloud> or --here"),
        "{}",
        text(&out.stderr)
    );
    // A file that says it's a box, written by others, is refused too.
    let open = world.dir.join("open");
    let path = stand_in_box(&open);
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o666)).unwrap();
    let out = world
        .command(&[
            "env",
            "image",
            "create",
            "--here",
            "--spec",
            &world.env_arg(),
        ])
        .env("LNK_TEST_MACHINE_KIND", &path)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("anyone who writes it could make this machine a box"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(world.read("home/stages"), "");

    // On a box.
    let out = world
        .command(&[
            "env",
            "image",
            "create",
            "--here",
            "--spec",
            &world.env_arg(),
        ])
        .env("LNK_TEST_MACHINE_KIND", world.dir.join("box/kind.toml"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(world.read("home/stages"), "system\ndeps\n");
    assert_eq!(world.read("as-root"), root_stage(&world));
    assert_eq!(world.read("home/app/README"), "cloned\n");
    // Nothing of a cloud's: no box made, none saved from here.
    assert_eq!(world.read("lnk.log"), "lnk env here\n");
    let hash = hash_of(&world);
    let said = text(&out.stderr);
    assert!(
        said.contains(&format!("--hash {hash}")) && said.contains("lnk box image create"),
        "{said}"
    );
}

/// `lnk env setup` runs the stages an image would save, and saves none:
/// here on a computer, its root stage is refused before any stage runs,
/// the sandboxed one after it included; on a box, here on a stand-in or
/// by its name, every one runs, the root one as root.
#[test]
fn setup_runs_the_build_stages_and_saves_no_image() {
    if !box_identity() {
        return;
    }
    let world = World::new("setup");
    let env = world.env_arg();
    let out = world.run(&["env", "setup", "--spec", &env, "--here"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("stage system runs as root, which happens on a box alone"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(world.read("home/stages"), "");
    assert_eq!(world.read("as-root"), "");
    assert!(!world.dir.join("home/app").exists());

    // Here, on a box.
    let out = world
        .command(&["env", "setup", "--spec", &env, "--here"])
        .env("LNK_TEST_MACHINE_KIND", world.dir.join("box/kind.toml"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(world.read("home/stages"), "system\ndeps\n");
    assert_eq!(world.read("as-root"), root_stage(&world));
    assert_eq!(world.read("home/app/README"), "cloned\n");

    // A box by its name, over SSH.
    std::fs::create_dir_all(world.dir.join("boxes.d/web-1/home")).unwrap();
    std::fs::write(world.dir.join("boxes"), "web-1\n").unwrap();
    let out = world.run(&["env", "setup", "--spec", &env, "--on", "web-1"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(world.read("boxes.d/web-1/home/stages"), "system\ndeps\n");
    let log = world.read("lnk.log");
    assert!(
        !log.contains("image save") && !log.contains("box create"),
        "{log}"
    );
    // A file for another OS than the box's: nothing runs.
    std::fs::write(world.dir.join("boxes.d/web-1/home/stages"), "").unwrap();
    let text_of_env = std::fs::read_to_string(&world.env).unwrap();
    std::fs::write(&world.env, format!("os = \"ubuntu-22.04\"\n{text_of_env}")).unwrap();
    let out = world.run(&["env", "setup", "--spec", &env, "--on", "web-1"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("is for ubuntu-22.04, and this box is ubuntu-24.04"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(world.read("boxes.d/web-1/home/stages"), "");
    std::fs::write(&world.env, text_of_env).unwrap();
    // Nothing that isn't a box.
    let out = world.run(&["env", "setup", "--spec", &env, "--on", "aws"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("aws is a cloud: --cloud aws"),
        "{}",
        text(&out.stderr)
    );
}

/// A box started as the file says: the image built once, on a box made
/// for it and removed; each start from it runs only the stages run at
/// every start, given the variables.
#[test]
fn a_start_builds_the_image_once_then_starts_from_it() {
    if !box_identity() {
        return;
    }
    let world = World::new("start");
    std::fs::write(world.dir.join("services-up"), "").unwrap();
    let out = world.run(&[
        "env",
        "create",
        "--cloud",
        "aws",
        "--spec",
        &world.env_arg(),
        "--yes",
    ]);
    assert!(
        out.status.success(),
        "{}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    let log = world.read("lnk.log");
    assert_eq!(log.matches("box image create").count(), 1, "{log}");
    assert!(
        log.contains("box create --cloud aws --name app-image-"),
        "{log}"
    );
    assert!(
        log.contains(" --deadline 14400s --lnk.login=false\n"),
        "{log}"
    );
    assert!(
        log.contains("box create --cloud aws --name app-1 --yes --image-hash "),
        "{log}"
    );
    // A job's box is never signed in to the account.
    let job_box = log
        .lines()
        .find(|l| l.contains("box create --cloud aws --name app-1 "))
        .unwrap_or_default();
    assert!(job_box.ends_with("--lnk.login=false"), "{job_box}");
    let built = world
        .read("removed")
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    assert!(built.starts_with("app-image-"), "the build box is removed");
    assert_eq!(world.read("boxes"), "app-1\n");
    // The image: cloned, its stages saved, the system one as root.
    assert_eq!(
        world.read("boxes.d/app-1/home/stages"),
        "system\ndeps\npull\n"
    );
    assert_eq!(world.read("boxes.d/app-1/home/app/README"), "cloned\n");
    assert!(
        world
            .read(&format!("boxes.d/{built}/as-root"))
            .contains("echo system")
    );
    // On the way, what it's given sealed, to the key that run of `lnk env
    // here` made for it.
    let stream = world.read("stream-app-1");
    assert!(stream.contains(r#""given":{"about""#), "{stream}");
    // Its service, a background service running `lnk env serve`.
    let unit = world.read("boxes.d/app-1/home/.config/systemd/user/lnk-env-app-db.service");
    assert!(
        unit.contains(r#""env" "serve""#) && unit.contains(r#"envs/app/env.toml" "db""#),
        "{unit}"
    );
    let given = world.everything_in(&format!("boxes.d/{built}/with"));
    assert!(
        given.contains("DATABASE_URL"),
        "the variables are given while it's built"
    );
    // A second box from the same image: no stage but the start's.
    let out = world.run(&[
        "env",
        "create",
        "--cloud",
        "aws",
        "--spec",
        &world.env_arg(),
        "--yes",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(world.read("lnk.log").matches("box image create").count(), 1);
    assert_eq!(
        world.read("boxes.d/app-2/home/stages"),
        "system\ndeps\npull\n"
    );
    // A changed lockfile makes a new image; a changed README doesn't.
    std::fs::write(world.dir.join("project/README.md"), "changed\n").unwrap();
    assert!(
        world
            .run(&[
                "env",
                "create",
                "--cloud",
                "aws",
                "--spec",
                &world.env_arg(),
                "--name",
                "web-3",
                "--yes"
            ])
            .status
            .success()
    );
    assert_eq!(world.read("lnk.log").matches("box image create").count(), 1);
    std::fs::write(world.dir.join("project/requirements.txt"), "flask==3\n").unwrap();
    assert!(
        world
            .run(&[
                "env",
                "create",
                "--cloud",
                "aws",
                "--spec",
                &world.env_arg(),
                "--yes"
            ])
            .status
            .success()
    );
    assert_eq!(world.read("lnk.log").matches("box image create").count(), 2);
}

/// Starts at once of an environment with no image yet build it once: one
/// that finds another's build of it under way waits for that build, then
/// starts from its image, asking for no box of its own to build on.
#[test]
fn a_start_waits_for_a_build_under_way_and_builds_none() {
    let world = World::new("build-under-way");
    std::fs::write(world.dir.join("services-up"), "").unwrap();
    let hash = hash_of(&world);
    let machine = format!("app-image-{}", &hash[..6]);
    // Another start's build: its box, and the build held.
    std::fs::write(world.dir.join("boxes"), format!("{machine}\n")).unwrap();
    let held = link_files::lock_file(
        &world
            .dir
            .join(format!("home/.lnk/safe/envs/building/{machine}.lock")),
    )
    .unwrap();
    let stderr = std::fs::File::create(world.dir.join("stderr")).unwrap();
    let mut child = world
        .command(&[
            "env",
            "create",
            "--cloud",
            "aws",
            "--spec",
            &world.env_arg(),
            "--yes",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .unwrap();
    // Its note is on stderr: it says it waits for the build under way.
    let said = link_test_kit::wait::eventually(
        "the start's note that it waits",
        std::time::Duration::from_secs(20),
        || {
            let said = world.read("stderr");
            said.contains("waits for it").then_some(said)
        },
    );
    assert!(
        said.contains(&format!("under way, on {machine}, so this waits for it")),
        "{said}"
    );
    // That build ends, its image saved and its box removed.
    std::fs::create_dir_all(world.dir.join(format!("images.d/{hash}/app"))).unwrap();
    std::fs::write(world.dir.join("boxes"), "").unwrap();
    drop(held);
    assert!(child.wait().unwrap().success(), "{}", world.read("stderr"));
    let log = world.read("lnk.log");
    assert!(!log.contains(&format!("--name {machine}")), "{log}");
    assert!(!log.contains("box image create"), "{log}");
    assert!(
        log.contains(&format!(
            "box create --cloud aws --name app-1 --yes --image-hash {hash}"
        )),
        "{log}"
    );
}

/// A stage that reaches its own timeout fails the build, naming it; as
/// root, `timeout` under sudo ends it, since what root starts is root's.
#[test]
fn a_stage_that_reaches_its_timeout_fails_the_build() {
    if !box_identity() {
        return;
    }
    let world = World::new("stage-timeout");
    let slow = FILE.replace(
        "run = \"echo deps >> stages\"",
        "run = \"echo $$ > ../deps.pid; exec sleep 60\"\ntimeout = \"1s\"",
    );
    std::fs::write(&world.env, slow).unwrap();
    let started = std::time::Instant::now();
    let out = world.run(&[
        "env",
        "image",
        "create",
        "--cloud",
        "aws",
        "--spec",
        &world.env_arg(),
        "--yes",
    ]);
    assert!(!out.status.success());
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    assert!(
        text(&out.stderr).contains("failed at deps"),
        "{}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    let built = world
        .read("removed")
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let pid: i32 = world
        .read(&format!("boxes.d/{built}/deps.pid"))
        .trim()
        .parse()
        .unwrap();
    // SAFETY: signal 0 only asks whether it's there.
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "the stage still runs");
    let root = FILE.replace(
        "run = \"echo system >> stages\"",
        "run = \"exec sleep 60\"\ntimeout = \"1s\"",
    );
    std::fs::write(&world.env, root).unwrap();
    let out = world.run(&[
        "env",
        "image",
        "create",
        "--cloud",
        "aws",
        "--spec",
        &world.env_arg(),
        "--yes",
    ]);
    assert!(!out.status.success());
    let as_root = std::fs::read_dir(world.dir.join("boxes.d"))
        .unwrap()
        .map(|b| std::fs::read_to_string(b.unwrap().path().join("as-root")).unwrap_or_default())
        .collect::<String>();
    assert!(
        as_root.contains("timeout -s KILL 1s sh -c exec sleep 60"),
        "{as_root}"
    );
}

/// `lnk env image create --timeout` bounds the whole build: reaching it fails
/// the build, stops what runs, and removes the box made for it.
#[test]
fn a_build_that_reaches_its_timeout_fails_and_leaves_no_box() {
    if !box_identity() {
        return;
    }
    let world = World::new("build-timeout");
    let slow = FILE.replace(
        "run = \"echo deps >> stages\"",
        "run = \"echo $$ > ../deps.pid; exec sleep 60\"",
    );
    std::fs::write(&world.env, slow).unwrap();
    let started = std::time::Instant::now();
    let out = world.run(&[
        "env",
        "image",
        "create",
        "--cloud",
        "aws",
        "--spec",
        &world.env_arg(),
        "--timeout",
        "2s",
        "--yes",
    ]);
    assert!(!out.status.success());
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    assert!(
        text(&out.stderr).contains("building app's image reached its timeout, 2s"),
        "{}",
        text(&out.stderr)
    );
    let built = world.read("removed");
    assert!(built.starts_with("app-image-"), "the build box is removed");
    assert_eq!(world.read("boxes"), "");
    let pid: i32 = world
        .read(&format!("boxes.d/{}/deps.pid", built.trim()))
        .trim()
        .parse()
        .unwrap();
    // SAFETY: signal 0 only asks whether it's there.
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "the stage still runs");
    assert!(!world.read("lnk.log").contains("box image create"));
}

#[test]
fn an_environment_without_build_stages_needs_no_auxiliary_image_box() {
    let world = World::new("start-no-image");
    std::fs::write(
        &world.env,
        r#"schema = 1
name = "app"
version = 1
[[stage]]
name = "resources"
run = "echo started >> stages"
when = "start"
sandbox = "deps.toml"
inputs = ["requirements.txt"]
"#,
    )
    .unwrap();
    let out = world.run(&[
        "env",
        "create",
        "--cloud",
        "aws",
        "--spec",
        &world.env_arg(),
        "--yes",
    ]);
    assert!(
        out.status.success(),
        "{}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    let log = world.read("lnk.log");
    assert!(
        !log.contains("box image create")
            && !log.contains("--image-hash")
            && !log.contains("app-image-"),
        "{log}"
    );
    assert_eq!(world.read("boxes"), "app-1\n");
    assert_eq!(world.read("boxes.d/app-1/home/stages"), "started\n");
}

/// A box and an image are billed by their cloud, so each is made only
/// after a yes: with no terminal to ask and no `--yes`, `lnk env create`
/// and `lnk env image create` make nothing, naming what they'd make and `--yes`.
/// The other tests here pass `--yes`, and are made.
#[test]
fn nothing_billed_is_made_without_a_yes() {
    let world = World::new("billed");
    for args in [
        &["env", "create", "--cloud", "aws", "--name", "web-1"][..],
        &["env", "image", "create", "--cloud", "aws"][..],
    ] {
        let env = world.env_arg();
        let out = world.run(&[args, &["--spec", &env]].concat());
        assert!(!out.status.success(), "{args:?}");
        let said = text(&out.stderr);
        assert!(
            said.contains("Make the image of app")
                && said.contains("billed by that cloud")
                && said.contains("--yes"),
            "{said}"
        );
        let log = world.read("lnk.log");
        assert!(
            !log.contains("box create") && !log.contains("box image create"),
            "{log}"
        );
    }
    assert_eq!(world.read("boxes"), "");
}

/// What the stand-in `sudo` records of the file's root stage run here: the
/// stage under `/usr/bin/env`, keeping the caller's `PATH` past sudo's own.
fn root_stage(world: &World) -> String {
    format!(
        "/usr/bin/env PATH={} sh -c echo system >> stages\n",
        world.path()
    )
}
