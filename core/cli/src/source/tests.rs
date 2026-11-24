use super::*;

fn package(root: &Path, name: &str, folder: &str, bin: &str) -> Package {
    Package {
        id: name.into(),
        name: name.into(),
        manifest_path: root.join(folder).join("Cargo.toml"),
        metadata: None,
        targets: vec![Target {
            name: bin.into(),
            kind: vec!["bin".into()],
        }],
    }
}

/// `lnk` copied into `home`'s Cargo installation, `installation/bin`.
#[cfg(unix)]
fn installed_lnk(home: &Path) -> PathBuf {
    let config: Config = toml::from_str(include_str!("../../source.toml")).unwrap();
    let root = home.join("installation");
    let bin = root.join(&config.bin_directory);
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(root.join(&config.install_record), "").unwrap();
    link_test_kit::copy_program(&link_test_kit::programs().join("lnk"), &bin.join("lnk")).unwrap();
    bin.join("lnk")
}

/// A source build's modules go where `lnk` is: a target folder's
/// profile, a Cargo installation's root, or nowhere, said.
#[test]
fn modules_go_where_lnk_is() {
    let config: Config = toml::from_str(include_str!("../../source.toml")).unwrap();
    let home = link_test_kit::TempDir::new("source-where");
    let target = home.join("target");
    for profile in ["debug", "release"] {
        std::fs::create_dir_all(target.join(profile)).unwrap();
    }
    std::fs::write(target.join(&config.target_tag), "").unwrap();
    let target = target.canonicalize().unwrap();
    assert_eq!(
        Where::of(&config, &target.join("debug/lnk")).unwrap(),
        Where::Target {
            dir: target.clone(),
            profile: "dev".into(),
            bin: target.join("debug"),
        }
    );
    assert_eq!(
        Where::of(&config, &target.join("release/lnk")).unwrap(),
        Where::Target {
            dir: target.clone(),
            profile: "release".into(),
            bin: target.join("release"),
        }
    );
    let root = home.join("installation");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join(&config.install_record), "").unwrap();
    let root = root.canonicalize().unwrap();
    let installed = Where::of(&config, &root.join("bin/lnk")).unwrap();
    assert_eq!(installed, Where::Installed { root: root.clone() });
    assert_eq!(installed.bin(&config), root.join("bin"));
    // A bin folder Cargo didn't install into, or any other folder.
    for elsewhere in ["exec/bin", "elsewhere"] {
        std::fs::create_dir_all(home.join(elsewhere)).unwrap();
        let lnk = home.join(elsewhere).join("lnk");
        let err = Where::of(&config, &lnk).unwrap_err().to_string();
        assert!(err.contains(&lnk.display().to_string()), "{err}");
        assert!(err.contains("install.sh --source"), "{err}");
    }
}

#[test]
fn source_install_excludes_core_fixtures_dependencies_and_libraries() {
    let root = PathBuf::from("checkout");
    let config: Config = toml::from_str(include_str!("../../source.toml")).unwrap();
    let mut library = package(&root, "library", "packages/library", "lnk-library");
    library.targets[0].kind = vec!["lib".into()];
    let packages = vec![
        package(&root, "link-cli", "core/cli", "lnk"),
        package(&root, "agent", "modules/agent", "lnk-agent"),
        package(&root, "fixture", "tests/fixture", "lnk-fixture"),
        package(
            &root,
            "nested-fixture",
            "tools/tests/fixture",
            "lnk-nested-fixture",
        ),
        package(&root, "external", "external", "lnk-external"),
        package(&root, "server", "modules/server", "server"),
        library,
    ];
    let metadata = Metadata {
        workspace_members: packages
            .iter()
            .filter(|p| p.name != "external")
            .map(|p| p.id.clone())
            .collect(),
        workspace_root: root.clone(),
        target_directory: root.join("target"),
        packages,
    };
    let names: Vec<_> = metadata
        .programs(&config)
        .unwrap()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["agent"]);
    let unrelated = Metadata {
        packages: vec![],
        ..metadata
    };
    assert!(unrelated.programs(&config).is_err());
}

