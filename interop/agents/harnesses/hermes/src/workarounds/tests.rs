use super::*;

pub(crate) fn complete_payload(root: &Path) {
    let venv = Path::new(&CONFIG.installs)
        .join("0000000000000000")
        .join(&CONFIG.environments)
        .join("selected");
    std::fs::create_dir_all(root.join(&venv)).unwrap();
    std::fs::create_dir_all(root.join(&CONFIG.store)).unwrap();
    complete_manager(root);
    std::fs::write(
        root.join(&venv).join(&CONFIG.venv_config),
        "version = 3.11\n",
    )
    .unwrap();
    let launcher = root.join(&CONFIG.code).join(&CONFIG.launcher);
    std::fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    link_test_kit::write_program(&launcher, "#!/bin/sh\nexit 0\n").unwrap();
    link_test_kit::write_program(
        &root.join(&CONFIG.code).join(&CONFIG.acp),
        "#!/bin/sh\nexit 0\n",
    )
    .unwrap();
    let selection = Manifest {
        repo: CONFIG.code.clone(),
        venv: venv.to_str().unwrap().into(),
        store: CONFIG.store.clone(),
    };
    std::fs::write(
        root.join(&CONFIG.manifest),
        serde_json::to_vec(&selection).unwrap(),
    )
    .unwrap();
}

fn complete_manager(root: &Path) {
    let runtime = root.join(&CONFIG.manager.directory);
    std::fs::create_dir_all(runtime.join("lib/python3.11/site-packages")).unwrap();
    std::fs::write(
        root.join(&CONFIG.store).join("python"),
        "fixture interpreter",
    )
    .unwrap();
    std::fs::write(
        runtime.join("pm-runtime.json"),
        r#"{"python":"../tools/python","sitePackages":"lib/python3.11/site-packages"}"#,
    )
    .unwrap();
}

#[test]
fn staged_pm_generation_keeps_its_contained_interpreter_and_dependencies() {
    let (fixture, root, _environment) = staged();
    let state = install_state(&root).unwrap();
    let runtime = state.join("pm-runtime");
    let generation = runtime.join("generations/selected");
    let python = root.join("tools/python");
    std::fs::write(&python, "fixture interpreter").unwrap();
    std::fs::create_dir_all(generation.join("bin")).unwrap();
    std::fs::create_dir_all(generation.join("lib/python3.11/site-packages")).unwrap();
    std::os::unix::fs::symlink(&python, generation.join("bin/python")).unwrap();
    std::fs::write(generation.join("pm-runtime.json"), r#"{"inputs":"locked"}"#).unwrap();
    std::fs::write(
        runtime.join("selected.json"),
        r#"{"inputs":"locked","generation":"generations/selected"}"#,
    )
    .unwrap();
    manager::stage(&root, &state).unwrap();
    manager::check(&root.canonicalize().unwrap()).unwrap();
    assert!(!generation.exists());
    assert!(
        root.join("pm-runtime/lib/python3.11/site-packages")
            .is_dir()
    );
    let published = fixture.join("published");
    std::fs::rename(root, &published).unwrap();
    manager::check(&published.canonicalize().unwrap()).unwrap();
    std::fs::write(
        published.join("pm-runtime/pm-runtime.json"),
        r#"{"python":"../../outside","sitePackages":"lib/python3.11/site-packages"}"#,
    )
    .unwrap();
    assert!(manager::check(&published.canonicalize().unwrap()).is_err());
}

#[test]
fn runtime_refuses_a_launcher_without_a_complete_contained_seal() {
    let root = link_test_kit::TempDir::new("hermes-required-seal");
    complete_payload(&root);
    assert!(payload(&root).is_ok());
    let marker = root.join(&CONFIG.manifest);
    let saved = std::fs::read(&marker).unwrap();
    std::fs::remove_file(&marker).unwrap();
    assert!(payload(&root).is_err());
    std::fs::write(
        &marker,
        r#"{"repo":"hermes-agent","store":"tools","venv":"../data"}"#,
    )
    .unwrap();
    assert!(payload(&root).is_err());
    std::fs::write(&marker, saved).unwrap();
    let acp = root.join(&CONFIG.code).join(&CONFIG.acp);
    let entrypoint = std::fs::read(&acp).unwrap();
    std::fs::remove_file(&acp).unwrap();
    assert!(payload(&root).is_err());
    link_test_kit::write_program(&acp, entrypoint).unwrap();
    assert!(payload(&root).is_ok());
    let selection: Manifest = serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
    std::fs::remove_file(root.join(&selection.venv).join(&CONFIG.venv_config)).unwrap();
    assert!(payload(&root).is_err());
}

fn staged() -> (
    link_test_kit::TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let fixture = link_test_kit::TempDir::new("hermes-payload");
    let root = fixture.join("staged");
    let code = root.join(&CONFIG.code);
    std::fs::create_dir_all(&code).unwrap();
    let code = code.canonicalize().unwrap();
    let key = link_digest::sha256(code.to_str().unwrap().as_bytes());
    let state = root
        .join(&CONFIG.installs)
        .join(&key[..CONFIG.install_key_characters]);
    let environment = state.join(&CONFIG.environments).join("selected");
    std::fs::create_dir_all(&environment).unwrap();
    std::fs::create_dir_all(root.join(&CONFIG.store)).unwrap();
    std::fs::write(environment.join(&CONFIG.venv_config), "version = 3.11\n").unwrap();
    let facts = serde_json::json!({"packages": {"venv": {"environment": environment}}, "otherUpstreamFacts": true});
    std::fs::write(
        state.join(&CONFIG.facts),
        serde_json::to_vec(&facts).unwrap(),
    )
    .unwrap();
    (fixture, root, environment)
}

#[test]
fn sealed_selection_is_relative_and_survives_payload_relocation() {
    let (fixture, root, environment) = staged();
    let selected = manifest(&root).unwrap();
    assert_eq!(selected.repo, CONFIG.code);
    assert_eq!(selected.store, CONFIG.store);
    assert_eq!(root.join(&selected.venv), environment);
    let published = fixture.join("published");
    std::fs::rename(&root, &published).unwrap();
    assert!(
        published
            .join(&selected.venv)
            .join(&CONFIG.venv_config)
            .is_file()
    );
    assert!(published.join(&selected.repo).is_dir());
    assert!(published.join(&selected.store).is_dir());
    assert!(
        !serde_json::to_string(&selected)
            .unwrap()
            .contains(root.to_str().unwrap())
    );
}

#[test]
fn a_selection_outside_its_own_staged_generation_is_refused() {
    let (fixture, root, environment) = staged();
    let outside = fixture.join("unrelated");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join(&CONFIG.venv_config), "version = 3.11\n").unwrap();
    std::fs::rename(&environment, environment.with_extension("held")).unwrap();
    std::os::unix::fs::symlink(&outside, &environment).unwrap();
    assert!(manifest(&root).is_err());
    assert!(!root.join(&CONFIG.manifest).exists());
}

