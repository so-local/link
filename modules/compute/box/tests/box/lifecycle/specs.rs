//! A box made from a spec: the sizes asked for, the spec it prints back,
//! and what a spec wants of a cloud.

use super::*;

const SIZES: &str = r#"{"sizes": [
    {"size": "s-2", "cpus": 2, "memory_mb": 4096, "arch": "x86_64", "price": 0.05},
    {"size": "m-4", "cpus": 4, "memory_mb": 16384, "arch": "x86_64", "price": 0.20},
    {"size": "c-4", "cpus": 4, "memory_mb": 8192, "arch": "x86_64"},
    {"size": "g-4", "cpus": 4, "memory_mb": 16384, "gpus": 1, "gpu_model": "nvidia-l4", "arch": "x86_64", "price": 0.80}
]}"#;

/// A box made from a spec: the best fit of what the adapter lists, its
/// disk, spot and tags handed on, named after the spec, and what it is
/// said before it's made.
#[test]
fn a_box_is_made_from_a_spec() {
    let Some(world) = World::new("spec", true) else {
        return;
    };
    std::fs::write(world.dir.join("sizes.json"), SIZES).unwrap();
    let spec = world.dir.join("web.toml");
    std::fs::write(
        &spec,
        "schema = 1\nname = \"web\"\nversion = 2\n[machine]\ncpus = 4\nmemory = { min = \"12 GB\" }\n\
         disk = { min = \"40 GB\" }\nspot = true\ntags = { team = \"research\" }\n",
    )
    .unwrap();
    let spec = spec.to_str().unwrap();
    let listed = world.lnk_box(&[
        "box", "size", "list", "--cloud", "acct", "--spec", spec, "--json",
    ]);
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed["pick"], "m-4");
    assert_eq!(listed["sizes"].as_array().unwrap().len(), 1);
    let made = world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--spec",
        spec,
        "--yes",
        "--lnk.login=false",
    ]);
    let stderr = String::from_utf8_lossy(&made.stderr);
    for asked in [
        "m-4: 4 CPUs, 16 GB, x86_64, spot",
        "40 GB balanced",
        "latest",
        "$0.20/hour",
    ] {
        assert!(stderr.contains(asked), "{asked}: {stderr}");
    }
    let stdout = String::from_utf8_lossy(&made.stdout);
    assert!(stdout.contains("Your box web-1 is up"), "{stdout}");
    let adapter = world.read("adapter.log");
    let launch = adapter.lines().find(|l| l.starts_with("launch ")).unwrap();
    let request: serde_json::Value = serde_json::from_str(&launch["launch ".len()..]).unwrap();
    let made = &request["spec"];
    assert_eq!(made["size"], "m-4");
    assert_eq!(made["disk_gb"], 40);
    assert_eq!(made["disk_type"], "balanced");
    assert_eq!(made["os"], "ubuntu-24.04");
    assert_eq!(made["spot"], true);
    assert_eq!(made["gpus"], 0);
    assert_eq!(made["tags"]["team"], "research");
    assert_eq!(made["tags"]["lnk-box"], "web-1");

    // A flag wins over the file: GPUs, so the GPU machine.
    let flags = world.lnk_box(&[
        "box",
        "size",
        "list",
        "--cloud",
        "acct",
        "--spec",
        spec,
        "--machine.gpus",
        "1",
        "--json",
    ]);
    let flags: serde_json::Value = serde_json::from_slice(&flags.stdout).unwrap();
    assert_eq!(flags["pick"], "g-4");
    // Nothing fits: the nearest, said.
    let none = world.lnk_box_fails(&[
        "box",
        "size",
        "list",
        "--cloud",
        "acct",
        "--machine.cpus",
        "4",
        "--machine.memory",
        "{max='6 GB'}",
    ]);
    let stderr = String::from_utf8_lossy(&none.stderr);
    assert!(
        stderr.contains("the smallest with 4 CPUs has 8 GB, over your max of 6 GB"),
        "{stderr}"
    );
    // A size the adapter doesn't list there (one it doesn't fully know)
    // is made as given, saying so.
    let unknown = world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--machine.fake.size",
        "x-9",
        "--name",
        "x",
        "--yes",
        "--lnk.login=false",
    ]);
    let stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(
        stderr.contains("lnk-fake doesn't list x-9 in moon-1"),
        "{stderr}"
    );
    let launch = world.read("adapter.log");
    let launch = launch.lines().rfind(|l| l.starts_with("launch ")).unwrap();
    assert!(launch.contains(r#""size":"x-9""#), "{launch}");
    // arm64 with GPUs: NVIDIA's images are x86_64.
    let arm = world.lnk_box_fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--machine.arch",
        "arm64",
        "--machine.gpus",
        "1",
        "--yes",
    ]);
    let stderr = String::from_utf8_lossy(&arm.stderr);
    assert!(stderr.contains("a box with GPUs is x86_64"), "{stderr}");
    // A box's name with a spec's flags is refused: they make a new box,
    // in an account; and `start` takes none.
    let again = world.lnk_box_fails(&["box", "create", "--on", "web-1", "--machine.cpus", "2"]);
    assert!(String::from_utf8_lossy(&again.stderr).contains("which is a box already"));
    let start = world.lnk_box_fails(&["box", "start", "web-1", "--machine.cpus", "2"]);
    assert!(
        String::from_utf8_lossy(&start.stderr).contains("takes no field flags"),
        "{}",
        String::from_utf8_lossy(&start.stderr)
    );

    // An lnk behind this one: never from a file, only as a flag.
    let old = world.dir.join("old.toml");
    std::fs::write(&old, "schema = 1\n[lnk]\nversion = \"0.11.0\"\n").unwrap();
    let refused = world.lnk_box_fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--spec",
        old.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("behind this computer's lnk"), "{stderr}");
    let made = world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--lnk.version",
        "0.11.0",
        "--name",
        "old",
        "--yes",
        "--lnk.login=false",
    ]);
    assert!(String::from_utf8_lossy(&made.stderr).contains("0.11.0"));
    assert!(
        world
            .read("ssh.log")
            .contains("LNK_VERSION='0.11.0' sh <<'LNK_CHECKED_INSTALL_SH'"),
        "{}",
        world.read("ssh.log")
    );
    // Its spec reads back: the older lnk is said, not printed.
    let printed = world.lnk_box(&["box", "spec", "show", "old"]);
    let text = String::from_utf8_lossy(&printed.stdout);
    let again = link_box::spec::BoxSpec::parse(&text).unwrap();
    assert_eq!(again.lnk().version, None, "{text}");
    let stderr = String::from_utf8_lossy(&printed.stderr);
    assert!(stderr.contains("give --lnk.version 0.11.0"), "{stderr}");

    // An adapter that lists no sizes: a size is taken as given, matching
    // refused.
    std::fs::remove_file(world.dir.join("sizes.json")).unwrap();
    let matching = world.lnk_box_fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--machine.cpus",
        "4",
        "--yes",
    ]);
    assert!(
        String::from_utf8_lossy(&matching.stderr).contains("doesn't list its machine types"),
        "{}",
        String::from_utf8_lossy(&matching.stderr)
    );
    world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--machine.fake.size",
        "x-9",
        "--name",
        "b2",
        "--yes",
        "--lnk.login=false",
    ]);
}