#[test]
fn source_flag_requires_a_checkout_and_rejects_release_choices() {
    use clap::Parser;
    assert!(cli::Cli::try_parse_from(["lnk", "up", "--source", "."]).is_ok());
    assert!(
        cli::Cli::try_parse_from(["lnk", "up", "--source", ".", "--source", "../extra"]).is_ok()
    );
    assert!(cli::Cli::try_parse_from(["lnk", "up", "--source"]).is_err());
    // A source-built lnk chooses from its own checkout; a release has none.
    let built_from_source = link_core::CHECKOUT.is_some();
    assert_eq!(
        cli::Cli::try_parse_from(["lnk", "up", "--module", "agent"]).is_ok(),
        built_from_source
    );
    assert_eq!(
        cli::Cli::try_parse_from(["lnk", "up", "--stale"]).is_ok(),
        built_from_source
    );
    assert!(
        cli::Cli::try_parse_from([
            "lnk",
            "up",
            "--source",
            ".",
            "--module",
            "agent",
            "--module",
            "scheduler",
            "--stale"
        ])
        .is_ok()
    );
    assert!(cli::Cli::try_parse_from(["lnk", "up", "agents", "--source", "."]).is_err());
    let parsed = cli::Cli::try_parse_from([
        "lnk",
        "up",
        "--source",
        ".",
        "--module",
        "agent,manual,model",
        "--module",
        "scheduler",
    ])
    .unwrap();
    let cli::Command::CoreUp(asked) = parsed.command else {
        panic!("expected up")
    };
    assert_eq!(
        modules_named(&asked.module),
        ["agent", "manual", "model", "scheduler"]
    );
}

/// A package's `[package.metadata.link.source]` selects the hosts it's
/// built from source on, by OS and architecture; one declaring none is
/// built on every host.
#[test]
fn source_plans_keep_each_package_to_the_hosts_it_names() {
    let config: Config = toml::from_str(include_str!("../../source.toml")).unwrap();
    let package = |name: &str, source: serde_json::Value| {
        serde_json::json!({"id": name, "name": name,
            "manifest_path": format!("checkout/{name}/Cargo.toml"),
            "metadata": {"link": {"source": source}},
            "targets": [{"name": format!("lnk-{name}"), "kind": ["bin"]}]})
    };
    let metadata: Metadata = serde_json::from_value(serde_json::json!({
        "workspace_root": "checkout",
        "workspace_members": ["mac", "mac-arm", "linux-x86", "anywhere"],
        "target_directory": "target",
        "packages": [
            package("mac", serde_json::json!({"os": ["macos"]})),
            package("mac-arm", serde_json::json!({"os": ["macos"], "arch": ["aarch64"]})),
            package("linux-x86", serde_json::json!({"os": ["linux"], "arch": ["x86_64"]})),
            package("anywhere", serde_json::json!({})),
        ]
    }))
    .unwrap();
    let names = |os, arch| -> std::collections::BTreeSet<String> {
        metadata
            .plan(&config, os, arch)
            .unwrap()
            .programs
            .iter()
            .map(|p| p.name.clone())
            .collect()
    };
    let set = |names: &[&str]| names.iter().map(|n| n.to_string()).collect();
    assert_eq!(
        names("macos", "aarch64"),
        set(&["mac", "mac-arm", "anywhere"])
    );
    assert_eq!(names("macos", "x86_64"), set(&["mac", "anywhere"]));
    assert_eq!(names("linux", "x86_64"), set(&["linux-x86", "anywhere"]));
    assert_eq!(names("linux", "aarch64"), set(&["anywhere"]));
    // Ordinary Cargo packages without any custom metadata remain eligible.
    let metadata: Metadata = serde_json::from_value(serde_json::json!({
        "workspace_root": "checkout", "workspace_members": ["addon"], "target_directory": "target",
        "packages": [{"id": "addon", "name": "addon", "manifest_path": "checkout/addon/Cargo.toml",
            "metadata": null, "targets": [{"name": "lnk-addon", "kind": ["bin"]}]}]
    }))
    .unwrap();
    assert_eq!(
        metadata
            .plan(&config, "macos", "aarch64")
            .unwrap()
            .programs
            .len(),
        1
    );
}

