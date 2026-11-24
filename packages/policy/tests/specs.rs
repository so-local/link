//! The Policy Spec's cases, `tests/specs/policy/*.toml`: each a policy as
//! a person writes it (`[spec]`), the `[access]` of the resources it's
//! checked on, if a case gives one, and what must come of it (`[expect]`):
//! its name, or the words of the error it gets when it's `invalid`; and
//! the roles (`[[role]]`, each a Policy Spec) every check's principal
//! holds, if a case gives any. Then
//! the table `allows` is held to: each `[[check]]` a principal (`as`, of
//! `account`, `acct-own` unless named, and `expired` or not), an action
//! and a resource, and the words of the answer (`said`); and each
//! `[[usage]]` within the policy's limits, or the one it `exceeds`. Every
//! policy that reads is printed and read back as itself. A new case is a
//! new file.

link_test_kit::program!(unit);

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use link_policy::{Access, Principal, Resource, Usage, allows};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    spec: toml::Table,
    #[serde(default)]
    access: Access,
    #[serde(default)]
    role: Vec<toml::Table>,
    expect: Expect,
    #[serde(default)]
    check: Vec<Check>,
    #[serde(default)]
    usage: Vec<Used>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    #[serde(default)]
    invalid: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    #[serde(rename = "as")]
    principal: String,
    #[serde(default = "own")]
    account: String,
    #[serde(default)]
    expired: bool,
    action: String,
    on: String,
    #[serde(default)]
    said: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Used {
    #[serde(default)]
    boxes: u32,
    #[serde(default)]
    cpus: u32,
    #[serde(default)]
    memory_gb: u32,
    #[serde(default)]
    disk_gb: u32,
    #[serde(default)]
    hours: u32,
    #[serde(default)]
    exceeds: Option<String>,
}

fn own() -> String {
    "acct-own".into()
}

fn cases() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/specs/policy");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    files
}

/// The case's policy read, then each check's answer, or the first error.
fn run(case: &Case, wrong: &mut Vec<String>, name: &str) -> anyhow::Result<()> {
    let text = toml::to_string(&case.spec)?;
    let policy = link_policy::parse(&text)?;
    let printed = link_spec::print_spec(&policy)?;
    if link_policy::parse(&printed)? != policy {
        wrong.push(format!("{name}: printed, it reads otherwise:\n{printed}"));
    }
    if case
        .expect
        .name
        .as_deref()
        .is_some_and(|n| n != policy.name)
    {
        wrong.push(format!("{name}: named {:?}", policy.name));
    }
    let roles = case
        .role
        .iter()
        .map(|role| link_policy::parse(&toml::to_string(role)?))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let now = SystemTime::now();
    for check in &case.check {
        let id: link_policy::PrincipalId = check.principal.parse().map_err(anyhow::Error::msg)?;
        let hour = Duration::from_secs(3600);
        let expires = match check.expired {
            true => Some(now - hour),
            false => Some(now + hour),
        };
        let account = check.account.parse().map_err(anyhow::Error::msg)?;
        let principal =
            Principal::new(id, account, expires, policy.clone()).with_roles(roles.clone());
        let action = check.action.parse().map_err(anyhow::Error::msg)?;
        let resource = Resource::new(
            check.on.parse().map_err(anyhow::Error::msg)?,
            case.access.clone(),
        )?;
        let said = allows(&principal, action, &resource, now).to_string();
        if check.said.is_empty() || !said.starts_with(&check.said) {
            wrong.push(format!(
                "{name}: {} {} {}: wants {:?}, said {said:?}",
                check.principal, check.action, check.on, check.said
            ));
        }
    }
    for used in &case.usage {
        let usage = Usage {
            boxes: used.boxes,
            cpus: used.cpus,
            memory_gb: used.memory_gb,
            disk_gb: used.disk_gb,
            hours: used.hours,
        };
        let exceeds = policy.admits(&usage).err().map(|e| e.limit);
        if exceeds != used.exceeds.as_deref() {
            wrong.push(format!(
                "{name}: {usage:?} exceeds {exceeds:?}, wants {:?}",
                used.exceeds
            ));
        }
    }
    Ok(())
}