/// The adapter is asked for its sizes only when the box needs them, and
/// when that fails the adapter's own error is what's said.
#[test]
fn sizes_are_asked_only_when_needed() {
    let Some(world) = World::new("asked", true) else {
        return;
    };
    std::fs::write(world.dir.join("sizes-fail"), "").unwrap();
    // The default size: not asked, so nothing said of it.
    let made = world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--yes",
        "--lnk.login=false",
    ]);
    let stderr = String::from_utf8_lossy(&made.stderr);
    assert!(!stderr.contains("AccessDenied"), "{stderr}");
    assert!(!world.read("adapter.log").contains("sizes "));
    // Matching: the adapter's error, not a guess at why.
    let matched = world.lnk_box_fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--machine.cpus",
        "4",
        "--yes",
    ]);
    let stderr = String::from_utf8_lossy(&matched.stderr);
    assert!(
        stderr.contains("AccessDenied: not allowed to list types"),
        "{stderr}"
    );
    assert!(
        stderr.contains("couldn't list its machine types"),
        "{stderr}"
    );
    // A pinned size: made as given, the error a warning.
    let pinned = world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--machine.fake.size",
        "big",
        "--name",
        "p",
        "--yes",
        "--lnk.login=false",
    ]);
    let stderr = String::from_utf8_lossy(&pinned.stderr);
    assert!(stderr.contains("making big as given"), "{stderr}");
    // `lnk box sizes` is for listing: its failure is the error.
    let listed = world.lnk_box_fails(&["box", "size", "list", "--cloud", "acct"]);
    assert!(String::from_utf8_lossy(&listed.stderr).contains("AccessDenied"));
}