#[cfg(unix)]
#[test]
fn source_bootstrap_passes_paths_with_spaces_without_downloading_a_release() {
    let home = link_test_kit::TempDir::new("source-bootstrap");
    let bin = home.join("bin");
    let checkout = home.join("source checkout");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&checkout).unwrap();
    link_test_kit::write_program(&bin.join("cargo"), "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
    let path =
        std::env::join_paths([bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
    let output = link_test_kit::env::command("sh", &home)
        .arg(&script)
        .arg("--source")
        .arg(&checkout)
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let args: Vec<_> = stdout.lines().collect();
    assert_eq!(
        args,
        [
            "install",
            "--locked",
            "--force",
            "--path",
            checkout.join("core/cli").to_str().unwrap(),
            "--bin",
            "lnk",
            "--target-dir",
            checkout.join("target").to_str().unwrap()
        ]
    );
    let output = link_test_kit::env::command("sh", &home)
        .arg(&script)
        .args(["--source", ".", "extra"])
        .env("PATH", path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn source_install_runs_cargo_for_every_module_and_stops_after_a_failure() {
    let home = link_test_kit::TempDir::new("source-install");
    let bin = home.join("bin");
    let root = home.join("checkout");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let packages = [
        ("link-cli", "core/cli", "lnk"),
        ("agent", "modules/agent", "lnk-agent"),
        ("scheduler", "modules/scheduler", "lnk-scheduler-tool"),
        ("fixture", "tests/fixture", "lnk-fixture"),
    ];
    let metadata = serde_json::json!({
        "workspace_root": root,
        "target_directory": root.join("shared target"),
        "workspace_members": packages.iter().map(|p| p.0).collect::<Vec<_>>(),
        "packages": packages.iter().map(|(name, folder, bin)| serde_json::json!({
            "id": name, "name": name, "manifest_path": root.join(folder).join("Cargo.toml"),
            "targets": [{"name": bin, "kind": ["bin"]}]
        })).collect::<Vec<_>>()
    });
    let metadata_path = home.join("metadata.json");
    let log = home.join("cargo.log");
    std::fs::write(&metadata_path, metadata.to_string()).unwrap();
    link_test_kit::write_program(
        &bin.join("cargo"),
        r#"#!/bin/sh
case "$1" in
    metadata) cat "$SOURCE_METADATA" ;;
    install)
        printf '%s\n' "$*" >> "$SOURCE_LOG"
        if [ "${SOURCE_FAIL:-}" = yes ]; then exit 9; fi
        ;;
    *) exit 8 ;;
esac
"#,
    )
    .unwrap();
    let path =
        std::env::join_paths([bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
    let lnk = installed_lnk(&home);
    let run = |fail| {
        link_test_kit::env::command(&lnk, &home)
            .args(["up", "--source"])
            .arg(&root)
            .env("PATH", &path)
            .env("SOURCE_METADATA", &metadata_path)
            .env("SOURCE_LOG", &log)
            .env("SOURCE_FAIL", fail)
            .output()
            .unwrap()
    };
    let output = run("no");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Installing binaries has no need for the user's Link folder.
    assert!(
        !home.join("Link").exists(),
        "installing binaries must not initialize user or agent data"
    );
    let calls = std::fs::read_to_string(&log).unwrap();
    let calls: Vec<_> = calls.lines().collect();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].contains("modules/agent"), "{calls:?}");
    assert!(calls[0].contains("--bin lnk-agent"), "{calls:?}");
    assert!(calls[1].contains("modules/scheduler"), "{calls:?}");
    let root = format!(
        "--root {}",
        lnk.parent().unwrap().parent().unwrap().display()
    );
    assert!(calls.iter().all(|c| c.contains("install --locked")
        && c.contains("--force --path")
        && c.contains("--target-dir")
        && c.contains("shared target")
        && c.contains(&root)));
    std::fs::write(&log, "").unwrap();
    let output = run("yes");
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.contains("installing agent failed"), "{output}");
}

#[cfg(unix)]
#[test]
fn additional_module_workspaces_share_cache_and_are_validated_before_installation() {
    let home = link_test_kit::TempDir::new("source-workspaces");
    let first = home.join("first workspace");
    let extra = home.join("extra workspace");
    let bin = home.join("bin");
    for dir in [&first, &extra, &bin] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let fixture = |root: &Path, name: &str, binary: &str| {
        serde_json::json!({
            "workspace_root": root, "workspace_members": [name], "target_directory": root.join("target"),
            "packages": [{"id": name, "name": name, "manifest_path": root.join("package/Cargo.toml"),
                "targets": [{"name": binary, "kind": ["bin"]}]}]
        })
    };
    std::fs::write(
        first.join("metadata.json"),
        fixture(&first, "first", "lnk-first").to_string(),
    )
    .unwrap();
    let addon = fixture(&extra, "addon", "lnk-addon");
    std::fs::write(extra.join("metadata.json"), addon.to_string()).unwrap();
    let log = home.join("cargo.log");
    link_test_kit::write_program(
        &bin.join("cargo"),
        r#"#!/bin/sh
case "$1" in
    metadata) cat metadata.json ;;
    install) printf '%s|%s\n' "$PWD" "$*" >> "$SOURCE_LOG" ;;
    *) exit 8 ;;