#[test]
fn every_policy_spec_case_holds() {
    let files = cases();
    assert!(files.len() >= 15, "only {} cases", files.len());
    let mut wrong = Vec::new();
    let mut verbs = std::collections::BTreeSet::new();
    for path in files {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let case: Case = toml::from_str(&std::fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|e| panic!("{name}: not a case: {e}"));
        verbs.extend(case.check.iter().map(|c| c.action.clone()));
        let result = run(&case, &mut wrong, &name);
        match (&case.expect.invalid, result) {
            (Some(words), Ok(())) => wrong.push(format!("{name}: read, but wants {words:?}")),
            (Some(words), Err(err)) => {
                let said = format!("{err:#}");
                if !said.contains(words.as_str()) {
                    wrong.push(format!("{name}: wants {words:?}, said {said}"));
                }
            }
            (None, Err(err)) => wrong.push(format!("{name}: {err:#}")),
            (None, Ok(())) => {}
        }
    }
    for action in link_policy::Action::all() {
        if !verbs.contains(&action.to_string()) {
            wrong.push(format!("no case checks {action}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Every policy field is active here, including both grant members and
/// all limits. The expected policy is built independently of field flags.
fn complete_reader_policy() -> link_policy::Policy {
    link_policy::parse(
        r#"schema = 1
name = "reviewed"
version = 1
[[allow]]
actions = ["cloud.read"]
on = ["cloud:aws"]
[limits]
boxes = 2
cpus = 3
memory_gb = 4
disk_gb = 5
hours = 6
"#,
    )
    .unwrap()
}

#[test]
fn every_policy_field_reads_equally_and_its_flags_override_file_values() {
    let expected = complete_reader_policy();
    let binding = link_spec::Binding::of::<link_policy::Policy>();
    let words = link_spec::flags_of(&expected).unwrap();
    let (flags, rest) =
        link_spec::split(words.into_iter().map(Into::into), Some(&binding), None).unwrap();
    assert!(rest.is_empty());
    let from_flags = link_spec::read_text_with::<link_policy::Policy>("", &flags).unwrap();
    assert_eq!(from_flags, expected);
    let printed = link_spec::print_spec(&expected).unwrap();
    assert_eq!(link_policy::parse(&printed).unwrap(), expected);
    // Every leaf differs, including an invalid version repaired before the
    // registered semantic check. Losing any override changes or refuses it.
    let base = r#"schema = 1
name = "before"
version = 2
[[allow]]
actions = ["cloud.admin"]
on = ["cloud:gcp"]
[limits]
boxes = 1
cpus = 1
memory_gb = 1
disk_gb = 1
hours = 1
"#;
    assert!(link_policy::parse(base).is_err());
    assert_eq!(
        link_spec::read_text_with::<link_policy::Policy>(base, &flags).unwrap(),
        expected
    );
}

#[test]
fn policy_fields_refuse_wrong_types_and_missing_required_grant_members() {
    // As a person writes it, naming its schema: each change below is the
    // one thing wrong with it.
    let complete: toml::Table =
        toml::from_str(&link_spec::print_spec(&complete_reader_policy()).unwrap()).unwrap();
    for field in ["name", "version", "allow", "limits"] {
        let mut invalid = complete.clone();
        invalid.insert(field.into(), toml::Value::Boolean(false));
        assert!(
            link_policy::parse(&toml::to_string(&invalid).unwrap()).is_err(),
            "{field}"
        );
    }
    for field in ["actions", "on"] {
        let mut invalid = complete.clone();
        invalid["allow"].as_array_mut().unwrap()[0]
            .as_table_mut()
            .unwrap()
            .insert(field.into(), toml::Value::Boolean(false));
        assert!(
            link_policy::parse(&toml::to_string(&invalid).unwrap()).is_err(),
            "{field}"
        );
        let mut missing = complete.clone();
        missing["allow"].as_array_mut().unwrap()[0]
            .as_table_mut()
            .unwrap()
            .remove(field);
        assert!(
            link_policy::parse(&toml::to_string(&missing).unwrap()).is_err(),
            "{field}"
        );
    }
    for field in ["boxes", "cpus", "memory_gb", "disk_gb", "hours"] {
        let mut invalid = complete.clone();
        invalid["limits"]
            .as_table_mut()
            .unwrap()
            .insert(field.into(), toml::Value::Boolean(false));
        assert!(
            link_policy::parse(&toml::to_string(&invalid).unwrap()).is_err(),
            "{field}"
        );
    }
    for field in ["boxes", "cpus", "memory_gb", "disk_gb", "hours"] {
        for value in [-1, i64::from(u32::MAX) + 1] {
            let mut invalid = complete.clone();
            invalid["limits"]
                .as_table_mut()
                .unwrap()
                .insert(field.into(), value.into());
            assert!(
                link_policy::parse(&toml::to_string(&invalid).unwrap()).is_err(),
                "{field}={value}"
            );
        }
        let mut boundary = complete.clone();
        boundary["limits"]
            .as_table_mut()
            .unwrap()
            .insert(field.into(), 0.into());
        assert!(
            link_policy::parse(&toml::to_string(&boundary).unwrap()).is_ok(),
            "{field}=0"
        );
    }
    for (field, value) in [
        ("name", toml::Value::String(String::new())),
        ("name", toml::Value::String("x".repeat(65))),
        (
            "version",
            toml::Value::Integer(i64::from(link_policy::VERSION) + 1),
        ),
    ] {
        let mut invalid = complete.clone();
        invalid.insert(field.into(), value);
        assert!(
            link_policy::parse(&toml::to_string(&invalid).unwrap()).is_err(),
            "{field}"
        );
    }
    let mut too_many = complete.clone();
    let grant = complete["allow"].as_array().unwrap()[0].clone();
    too_many.insert(
        "allow".into(),
        toml::Value::Array(vec![grant; link_policy::MAX_GRANTS + 1]),
    );
    assert!(link_policy::parse(&toml::to_string(&too_many).unwrap()).is_err());
    for field in ["actions", "on"] {
        let mut invalid = complete.clone();
        invalid["allow"].as_array_mut().unwrap()[0]
            .as_table_mut()
            .unwrap()
            .insert(field.into(), toml::Value::Array(Vec::new()));
        assert!(
            link_policy::parse(&toml::to_string(&invalid).unwrap()).is_err(),
            "{field}"
        );
    }
    for field in ["name", "version"] {
        let mut missing = complete.clone();
        missing.remove(field);
        assert!(
            link_policy::parse(&toml::to_string(&missing).unwrap()).is_err(),
            "{field}"
        );
    }
}

#[test]
fn omitted_policy_fields_and_each_limit_have_independent_effects() {
    let complete = complete_reader_policy();
    let table: toml::Table = toml::from_str(&link_spec::print_spec(&complete).unwrap()).unwrap();
    let mut no_grants = table.clone();
    no_grants.remove("allow");
    let no_grants = link_policy::parse(&toml::to_string(&no_grants).unwrap()).unwrap();
    assert!(no_grants.allow.is_empty());
    let mut unlimited = table.clone();
    unlimited.remove("limits");
    let unlimited = link_policy::parse(&toml::to_string(&unlimited).unwrap()).unwrap();
    assert!(unlimited.limits.is_none());
    let max = Usage {
        boxes: u32::MAX,
        cpus: u32::MAX,
        memory_gb: u32::MAX,
        disk_gb: u32::MAX,
        hours: u32::MAX,
    };
    assert!(unlimited.admits(&max).is_ok());
    let exact = Usage {
        boxes: 2,
        cpus: 3,
        memory_gb: 4,
        disk_gb: 5,
        hours: 6,
    };
    assert!(complete.admits(&exact).is_ok());
    for field in ["boxes", "cpus", "memory_gb", "disk_gb", "hours"] {
        let mut missing = table.clone();
        missing["limits"].as_table_mut().unwrap().remove(field);
        let parsed = link_policy::parse(&toml::to_string(&missing).unwrap()).unwrap();
        let printed = toml::Value::try_from(&parsed).unwrap();
        assert!(printed["limits"].get(field).is_none(), "{field}");
        let mut above = exact;
        match field {
            "boxes" => above.boxes += 1,
            "cpus" => above.cpus += 1,
            "memory_gb" => above.memory_gb += 1,
            "disk_gb" => above.disk_gb += 1,
            "hours" => above.hours += 1,
            _ => unreachable!(),
        }
        assert_eq!(complete.admits(&above).unwrap_err().limit, field);
        assert!(parsed.admits(&above).is_ok(), "{field}");
    }
    let resource = Resource::new("cloud:aws".parse().unwrap(), Access::default()).unwrap();
    for (policy, prefix) in [
        (complete, "allowed by grant"),
        (no_grants, "refused: no grant"),
    ] {
        let principal = Principal::new(
            "service:scoutsimjo7n".parse().unwrap(),
            "acct-own".parse().unwrap(),
            None,
            policy,
        );
        let decision = allows(
            &principal,
            "cloud.read".parse().unwrap(),
            &resource,
            SystemTime::UNIX_EPOCH,
        );
        assert!(decision.to_string().starts_with(prefix), "{decision}");
        for (action, resource) in [("cloud.admin", "cloud:aws"), ("cloud.read", "cloud:gcp")] {
            let resource = Resource::new(resource.parse().unwrap(), Access::default()).unwrap();
            let refused = allows(
                &principal,
                action.parse().unwrap(),
                &resource,
                SystemTime::UNIX_EPOCH,
            );
            assert!(
                refused.to_string().starts_with("refused: no grant"),
                "{refused}"
            );
        }
    }
}
