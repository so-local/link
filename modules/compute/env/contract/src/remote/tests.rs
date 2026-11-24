use super::*;

fn read(text: &str) -> Read {
    let text = format!("schema = 1\n{text}");
    Read {
        file: file::parse(&text).unwrap(),
        text,
        path: "/nowhere/app.toml".into(),
        dir: "/nowhere".into(),
    }
}

const OWN: &str = r#"name = "app"
version = 1

[[stage]]
name = "deps"
run = "brew bundle"
sandbox = "deps.toml"

[[stage]]
name = "builder-only"
run = "make"
user = "builder"

[[service]]
name = "web"
run = "python3 -m http.server 8000"
sandbox = "web.toml"
ready = { port = 8000 }
"#;

/// What a report says: its headline and why, as plain words.
fn words(report: link_format::Report) -> String {
    match &report.why {
        Some(why) => format!("{} {}", report.headline, why),
        None => report.headline.to_string(),
    }
}

#[test]
fn an_ordinary_guest_runs_steps_as_its_own_user_alone() {
    let own = read(OWN);
    check_login(&own.file, GuestLogin::Ordinary("builder")).unwrap();
    check_login(&own.file, GuestLogin::Root).unwrap();
    let said = words(check_login(&own.file, GuestLogin::Ordinary("ann")).unwrap_err());
    assert!(said.contains("builder-only runs as builder"), "{said}");
    let root_stage = read(&format!(
        "{OWN}\n[[stage]]\nname = \"system\"\nrun = \"softwareupdate -i -a\"\nuser = \"root\"\n"
    ));
    let said = words(check_login(&root_stage.file, GuestLogin::Ordinary("builder")).unwrap_err());
    assert!(said.contains("system runs as root"), "{said}");
    assert!(said.contains("no root"), "{said}");
    check_login(&root_stage.file, GuestLogin::Root).unwrap();
}

#[test]
fn a_root_service_is_refused_before_the_guest_is_touched() {
    let root_service = read(&format!(
        "{}[[service]]\nname = \"db\"\nrun = \"postgres\"\nuser = \"root\"\nready = {{ port = 5432 }}\n",
        OWN
    ));
    // No box named here exists: reaching it would fail differently.
    let said = words(
        prepare_private(
            "no-such-box",
            &root_service,
            GuestLogin::Ordinary("builder"),
            true,
            true,
            false,
        )
        .unwrap_err(),
    );
    assert!(said.contains("db runs as root"), "{said}");
    assert!(said.contains("nothing was copied or run"), "{said}");
}

#[test]
fn an_account_that_is_one_machine_is_on_and_another_is_cloud() {
    let accounts = [("mac".to_string(), true), ("aws".to_string(), false)];
    assert_eq!(slot_of(&accounts, "mac"), "--on");
    assert_eq!(slot_of(&accounts, "aws"), "--cloud");
    assert_eq!(slot_of(&accounts, "unknown"), "--cloud");
}
