//! An environment prepared for a program that works in its sandbox.
use std::io::Write;
use std::process::Stdio;

use link_env_fixture::*;

/// An environment carried to a box (`lnk env export`, then `lnk env
/// import` there) for a program that works in its sandbox: imported, its
/// start's stages run, its service up and its workdir made, and the file
/// and workdir there said.
#[test]
fn an_environment_carried_to_a_box_is_prepared_there() {
    if !box_identity() {
        return;
    }
    let world = World::new("carried-box");
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
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = world.run(&["env", "export", "--spec", &world.env_arg()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let packed = out.stdout;
    let home = world.dir.join("boxes.d/app-1/home");
    let envs = home.join(".lnk/safe/envs/envs/app");
    // As the box's own lnk runs it.
    let mut child = world
        .command(&["env", "import"])
        .current_dir(&home)
        .env("HOME", &home)
        .env("LNK_EXE", world.bin.join("boxlnk"))
        .env("LNK_TEST_MACHINE_KIND", world.dir.join("box/kind.toml"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&packed).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    // What its stages said, then what's prepared, as JSON.
    let said = text(&out.stdout);
    let prepared: serde_json::Value = serde_json::from_str(said.lines().last().unwrap()).unwrap();
    assert_eq!(prepared["env"], envs.join("env.toml").display().to_string());
    assert_eq!(prepared["workdir"], home.join("app").display().to_string());
    // The start's stage ran again; the build's stages, done, didn't.
    assert_eq!(
        world.read("boxes.d/app-1/home/stages"),
        "system\ndeps\npull\npull\n"
    );
    // Nothing made for an agent: its sandbox is its own.
    assert!(!envs.join("agents").exists());
}

#[test]
fn preparing_an_agents_environment_refuses_root_and_private_workdirs_before_setup() {
    let world = World::new("prepare-refused");
    let file = world.dir.join("project/env.toml");
    std::fs::write(
        &file,
        FILE.replace("workdir = \"app\"", "workdir = \"new-app\""),
    )
    .unwrap();
    let out = world.run(&["env", "prepare", "--spec", &world.env_arg()]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("root build stages need a box"));
    assert!(!world.dir.join("home/new-app").exists());
    for workdir in [".ssh/payload", ".config/payload", "Link/Agents/payload"] {
        std::fs::write(
            &file,
            FILE.replace("workdir = \"app\"", &format!("workdir = {workdir:?}")),
        )
        .unwrap();
        let out = world.run(&["env", "prepare", "--spec", &world.env_arg()]);
        assert!(!out.status.success(), "{workdir}");
        assert!(!world.dir.join("home").join(workdir).exists());
    }
    assert_eq!(world.read("home/stages"), "");
}