esac
"#,
    )
    .unwrap();
    let path =
        std::env::join_paths([bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
    let lnk = installed_lnk(&home);
    let run = |roots: &[&Path]| {
        let mut command = link_test_kit::env::command(&lnk, &home);
        command.arg("up");
        for root in roots {
            command.arg("--source").arg(root);
        }
        command
            .env("PATH", &path)
            .env("SOURCE_LOG", &log)
            .output()
            .unwrap()
    };
    let output = run(&[&first, &extra]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = std::fs::read_to_string(&log).unwrap();
    let calls: Vec<_> = calls.lines().collect();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].starts_with(extra.to_str().unwrap()), "{calls:?}");
    assert!(calls[1].starts_with(first.to_str().unwrap()), "{calls:?}");
    let target = format!("--target-dir {}", first.join("target").display());
    assert!(calls.iter().all(|call| call.contains(&target)), "{calls:?}");
    // A module-only workspace can also be installed on its own.
    std::fs::write(&log, "").unwrap();
    assert!(run(&[&extra]).status.success());
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);
    // Conflicting executables and repeated roots must not change either install.
    std::fs::write(&log, "").unwrap();
    std::fs::write(
        extra.join("metadata.json"),
        fixture(&extra, "addon", "lnk-first").to_string(),
    )
    .unwrap();
    assert!(!run(&[&first, &extra]).status.success());
    assert!(std::fs::read_to_string(&log).unwrap().is_empty());
    assert!(!run(&[&first, &first]).status.success());
    assert!(std::fs::read_to_string(&log).unwrap().is_empty());
    // A later invalid workspace is discovered before the first package installs.
    std::fs::write(extra.join("metadata.json"), "{}").unwrap();
    assert!(!run(&[&first, &extra]).status.success());
    assert!(std::fs::read_to_string(&log).unwrap().is_empty());
}

#[cfg(unix)]
struct SourceFixture {
    home: link_test_kit::TempDir,
    checkout: PathBuf,
    bin: PathBuf,
    artifacts: PathBuf,
    log: PathBuf,
}

#[cfg(unix)]
impl SourceFixture {
    /// `lnk` in a Cargo installation, `installation/bin`.
    fn new() -> Self {
        let fixture = Self::at("installation/bin");
        std::fs::write(fixture.home.join("installation/.crates.toml"), "").unwrap();
        fixture
    }

    /// `lnk` in a Cargo target folder's dev profile, `target/debug`,
    /// where Cargo's build leaves each module beside it.
    fn in_target() -> Self {
        let fixture = Self::at("target/debug");
        std::fs::write(fixture.home.join("target/CACHEDIR.TAG"), "").unwrap();
        fixture
    }

    /// `lnk` in `folder` of a new home.
    fn at(folder: &str) -> Self {
        let home = link_test_kit::TempDir::new("source-selection");
        let checkout = home.join("checkout");
        let bin = home.join(folder);
        let artifacts = home.join("artifacts");
        let log = home.join("cargo.log");
        for dir in [&checkout, &bin, &artifacts] {
            std::fs::create_dir_all(dir).unwrap();
        }
        link_test_kit::copy_program(&link_test_kit::programs().join("lnk"), &bin.join("lnk"))
            .unwrap();
        let packages: Vec<_> = ["source-alpha", "source-beta", "source-missing"].iter().map(|name| {
            let binary = format!("lnk-{name}");
            link_test_kit::write_program(&artifacts.join(&binary), "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::write(artifacts.join(format!("{binary}.json")), serde_json::json!({
                "reason": "compiler-artifact", "target": {"name": binary, "kind": ["bin"]},
                "executable": artifacts.join(&binary)
            }).to_string()).unwrap();
            serde_json::json!({
                "id": name, "name": name, "manifest_path": checkout.join(name).join("Cargo.toml"),
                "targets": [{"name": binary, "kind": ["bin"]}, {"name": "helper", "kind": ["bin"]}]
            })
        }).collect();
        std::fs::write(checkout.join("metadata.json"), serde_json::json!({
            "workspace_root": checkout, "target_directory": checkout.join("target"),
            "workspace_members": ["source-alpha", "source-beta", "source-missing"], "packages": packages
        }).to_string()).unwrap();
        link_test_kit::write_program(
            &bin.join("cargo"),
            r#"#!/bin/sh
verb=$1
shift
if [ "$verb" = metadata ]; then cat metadata.json; exit; fi
printf '%s %s\n' "$verb" "$*" >> "$SOURCE_LOG"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --bin) binary=$2; shift ;;
    esac
    shift
