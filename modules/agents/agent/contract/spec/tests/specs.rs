//! The agent spec cases, `tests/specs/agent/*.toml`: each the
//! agent's own spec as a user writes it (`[spec]`), the files of teams
//! that list it (`[team.<name>]`, each as its file), and what must come
//! of the union (`[expect]`): keys and their values (`values`), whose
//! each is (`from`: `yours`, `link`, or a
//! team's name), or the words of the error it gets when it's
//! `invalid`. Every union is printed and read back as itself. A new
//! case is a new file.

link_test_kit::program!(unit);

use std::collections::BTreeMap;
use std::path::PathBuf;

use link_agent_spec::{From, Known, Team, parse, parse_team, resolve};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    #[serde(default)]
    spec: Option<toml::Table>,
    #[serde(default)]
    team: BTreeMap<String, toml::Table>,
    expect: Expect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    #[serde(default)]
    invalid: Option<String>,
    #[serde(default)]
    values: BTreeMap<String, toml::Value>,
    #[serde(default)]
    from: BTreeMap<String, String>,
}

/// The agent every case is: `scout`, ID `a1b2c3d4e5f6`.
const NAME: &str = "scout";

fn cases() -> Vec<PathBuf> {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../../tests/specs/agent");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    files
}

/// What the case's files make: the union, or why there's none.
fn run(case: &Case) -> anyhow::Result<link_agent_spec::Resolved> {
    let own = match &case.spec {
        Some(spec) => Some(parse(&toml::to_string(spec)?)?),
        None => None,
    };
    let mut teams: Vec<Team> = Vec::new();
    for (name, file) in &case.team {
        let team = parse_team(&toml::to_string(file)?)?;
        anyhow::ensure!(team.name == *name, "team {name}'s file names {}", team.name);
        teams.push(team);
    }
    teams.retain(|t| t.has("a1b2c3d4e5f6"));
    let known = Known { name: NAME.into() };
    resolve(own.as_ref(), &teams, &known)
}

#[test]
fn every_agent_spec_case_holds() {
    let files = cases();
    assert!(files.len() >= 5, "only {} cases", files.len());
    let mut wrong = Vec::new();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let case: Case = toml::from_str(&std::fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let result = run(&case);
        match (&case.expect.invalid, result) {
            (Some(words), Err(err)) => {
                if !format!("{err:#}").contains(words.as_str()) {
                    wrong.push(format!("{name}: {err:#}, not {words:?}"));
                }
            }
            (Some(words), Ok(_)) => wrong.push(format!("{name}: read, not refused with {words:?}")),
            (None, Err(err)) => wrong.push(format!("{name}: {err:#}")),
            (None, Ok(resolved)) => {
                let union = toml::Value::try_from(&resolved.spec).unwrap();
                for (key, value) in &case.expect.values {
                    let got = key.split('.').try_fold(&union, |v, part| v.get(part));
                    if got != Some(value) {
                        wrong.push(format!("{name}: {key} is {got:?}, not {value}"));
                    }
                }
                for (key, whose) in &case.expect.from {
                    let want = match whose.as_str() {
                        "yours" => From::Yours,
                        "link" => From::Link,
                        team => From::Team(team.into()),
                    };
                    if resolved.from.get(key) != Some(&want) {
                        wrong.push(format!("{name}: {key} is {:?}'s", resolved.from.get(key)));
                    }
                }
                // Printed, it's read back as itself.
                let printed = resolved.print().unwrap();
                match parse(&printed) {
                    Ok(back) if back == resolved.spec => {}
                    Ok(_) => wrong.push(format!("{name}: printed, it reads back otherwise")),
                    Err(err) => wrong.push(format!("{name}: printed, {err:#}\n{printed}")),
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
