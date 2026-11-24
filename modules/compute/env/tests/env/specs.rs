//! The environment spec cases, `tests/specs/env/*.toml`: each an
//! environment file as a user writes it (`[spec]`), the files it names
//! where a case gives them (`[files]`, each path its text; any other it
//! names is written as an empty sandbox spec), and what must come of it
//! (`[expect]`): the stages saved in its image (`image`) and those run at
//! every start (`start`), in order, or the words of the error it gets
//! when it's `invalid`. Every file that reads is printed and read back as
//! itself. A new case is a new file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use link_env::file;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    spec: toml::Table,
    #[serde(default)]
    files: BTreeMap<String, String>,
    expect: Expect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    #[serde(default)]
    invalid: Option<String>,
    #[serde(default)]
    image: Vec<String>,
    #[serde(default)]
    start: Vec<String>,
}

fn cases() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/specs/env");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    files
}

/// The case's file read as every command reads it, in a folder of its
/// own with the files it names.
fn run(name: &str, case: &Case) -> anyhow::Result<file::Read> {
    let dir = link_test_kit::TempDir::new(&format!("env-spec-{name}"));
    let text = toml::to_string(&case.spec)?;
    let mut named: Vec<PathBuf> = case.files.keys().map(PathBuf::from).collect();
    if let Ok(parsed) = file::parse(&text) {
        named.extend(parsed.named_files());
    }
    for path in named {
        let at = dir.join(&path);
        if at.exists() || path.components().any(|c| c.as_os_str() == "..") {
            continue;
        }
        std::fs::create_dir_all(at.parent().unwrap())?;
        let given = case.files.get(&path.display().to_string());
        std::fs::write(&at, given.map_or("", String::as_str))?;
    }
    std::fs::write(dir.join("env.toml"), &text)?;
    file::read(&dir.join("env.toml"))
}

#[test]
fn every_env_spec_case_holds() {
    let files = cases();
    assert!(files.len() >= 10, "only {} cases", files.len());
    let mut wrong = Vec::new();
    for path in files {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let case: Case = toml::from_str(&std::fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|e| panic!("{name}: not a case: {e}"));
        let result = run(&name, &case);
        match (&case.expect.invalid, result) {
            (Some(words), Ok(_)) => wrong.push(format!("{name}: read, but wants {words:?}")),
            (Some(words), Err(err)) => {
                let said = format!("{err:#}");
                if !said.contains(words.as_str()) {
                    wrong.push(format!("{name}: wants {words:?}, said {said}"));
                }
            }
            (None, Err(err)) => wrong.push(format!("{name}: {err:#}")),
            (None, Ok(read)) => {
                let stages = |saved: bool| -> Vec<String> {
                    read.file
                        .stages()
                        .iter()
                        .filter(|s| s.saved() == saved)
                        .map(|s| s.name().to_string())
                        .collect()
                };
                if stages(true) != case.expect.image || stages(false) != case.expect.start {
                    wrong.push(format!(
                        "{name}: image {:?} and start {:?}, wants {:?} and {:?}",
                        stages(true),
                        stages(false),
                        case.expect.image,
                        case.expect.start
                    ));
                }
                let printed = link_spec::print_spec(&read.file).unwrap();
                match file::parse(&printed) {
                    Ok(again) if again == read.file => {}
                    _ => wrong.push(format!(
                        "{name}: printed, it reads back otherwise:\n{printed}"
                    )),
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