done
case "$verb" in
    build)
        if [ "${SOURCE_FAIL:-}" = build ]; then exit 9; fi
        if [ -n "${SOURCE_BUILT:-}" ]; then cp "$SOURCE_ARTIFACTS/$binary" "$SOURCE_BUILT/$binary"; fi
        cat "$SOURCE_ARTIFACTS/$binary.json"
        ;;
    install)
        if [ "${SOURCE_FAIL:-}" = install ]; then exit 9; fi
        cp "$SOURCE_ARTIFACTS/$binary" "$SOURCE_BIN/$binary"
        ;;
    *) exit 8 ;;
esac
"#,
        )
        .unwrap();
        std::fs::write(&log, "").unwrap();
        Self {
            home,
            checkout,
            bin,
            artifacts,
            log,
        }
    }

    fn command(&self, flags: &[&str]) -> std::process::Command {
        let path =
            std::env::join_paths([self.bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")])
                .unwrap();
        let mut command = link_test_kit::env::command(self.bin.join("lnk"), &self.home);
        command
            .args(["up", "--source"])
            .arg(&self.checkout)
            .args(flags)
            .env("PATH", path)
            .env("SOURCE_LOG", &self.log)
            .env("SOURCE_ARTIFACTS", &self.artifacts)
            .env("SOURCE_BIN", &self.bin);
        command
    }

    fn run(&self, flags: &[&str]) -> String {
        let output = self.command(flags).output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read_to_string(&self.log).unwrap()
    }
}

#[cfg(unix)]
#[test]
fn selection_installs_only_named_module_binaries_and_rejects_unknown_names_first() {
    let fixture = SourceFixture::new();
    let calls = fixture.run(&[
        "--module",
        "source-beta,source-beta",
        "--module",
        "source-beta",
    ]);
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert!(calls.contains("--bin lnk-source-beta"), "{calls}");
    assert!(!calls.contains("helper"), "{calls}");
    std::fs::write(&fixture.log, "").unwrap();
    for name in ["unknown", "lnk-source-alpha"] {
        let output = fixture
            .command(&["--module", "source-alpha", "--module", name])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(std::fs::read_to_string(&fixture.log).unwrap().is_empty());
    }
    let path = fixture.checkout.join("metadata.json");
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    metadata["packages"][0]["metadata"] =
        serde_json::json!({"link": {"source": {"os": ["unsupported"]}}});
    std::fs::write(path, metadata.to_string()).unwrap();
    assert!(
        !fixture
            .command(&["--module", "source-alpha"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(std::fs::read_to_string(&fixture.log).unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn stale_refresh_compares_build_output_preserves_missing_modules_and_rechecks_inputs() {
    let fixture = SourceFixture::new();
    let alpha = "lnk-source-alpha";
    let beta = "lnk-source-beta";
    link_test_kit::copy_program(&fixture.artifacts.join(alpha), &fixture.bin.join(alpha)).unwrap();
    link_test_kit::write_program(&fixture.bin.join(beta), "#!/bin/sh\nexit 1\n").unwrap();
    let calls = fixture.run(&["--stale"]);
    assert_eq!(
        calls
            .lines()
            .filter(|line| line.starts_with("build "))
            .count(),
        2,
        "{calls}"
    );
    let installs: Vec<_> = calls
        .lines()
        .filter(|line| line.starts_with("install "))
        .collect();
    assert_eq!(installs.len(), 1, "{calls}");
    assert!(installs[0].contains(beta), "{calls}");
    assert!(
        installs[0].contains(&format!(
            "--root {}",
            fixture.bin.parent().unwrap().display()
        )),
        "{calls}"
    );
    assert!(!calls.contains("source-missing"), "{calls}");
    assert!(same_program(&fixture.artifacts.join(beta), &fixture.bin.join(beta)).unwrap());
    std::fs::write(&fixture.log, "").unwrap();
    assert!(
        !fixture
            .run(&["--stale"])
            .lines()
            .any(|line| line.starts_with("install "))
    );
    // Cargo may report a changed dependency's artifact with the same package version.
    link_test_kit::write_program(&fixture.artifacts.join(alpha), "#!/bin/sh\nexit 2\n").unwrap();
    std::fs::write(&fixture.log, "").unwrap();
    let calls = fixture.run(&["--module", "source-alpha", "--stale"]);
    assert_eq!(calls.lines().count(), 2, "{calls}");
    assert!(!calls.contains(beta), "{calls}");
    assert!(same_program(&fixture.artifacts.join(alpha), &fixture.bin.join(alpha)).unwrap());
}

#[cfg(unix)]
#[test]
fn stale_build_failure_leaves_installed_module_untouched_and_stops() {
    let fixture = SourceFixture::new();
    let binary = fixture.bin.join("lnk-source-alpha");
    link_test_kit::write_program(&binary, "#!/bin/sh\nexit 1\n").unwrap();
    let before = std::fs::read(&binary).unwrap();
    let output = fixture
        .command(&["--stale"])
        .env("SOURCE_FAIL", "build")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(std::fs::read(binary).unwrap(), before);
    let calls = std::fs::read_to_string(&fixture.log).unwrap();
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert!(calls.starts_with("build "), "{calls}");
}

/// A `lnk` in a Cargo target folder builds its modules into that folder
/// and profile, beside it, never installing; `--stale` rebuilds one there
/// in place.
#[cfg(unix)]
#[test]
fn a_target_folder_lnk_builds_its_modules_beside_it() {
    let fixture = SourceFixture::in_target();
    let built = |flags: &[&str]| {
        let output = fixture
            .command(flags)
            .env("SOURCE_BUILT", &fixture.bin)
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{text}");
        let calls = std::fs::read_to_string(&fixture.log).unwrap();
        std::fs::write(&fixture.log, "").unwrap();
        (calls, text)
    };
    let beta = "lnk-source-beta";
    let (calls, _) = built(&["--module", "source-beta"]);
    assert_eq!(calls.lines().count(), 1, "{calls}");
    let target = fixture.bin.parent().unwrap().canonicalize().unwrap();
    assert!(calls.starts_with("build "), "{calls}");
    assert!(calls.contains("--profile dev"), "{calls}");
    assert!(
        calls.contains(&format!("--target-dir {}", target.display())),
        "{calls}"
    );
    assert!(same_program(&fixture.artifacts.join(beta), &fixture.bin.join(beta)).unwrap());
    // An older module beside it is rebuilt in place; one not there is skipped.
    link_test_kit::write_program(&fixture.bin.join(beta), "#!/bin/sh\nexit 1\n").unwrap();
    let (calls, text) = built(&["--stale"]);
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert!(calls.contains(&format!("--bin {beta}")), "{calls}");
    assert!(same_program(&fixture.artifacts.join(beta), &fixture.bin.join(beta)).unwrap());
    assert!(text.contains("Installed source modules: 1"), "{text}");
    let (_, text) = built(&["--stale"]);
    assert!(text.contains("Unchanged: 1"), "{text}");
}

/// A `lnk` neither in a target folder nor a Cargo installation changes
/// nothing and says where it is.
#[cfg(unix)]
#[test]
fn a_lnk_elsewhere_installs_nothing_and_says_where_it_is() {
    let fixture = SourceFixture::at("elsewhere");
    let output = fixture.command(&[]).output().unwrap();
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("elsewhere"), "{text}");
    assert!(text.contains("install.sh --source"), "{text}");
    assert!(std::fs::read_to_string(&fixture.log).unwrap().is_empty());
}

#[test]
fn a_module_brings_what_it_requires_never_what_it_only_uses() {
    let declared = |text: &str| {
        required_here(&link_component::Source::parse(text).expect("a module declaration"))
    };
    let releases = r#"schema = 1
requires = [{ dependency = "sandbox" }]
[module]
id = "releases"
kind = "module"
owner = "modules/tooling/releases"
[dependencies.sandbox]
kind = "module"
id = "sandbox"
"#;
    assert_eq!(declared(releases), ["sandbox"]);
    let sandbox = include_str!("../../../../modules/security/sandbox/module.toml");
    assert!(
        declared(sandbox).is_empty(),
        "the modules the sandbox runs for aren't what it needs"
    );
}