#[test]
fn missing_or_repeated_selection_fields_cannot_publish_a_payload() {
    let (_fixture, root, environment) = staged();
    let facts = environment
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(&CONFIG.facts);
    std::fs::write(
        &facts,
        r#"{"packages":{"venv":{"environment":"/a","environment":"/b"}}}"#,
    )
    .unwrap();
    assert!(manifest(&root).is_err());
    std::fs::write(&facts, r#"{"packages":{"venv":{}}}"#).unwrap();
    assert!(seal(&root).is_err());
    assert!(!root.join(&CONFIG.manifest).exists());
}

#[test]
fn source_guards_refuse_changed_or_ambiguous_vendor_code() {
    let replacements = [DEPENDENCY_HOME, STORE, ACTIVATION, SELECTION];
    let source = replacements
        .iter()
        .map(|(before, _)| *before)
        .collect::<Vec<_>>()
        .join("\n");
    let hash = link_digest::sha256(source.as_bytes());
    let output = patched(source.as_bytes(), &hash, &replacements).unwrap();
    for (_, after) in replacements {
        assert!(output.contains(after));
    }
    assert!(
        patched(
            source.as_bytes(),
            &CONFIG.sources.environments.sha256,
            &replacements
        )
        .is_err()
    );
    let doubled = format!("{source}\n{}", DEPENDENCY_HOME.0);
    assert!(
        patched(
            doubled.as_bytes(),
            &link_digest::sha256(doubled.as_bytes()),
            &replacements
        )
        .is_err()
    );
    let missing = source.replace(ACTIVATION.0, "");
    assert!(
        patched(
            missing.as_bytes(),
            &link_digest::sha256(missing.as_bytes()),
            &replacements
        )
        .is_err()
    );
    let paths = patched(
        WRITABLE_STORE.0.as_bytes(),
        &link_digest::sha256(WRITABLE_STORE.0.as_bytes()),
        &[WRITABLE_STORE],
    )
    .unwrap();
    assert!(!paths.contains("get_default_hermes_root"));
}

#[test]
fn runtime_invocations_refuse_ambient_executable_store_overrides() {
    let home = link_test_kit::TempDir::new("hermes-runtime-environment");
    let mut command = link_test_kit::env::command("hermes", &home);
    command.env("HERMES_HOME", "/data/hermes");
    command.env("HERMES_RUNTIME_DIR", "/data/planted-tools");
    command.env("HERMES_INSTALL_ROOT", "/data/planted-stamp");
    runtime(&mut command);
    let value = |name| {
        command
            .get_envs()
            .find(|(key, _)| *key == name)
            .and_then(|(_, value)| value)
    };
    assert_eq!(
        value("HERMES_HOME"),
        Some(std::ffi::OsStr::new("/data/hermes"))
    );
    assert_eq!(
        value("HERMES_DISABLE_LAZY_INSTALLS"),
        Some(std::ffi::OsStr::new("1"))
    );
    assert_eq!(value("HERMES_RUNTIME_DIR"), None);
    assert_eq!(value("HERMES_INSTALL_ROOT"), None);
}

#[test]
fn shipped_source_guards_match_the_pinned_release_and_bounded_layout() {
    CONFIG.validate().unwrap();
    let mut settings = Config::shipped();
    settings.commit.clear();
    assert!(settings.validate().is_err());
    settings.commit = crate::COMMIT.into();
    settings.max_document_bytes = u64::MAX;
    assert!(settings.validate().is_err());
}
