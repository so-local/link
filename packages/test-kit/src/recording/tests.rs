use super::*;

#[test]
fn selected_commands_keep_their_arguments_input_and_exit() {
    if let Some(root) = std::env::var_os("LNK_TEST_RECORDING_PROBE") {
        let root = PathBuf::from(root);
        let output = crate::env::command(root.join("observe-command"), &root)
            .arg("two words")
            .stdin(std::fs::File::open(root.join("input")).unwrap())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"two words\noriginal input\n");
        assert_eq!(output.stderr, b"diagnostic\n");
        let ignored = crate::env::command(root.join("unselected"), &root)
            .output()
            .unwrap();
        assert!(ignored.status.success());
        return;
    }
    let dir = crate::TempDir::new("recorded-command");
    let program = dir.join("observe-command");
    crate::write_program(
        &program,
        "#!/bin/sh\nprintf '%s\\n' \"$1\"\ncat\nprintf 'diagnostic\\n' >&2\nexit 7\n",
    )
    .unwrap();
    crate::write_program(&dir.join("unselected"), "#!/bin/sh\nexit 0\n").unwrap();
    let observer = dir.join("observer");
    crate::write_program(
        &observer,
        "#!/bin/sh\nlog=$1 name=$2; shift 2\nprintf '%s\\n' \"$name\" >>\"$log\"\nexec \"$@\"\n",
    )
    .unwrap();
    let log = dir.join("observed");
    let manifest = dir.join("recorder.toml");
    std::fs::write(
        &manifest,
        toml::to_string(&Recorder {
            program: observer,
            args: vec![log.display().to_string()],
            selected: Programs {
                names: vec![],
                prefixes: vec!["observe-".into()],
            },
        })
        .unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("input"), "original input\n").unwrap();
    let child = crate::env::command(std::env::current_exe().unwrap(), &dir)
        .args([
            "--exact",
            "recording::tests::selected_commands_keep_their_arguments_input_and_exit",
            "--show-output",
        ])
        .env(ENV, &manifest)
        .env("LNK_TEST_RECORDING_PROBE", dir.path())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(std::fs::read_to_string(log).unwrap(), "observe-command\n");
}

#[test]
fn a_recording_manifest_refuses_unknown_fields() {
    let manifest = "program='/observer'\nargs=[]\nunknown=true\n[selected]\nnames=[]\n";
    assert!(toml::from_str::<Recorder>(manifest).is_err());
}

#[cfg(unix)]
#[test]
fn delegated_programs_keep_selection_arguments_streams_and_signals() {
    use std::os::unix::process::ExitStatusExt;
    const PROBE: &str = "LNK_TEST_RECORDING_PROGRAM_PROBE";
    if let Some(root) = std::env::var_os(PROBE) {
        let root = PathBuf::from(root);
        let original = root.join("with 'quotes; $(false)").join("observe-command");
        let folder = root.join("shims");
        let delegated = program(&original, &folder).unwrap();
        if std::env::var_os(ENV).is_some() {
            assert_ne!(delegated, original);
            // A second path in the same owned folder cannot overwrite the
            // first while a product still holds it.
            let second = program(&original, &folder).unwrap();
            assert_ne!(second, delegated);
        } else {
            assert_eq!(delegated, original);
            assert!(!folder.exists());
        }
        let ignored = root.join("unselected");
        assert_eq!(program(&ignored, &root.join("unused")).unwrap(), ignored);
        assert!(!root.join("unused").exists());
        assert!(
            crate::env::command(&ignored, &root)
                .status()
                .unwrap()
                .success()
        );
        // Emulate a product's own command builder, which has no recording
        // interception. The executable path carries the observation seam.
        let mut command = link_secretive::process::ours(&delegated);
        crate::env::clean(&mut command, &root);
        let args = [
            "two words",
            "'quoted'",
            "$(false); `false`",
            "",
            "line\nbreak",
        ];
        let output = command
            .args(args)
            .stdin(std::fs::File::open(root.join("input")).unwrap())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(
            output.stdout,
            format!("{}\noriginal input\n", args.join("\n")).as_bytes()
        );
        assert_eq!(output.stderr, b"diagnostic\n");
        let mut command = link_secretive::process::ours(&delegated);
        crate::env::clean(&mut command, &root);
        let killed = command.arg("signal").output().unwrap();
        assert_eq!(killed.status.signal(), Some(libc::SIGTERM));
        return;
    }
    let dir = crate::TempDir::new("delegated-recording");
    for selection in [None, Some(false), Some(true)] {
        let owned = crate::TempDir::under(&dir, "selection");
        let quoted = owned.join("with 'quotes; $(false)");
        std::fs::create_dir(&quoted).unwrap();
        crate::write_program(&owned.join("unselected"), "#!/bin/sh\nexit 0\n").unwrap();
        let original = quoted.join("observe-command");
        crate::write_program(
            &original,
            "#!/bin/sh\nif [ \"$1\" = signal ]; then kill -TERM \"$$\"; exit 99; fi\nprintf '%s\\n' \"$@\"\ncat\nprintf 'diagnostic\\n' >&2\nexit 7\n",
        )
        .unwrap();
        let observer = quoted.join("observer");
        crate::write_program(
            &observer,
            "#!/bin/sh\nlog=$1 marker=$2 name=$3; shift 3\nprintf '%s\\n' \"$marker\" \"$name\" \"$1\" >>\"$log\"\nexec \"$@\"\n",
        )
        .unwrap();
        let log = owned.join("observed");
        let marker = "prefix 'quoted' $(false); `false`";
        let manifest = owned.join("recorder.toml");
        std::fs::write(
            &manifest,
            toml::to_string(&Recorder {
                program: observer,
                args: vec![log.display().to_string(), marker.into()],
                selected: match selection {
                    Some(true) => Programs {
                        names: vec![],
                        prefixes: vec!["observe-".into()],
                    },
                    _ => Programs {
                        names: vec!["observe-command".into()],
                        prefixes: vec![],
                    },
                },
            })
            .unwrap(),
        )
        .unwrap();
        std::fs::write(owned.join("input"), "original input\n").unwrap();
        let mut child = crate::env::command(std::env::current_exe().unwrap(), &owned);
        child
            .args([
                "--exact",
                "recording::tests::delegated_programs_keep_selection_arguments_streams_and_signals",
                "--show-output",
            ])
            .env_remove(ENV)
            .env(PROBE, owned.path());
        if selection.is_some() {
            child.env(ENV, &manifest);
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if selection.is_some() {
            assert_eq!(
                std::fs::read_to_string(log).unwrap(),
                format!("{marker}\nobserve-command\n{}\n", original.display()).repeat(2)
            );
        } else {
            assert!(!log.exists());
        }
    }
}