/// An adapter with no `sizes` still makes the default box, and a pinned
/// size: not asking it is no warning.
#[test]
fn an_adapter_without_sizes_makes_a_default_or_pinned_box() {
    let Some(world) = World::new("old", true) else {
        return;
    };
    std::fs::write(world.dir.join("no-sizes"), "").unwrap();
    world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--yes",
        "--lnk.login=false",
    ]);
    let made = world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--machine.fake.size",
        "big",
        "--machine.tags.team",
        "x",
        "--yes",
        "--lnk.login=false",
    ]);
    // Its missing `sizes` isn't a warning: such an adapter makes these.
    assert!(!String::from_utf8_lossy(&made.stderr).contains("sizes"));
}

/// `lnk box spec show <box>` prints the spec it was made from, kept here,
/// with its machine pinned as its cloud says it is, whatever this computer
/// last saw; `lnk box spec show` with flags, the spec they make. A spot box
/// its cloud stops is listed as such.
#[test]
fn a_box_prints_the_spec_it_was_made_from() {
    let Some(mut world) = World::new("print", true) else {
        return;
    };
    // An account on "aws", whose adapter is the fake, beside lnk-box.
    // Copied by `copy_program`, never open for writing here: a test on
    // another thread forking while `fs::copy` held it open would keep it
    // open in that child until it execs, and running it then fails with
    // "Text file busy".
    let copy = link_test_kit::copy_program;
    copy(&world.bin.join("lnk-fake"), &world.bin.join("lnk-aws")).unwrap();
    world.exe = world.bin.join("lnk-box");
    copy(Path::new(env!("CARGO_BIN_EXE_lnk-box")), &world.exe).unwrap();
    std::fs::write(world.dir.join("sizes.json"), SIZES).unwrap();
    let spec = world.dir.join("web.toml");
    std::fs::write(
        &spec,
        "schema = 1\nname = \"web\"\nversion = 2\n[lnk]\nlogin = false\n[machine]\ncpus = 4\n\
         memory = { min = \"12 GB\" }\nspot = true\n[machine.gcp]\nregion = \"europe-west2\"\n",
    )
    .unwrap();
    world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "awsacct",
        "--spec",
        spec.to_str().unwrap(),
        "--machine.disk.min",
        "40GB",
        "--yes",
    ]);
    // The cloud says otherwise than this computer saw: it's the cloud's.
    std::fs::write(
        world.dir.join("describe.json"),
        r#"{"boxes": [{"id": "m-1", "name": "web-1", "place": "moon-2", "state": "running",
            "size": "m-8", "spot": true, "disk_gb": 60, "disk_type": "balanced",
            "os": "ubuntu-22.04", "arch": "x86_64"}]}"#,
    )
    .unwrap();
    let printed = world.lnk_box(&["box", "spec", "show", "web-1"]);
    let printed = String::from_utf8(printed.stdout).unwrap();
    let again = link_box::spec::BoxSpec::parse(&printed).unwrap();
    assert_eq!(again.name.as_ref().unwrap().as_str(), "web");
    assert_eq!(again.version, Some(2));
    assert_eq!(again.lnk().login, Some(false));
    let aws = again.cloud("aws").expect("[machine.aws]");
    assert_eq!(aws.size.as_ref().unwrap().as_str(), "m-8");
    assert_eq!(aws.region.as_ref().unwrap().as_str(), "moon-2");
    assert_eq!(aws.cpus, None, "{printed}");
    assert_eq!(aws.disk_min.unwrap().whole_gb(), 60);
    assert_eq!(aws.os, Some(link_box::spec::Os::Ubuntu2204));
    // The flag made it 40 GB: the file says so, in [machine].
    assert_eq!(again.fields().disk_min.unwrap().whole_gb(), 40);
    // Elsewhere, the best fit of the same spec.
    assert_eq!(again.want("gcp").cpus.min, Some(4));
    assert_eq!(again.want("gcp").region.unwrap().as_str(), "europe-west2");
    // Showing the box leads to the spec it was made from: a Next its
    // declarations name, or a debug build refuses it.
    world.lnk_box(&["box", "show", "web-1"]);
    let json = world.lnk_box(&["box", "spec", "show", "web-1", "--json"]);
    let json: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(json["machine"]["aws"]["size"], "m-8");

    // From flags, no box: the spec they make, making nothing.
    let flags = world.lnk_box(&[
        "box",
        "spec",
        "show",
        "--machine.cpus",
        "{min=2,max=8}",
        "--machine.spot",
        "--machine.tags.team",
        "x",
    ]);
    assert_eq!(
        String::from_utf8_lossy(&flags.stdout),
        "kind = \"box\"\nschema = 1\n[machine]\nspot = true\n\n[machine.cpus]\nmax = 8\nmin = 2\n\n[machine.tags]\nteam = \"x\"\n"
    );
    let size = world.lnk_box_fails(&["box", "spec", "show", "--machine.size", "t3.small"]);
    assert!(String::from_utf8_lossy(&size.stderr).contains("names one cloud's"));

    // The spot box found stopped, its adapter unable to say why: maybe
    // its cloud, maybe another computer's `lnk box stop`.
    std::fs::write(world.dir.join("state.json"), r#"{"state": "stopped"}"#).unwrap();
    let listed = world.lnk_box(&["box", "list", "--here", "--refresh", "--json"]);
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed[0]["stopped_by_cloud"], true);
    let shown = world.lnk_box(&["box", "list", "--here"]);
    let shown = String::from_utf8_lossy(&shown.stdout);
    assert!(
        shown.contains("stopped elsewhere")
            && shown.contains("by its cloud (spot) or by another computer"),
        "{shown}"
    );
    // Its adapter says the cloud stopped it: listed so.
    std::fs::write(
        world.dir.join("state.json"),
        r#"{"state": "stopped", "stopped_by_cloud": true}"#,
    )
    .unwrap();
    world.lnk_box(&["box", "list", "--here", "--refresh"]);
    let shown = world.lnk_box(&["box", "list", "--here"]);
    let shown = String::from_utf8_lossy(&shown.stdout);
    assert!(shown.contains("stopped by its cloud"), "{shown}");
    let ssh = world.lnk_box_fails(&["box", "ssh", "web-1"]);
    let stderr = String::from_utf8_lossy(&ssh.stderr);
    assert!(
        stderr.contains("Its cloud took the spot machine back"),
        "{stderr}"
    );
    // Or that a person did: not the cloud's.
    std::fs::write(
        world.dir.join("state.json"),
        r#"{"state": "stopped", "stopped_by_cloud": false}"#,
    )
    .unwrap();
    let listed = world.lnk_box(&["box", "list", "--here", "--refresh", "--json"]);
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed[0]["stopped_by_cloud"], false);
}

