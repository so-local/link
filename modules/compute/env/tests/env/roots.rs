//! Where a root run or a root service is allowed.
use link_env_fixture::*;

/// On a computer nothing runs as root: a request sent to `lnk env here`
/// straight with a run as root (a start of a file with a root service, a
/// job with a root stage, a root service's reset) is refused before
/// anything runs, the sandboxed stage at every start included; on a box
/// the start runs.
#[test]
fn a_root_run_is_refused_on_a_computer_before_anything_runs() {
    if !box_identity() {
        return;
    }
    use link_env::here::{self, Action, Request};
    use link_env_contract::values::Given;
    let world = World::new("root-here");
    // Its service is up already, as a box's background service would be.
    std::fs::write(world.dir.join("db-up"), "").unwrap();
    let file = FILE.replace(
        "sandbox = \"db.toml\"\nready = { run = \"test -f ../../db-up\" }",
        "user = \"root\"\nready = { run = \"test -f ../../db-up\" }\nreset = \"touch ../../reset-ran\"",
    );
    let envs = world.dir.join("home/.lnk/safe/envs/envs/app");
    std::fs::create_dir_all(&envs).unwrap();
    std::fs::write(envs.join("env.toml"), file).unwrap();
    for spec in [
        "deps.toml",
        "clone.toml",
        "db.toml",
        "run.toml",
        "requirements.txt",
    ] {
        std::fs::copy(world.dir.join("project").join(spec), envs.join(spec)).unwrap();
    }
    let send = |action: Action, file: Option<&str>, kind: Option<PathBuf>| {
        let mut command = world.command(&["env", "here"]);
        command.env("LNK_EXE", world.bin.join("boxlnk"));
        if let Some(kind) = kind {
            command.env("LNK_TEST_MACHINE_KIND", kind);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (seal_to, _rest) = link_core::announced(child.stdout.take().unwrap()).unwrap();
        let request = Request {
            workdir: None,
            name: "app".into(),
            file: file.map(PathBuf::from),
            action,
            given: link_secretive::Sealed::seal_json(
                &Given::default(),
                &seal_to,
                here::about("app"),
            )
            .unwrap(),
            mark: "lnk-env-test".into(),
            direct: false,
        };
        let mut input = child.stdin.take().unwrap();
        let mut line = serde_json::to_vec(&request).unwrap();
        line.push(b'\n');
        input.write_all(&line).unwrap();
        let mut said = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut said)
            .unwrap();
        drop(input);
        (child.wait().unwrap(), said)
    };
    let job = Action::Run {
        stages: vec![
            link_env::stage::Run {
                upstream: None,
                protected: None,
                name: "command".into(),
                run: "touch ../job-ran".into(),
                sandbox: None,
                user: Some(link_env::file::User::root()),
                timeout: None,
            }
            .into(),
        ],
        variables: Default::default(),
        secrets: Default::default(),
        out: None,
        profile: None,
    };
    let reset = Action::Reset {
        service: "db".into(),
    };
    // The job with no environment, so its own stage is its first run as
    // root (with this file's, its root service is).
    for (action, file, refused) in [
        (Action::Start, Some("env.toml"), "service db runs as root"),
        (job, None, "the run's stage command runs as root"),
        (reset, Some("env.toml"), "service db's reset runs as root"),
    ] {
        let (status, said) = send(action, file, None);
        assert!(!status.success(), "{said}");
        assert!(
            said.contains(&format!("{refused}, which happens on a box alone")),
            "{said}"
        );
        assert_eq!(world.read("home/stages"), "");
        assert_eq!(world.read("as-root"), "");
        assert!(!world.dir.join("job-ran").exists() && !world.dir.join("reset-ran").exists());
    }
    // Its start stage runs in the workdir the image's clone made.
    std::fs::create_dir_all(world.dir.join("home/app")).unwrap();
    let (status, said) = send(
        Action::Start,
        Some("env.toml"),
        Some(world.dir.join("box/kind.toml")),
    );
    assert!(status.success(), "{said}");
    assert_eq!(world.read("home/stages"), "pull\n");
}

/// A root service's reset runs as root: on a box made for its
/// environment, which runs it as root already, and refused on another.
#[test]
fn a_root_services_reset_runs_only_on_a_box_made_for_it() {
    if !box_identity() {
        return;
    }
    let world = World::new("reset");
    std::fs::write(world.dir.join("services-up"), "").unwrap();
    let file = FILE.replace(
        "sandbox = \"db.toml\"\nready = { run = \"test -f ../../db-up\" }",
        "user = \"root\"\nready = { run = \"test -f ../../db-up\" }\nreset = \"touch ../../reset-ran\"",
    );
    std::fs::write(&world.env, file).unwrap();
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
    // The box knows its environment: no file given.
    let out = world.run(&["env", "reset", "db", "--on", "app-1"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(world.dir.join("boxes.d/app-1/reset-ran").exists());
    // A box that's there, made otherwise: which environment is its isn't
    // known here, and given, its root service is refused.
    std::fs::create_dir_all(world.dir.join("boxes.d/web-1/home")).unwrap();
    let boxes = world.read("boxes") + "web-1\n";
    std::fs::write(world.dir.join("boxes"), boxes).unwrap();
    let out = world.run(&["env", "reset", "db", "--on", "web-1"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("--spec <file>"),
        "{}",
        text(&out.stderr)
    );
    let out = world.run(&[
        "env",
        "reset",
        "db",
        "--on",
        "web-1",
        "--spec",
        &world.env_arg(),
    ]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("isn't a box made for app"),
        "{}",
        text(&out.stderr)
    );
    assert!(!world.dir.join("boxes.d/web-1/reset-ran").exists());
    assert!(!world.read("lnk.log").contains("box copy web-1"));
}
