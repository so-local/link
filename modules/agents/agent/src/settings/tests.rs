use super::*;
use ports::coprime;
use providers::{key_for_provider_in, key_in};
mod addresses;
mod handing;
mod keys;
mod places;

/// A provider a module adds, declared for this test process as a fixture
/// under the name only a module provides (`link`), the module's answer
/// never asked.
pub(crate) fn example_models() -> &'static link_model_contract::providers::Declared {
    use link_model_contract::providers::{Declared, Providers, declare, declared};
    declare(Providers {
        provider: std::collections::BTreeMap::from([(
            "link".into(),
            Declared {
                title: "Example Models".into(),
                base_url: "https://models.gw.example/v1".into(),
                speaks: "openai".into(),
                key_prefix: "ex_key_".into(),
            },
        )]),
    });
    declared("link").expect("the example provider is declared")
}

/// A blank agent `name`, its ID reserved here as `lnk agent create`
/// reserves one: every agent saved has an ID.
pub(crate) fn blank(name: &str) -> Settings {
    let mut agent = Settings::new(name);
    agent.id = Some(ids::workload(&ids::reserve().unwrap()).unwrap());
    agent
}

/// Run each settings scenario in a dedicated process with a kit-owned home.
/// No test changes the process home or removes another scenario's settings.
pub(crate) fn own_config() -> bool {
    const FIXTURE: &str = "LNK_TEST_AGENT_SETTINGS_FIXTURE";
    if std::env::var_os(FIXTURE).is_some() {
        return true;
    }
    let home = link_test_kit::TempDir::short_out_of_tmp("agent-settings");
    if !link_test_kit::gate::settings(&home.join("Safe/Agents/main/agent.toml")) {
        return false;
    }
    let thread = std::thread::current();
    let test = thread.name().expect("libtest names its scenario thread");
    let log = home.join("fixture.log");
    let mut command = link_test_kit::process::rerun(test, &home, &log).unwrap();
    command.env(FIXTURE, "1");
    let process = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
    let status = link_test_kit::eventually(
        "the isolated agent settings scenario",
        std::time::Duration::from_secs(30),
        || process.try_status().unwrap(),
    );
    assert!(
        status.success(),
        "settings fixture failed: {status}\n{}",
        link_test_kit::process::rerun_said(&log)
    );
    false
}

/// A new agent or team can't take a name the layout keeps in its owner's
/// folders for itself, where it would collide with Link's own files.
#[test]
fn a_new_agent_or_team_refuses_the_names_its_folders_keep() {
    for (owner, name) in [
        ("agents", "journal"),
        ("agents", "context"),
        ("agents", "memory"),
    ] {
        assert!(check_new_name(owner, name).is_err(), "{owner} {name}");
    }
    check_new_name("agents", "main").unwrap();
    check_new_name("teams", "journal").unwrap();
    assert!(check_new_name("agents", "here").is_err());
}

#[test]
fn model_naming_preserves_the_builtin_fallback_and_unknown_provider_absence() {
    let spec = link_agent_spec::Resolved {
        spec: Default::default(),
        from: Default::default(),
    };
    let mut model = Model {
        provider: "anthropic".into(),
        ..Default::default()
    };
    assert_eq!(Settings::model_name(&model, &spec), model.named());
    model.provider = "unknown-provider".into();
    assert_eq!(Settings::model_name(&model, &spec), None);
    model.name = Some("an-explicit-id".into());
    assert_eq!(
        Settings::model_name(&model, &spec).as_deref(),
        Some("an-explicit-id")
    );
}

/// The spec an agent runs reads its settings file's behavior and parses
/// it once.
#[test]
fn its_spec_parses_its_own_once() {
    if !own_config() {
        return;
    }
    let mut agent = blank("main");
    agent.save().unwrap();
    let parses = || configuration::PARSES.with(std::cell::Cell::get);
    let before = parses();
    agent.spec().unwrap();
    assert_eq!(parses() - before, 1);
}