/// A box in a cloud the box module has never heard of (`fake`) prints a
/// spec pinning its machine in that cloud's table, which reads back.
#[test]
fn any_clouds_box_prints_a_spec_that_reads_back() {
    let Some(world) = World::new("third", true) else {
        return;
    };
    world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "b1",
        "--machine.fake.size",
        "big",
        "--zone",
        "moon-1",
        "--yes",
        "--lnk.login=false",
    ]);
    let printed = world.lnk_box(&["box", "spec", "show", "b1"]);
    let printed = String::from_utf8(printed.stdout).unwrap();
    assert!(printed.contains("[machine.fake]"), "{printed}");
    let again = link_box::spec::BoxSpec::parse(&printed).unwrap();
    let fake = again.cloud("fake").expect("[machine.fake]");
    assert_eq!(fake.size.as_ref().unwrap().as_str(), "big");
    assert_eq!(fake.region.as_ref().unwrap().as_str(), "moon-1");
    assert!(again.fields() == Default::default(), "{printed}");
}

/// What a spec asks of a box in one cloud, as a job's order carries it:
/// its table for that cloud laid over, every default filled in.
#[test]
fn a_spec_says_what_it_wants_of_a_cloud() {
    let Some(world) = World::new("want", true) else {
        return;
    };
    let file = world.dir.join("job-box.toml");
    std::fs::write(
        &file,
        "schema = 1\n[machine]\ncpus = 4\ntags = { team = \"research\" }\n\n[machine.aws]\ncpus = 8\n",
    )
    .unwrap();
    let file = file.display().to_string();
    let wanted = |kind: &str| -> link_cloud_contract::want::Want {
        let out = world.lnk_box(&["box", "check", "--plan", kind, "--spec", &file, "--json"]);
        let mut checked: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(checked["kind"], "plan", "{checked}");
        serde_json::from_value(checked["want"].take()).unwrap()
    };
    let aws = wanted("aws");
    assert_eq!(aws.cpus, link_cloud_contract::want::Range::exactly(8));
    assert_eq!(aws.tags.len(), 1);
    assert!(aws.matching);
    let other = wanted("gcp");
    assert_eq!(other.cpus, link_cloud_contract::want::Range::exactly(4));
    assert_eq!(other.disk, link_cloud_contract::want::Size::gb(20));
    let out = world.run(&["box", "check", "--plan", "Not A Kind", "--spec", &file]);
    assert!(!out.status.success());
}
