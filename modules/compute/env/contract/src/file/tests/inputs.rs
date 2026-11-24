use super::*;

fn fields(words: &[&str]) -> link_spec::FieldFlags {
    link_spec::split(
        words.iter().map(Into::into),
        Some(&link_spec::Binding::of::<Env>()),
        None,
    )
    .unwrap()
    .0
}

#[test]
fn overrides_are_applied_before_the_effective_environment_is_read_and_checked() {
    let dir = TempDir::new("env-input-overrides");
    for text in [
        "schema = 1\nname='Bad Name'\nversion=1\n",
        "schema = 1\nname='app'\nversion='bad'\n",
    ] {
        std::fs::write(dir.join("env.toml"), text).unwrap();
        let read = read_with(
            &dir.join("env.toml"),
            &fields(&["--spec.name", "app", "--spec.version", "1"]),
        )
        .unwrap();
        assert_eq!(read.file.name, "app");
        assert_eq!(read.file.version, 1);
    }
    std::fs::write(
        dir.join("env.toml"),
        "schema = 1\nname='app'\nversion=1\n[[stage]]\nname='old'\nrun='true'\nsandbox='absent.toml'\n",
    )
    .unwrap();
    let read = read_with(
        &dir.join("env.toml"),
        &fields(&["--stage", "{name='new',run='true',user='root'}"]),
    )
    .unwrap();
    assert_eq!(read.file.stages()[0].members()[0].name, "new");
}

#[test]
fn inspection_refuses_the_same_semantically_invalid_values_as_consumption() {
    for (words, why) in [
        (vec!["--workdir", "../outside"], "workdir"),
        (
            vec!["--variables.API_TOKEN", "not-a-secret"],
            "named like a secret",
        ),
        (
            vec![
                "--stage",
                "{name='build',run='true',user='root',when='start'}",
            ],
            "root and at every start",
        ),
        (
            vec![
                "--service",
                "{name='web',run='true',user='root',ready={port=1,run='true'}}",
            ],
            "ready is a port or a run",
        ),
    ] {
        let mut flags = vec!["--spec.name", "app", "--spec.version", "1"];
        flags.extend(words);
        let error =
            link_spec::show::<Env>(&link_spec::Show::default(), &fields(&flags), None).unwrap_err();
        assert!(format!("{error:#}").contains(why), "{error:#}");
    }
}

#[test]
fn sidecar_flags_never_substitute_a_file_from_another_source_folder() {
    let cwd = std::env::current_dir().unwrap();
    let caller = TempDir::under(&cwd.join("target"), "env-caller");
    let bundle = TempDir::new("env-bundle");
    let caller_file = caller.join("sandbox.toml");
    let relative = caller_file.strip_prefix(&cwd).unwrap();
    std::fs::write(&caller_file, "# caller's sandbox\n").unwrap();
    let decoy = bundle.join(relative);
    std::fs::create_dir_all(decoy.parent().unwrap()).unwrap();
    std::fs::write(&decoy, "# different sandbox from the spec folder\n").unwrap();
    let spec = bundle.join("env.toml");
    std::fs::write(&spec, "schema = 1\nname='app'\nversion=1\n").unwrap();
    let record = format!(
        "{{name='build',run='true',sandbox={:?}}}",
        relative.display().to_string()
    );
    let result = read_with(&spec, &fields(&["--stage", &record]));
    let error = result.unwrap_err();
    assert!(
        format!("{error:#}").contains("outside the spec's folder"),
        "{error:#}"
    );
    let show = link_spec::Show {
        spec: Some(spec.clone()),
        ..link_spec::Show::default()
    };
    assert!(link_spec::show::<Env>(&show, &fields(&["--stage", &record]), None).is_err());
    assert_eq!(
        std::fs::read_to_string(&spec).unwrap(),
        "schema = 1\nname='app'\nversion=1\n"
    );
}
