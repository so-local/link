use super::*;

#[test]
fn preferences_and_bootstrap_refuse_links_into_data() {
    let home = home("settings-links");
    let hidden = home.join(".lnk");
    let link = home.join("Link");
    let bootstrap = Hidden::Settings.at(&hidden);
    let preferences = Home::Settings.at(&link);
    link_files::create_private_dir(bootstrap.parent().unwrap()).unwrap();
    link_files::create_private_dir(preferences.parent().unwrap()).unwrap();
    if !link_test_kit::gate::settings(&bootstrap) {
        return;
    }
    let data = home.join("untrusted.toml");
    link_files::write_private(&data, b"").unwrap();
    std::os::unix::fs::symlink(&data, &preferences).unwrap();
    assert!(Settings::load_at(&hidden, &link).is_err());
    std::fs::remove_file(&preferences).unwrap();
    std::os::unix::fs::symlink(&data, &bootstrap).unwrap();
    assert!(Settings::load_at(&hidden, &link).is_err());
}

#[test]
fn a_folder_can_be_created_after_startup_prepares_its_safe() {
    let fixture = link_test_kit::TempDir::new("prepared-home");
    let user = fixture.canonicalize().unwrap();
    let hidden = user.join(".lnk");
    let home = Folder::Home.default_in(&user);
    link_files::layout::prepare_at(&hidden, &home).unwrap();
    assert_eq!(make_new_in(&user, &hidden).unwrap(), home);
    assert!(Home::Marker.at(&home).is_file());
    assert!(make_new_in(&user, &hidden).is_err());
}

#[test]
fn preparing_a_safe_never_adopts_existing_user_data() {
    let fixture = link_test_kit::TempDir::new("existing-home");
    let user = fixture.canonicalize().unwrap();
    let hidden = user.join(".lnk");
    let home = Folder::Home.default_in(&user);
    std::fs::create_dir(&home).unwrap();
    link_files::layout::prepare_at(&hidden, &home).unwrap();
    std::fs::write(home.join("notes"), "mine").unwrap();
    assert!(make_new_in(&user, &hidden).is_err());
    assert!(!Home::Marker.at(&home).exists());
    assert_eq!(std::fs::read_to_string(home.join("notes")).unwrap(), "mine");
}

fn home(name: &str) -> link_test_kit::TempDir {
    link_test_kit::TempDir::new(name)
}

#[test]
fn a_fresh_machine_uses_link_in_the_home_folder_and_marks_it_once_made() {
    let home = home("fresh");
    assert_eq!(
        find_in(&home, &home.join(".lnk")).unwrap(),
        Found::Here(home.join("Link"))
    );
    assert_eq!(
        Settings::load_in(&home, &home.join(".lnk")).unwrap(),
        Settings::default()
    );
    std::fs::create_dir(home.join("Link")).unwrap();
    assert_eq!(
        find_in(&home, &home.join(".lnk")).unwrap(),
        Found::Here(home.join("Link"))
    );
    let id = marker_id(&home.join("Link")).unwrap();
    assert_eq!(
        Settings::load_in(&home, &home.join(".lnk")).unwrap().id,
        Some(id)
    );
}

#[test]
fn many_commands_finding_a_fresh_folder_at_once_agree_on_its_marker() {
    let home = home("at-once");
    std::fs::create_dir(home.join("Link")).unwrap();
    let found: Vec<Found> = std::thread::scope(|scope| {
        let finds: Vec<_> = (0..16)
            .map(|_| scope.spawn(|| find_in(&home, &home.join(".lnk")).unwrap()))
            .collect();
        finds.into_iter().map(|find| find.join().unwrap()).collect()
    });
    assert!(found.iter().all(|f| *f == Found::Here(home.join("Link"))));
    let id = marker_id(&home.join("Link")).unwrap();
    assert_eq!(
        Settings::load_in(&home, &home.join(".lnk")).unwrap().id,
        Some(id)
    );
    assert_eq!(
        find_in(&home, &home.join(".lnk")).unwrap(),
        Found::Here(home.join("Link"))
    );
    let names: Vec<_> = std::fs::read_dir(home.join("Link")).unwrap().collect();
    assert_eq!(names.len(), 1, "only the marker: {names:?}");
}

#[test]
fn a_moved_folder_is_lost_until_pointed_at_then_found_by_its_relative_place() {
    let home = home("moved");
    std::fs::create_dir(home.join("Link")).unwrap();
    find_in(&home, &home.join(".lnk")).unwrap();
    std::fs::create_dir(home.join("Documents")).unwrap();
    std::fs::rename(home.join("Link"), home.join("Documents/Link")).unwrap();
    assert_eq!(
        find_in(&home, &home.join(".lnk")).unwrap(),
        Found::Lost(home.join("Link"))
    );
    // A fresh folder made at the old place isn't the one this machine used.
    std::fs::create_dir(home.join("Link")).unwrap();
    assert_eq!(
        find_in(&home, &home.join(".lnk")).unwrap(),
        Found::Lost(home.join("Link"))
    );
    assert!(point_in(&home, &home.join(".lnk"), &home.join("Link")).is_err());
    std::fs::remove_dir(home.join("Link")).unwrap();
    point_in(&home, &home.join(".lnk"), &home.join("Documents/Link")).unwrap();
    let map = std::fs::read_to_string(home.join(".lnk/safe/map.toml")).unwrap();
    assert!(map.contains("link = \"Documents/Link\""), "{map}");
    assert!(!map.contains(home.to_str().unwrap()), "{map}");
    assert_eq!(
        find_in(&home, &home.join(".lnk")).unwrap(),
        Found::Here(home.join("Documents/Link"))
    );
}

#[test]
fn a_new_folder_replaces_a_lost_one_but_never_what_is_there() {
    let home = home("new");
    std::fs::create_dir(home.join("Link")).unwrap();
    find_in(&home, &home.join(".lnk")).unwrap();
    assert!(make_new_in(&home, &home.join(".lnk")).is_err());
    std::fs::rename(home.join("Link"), home.join("Gone")).unwrap();
    assert!(matches!(
        find_in(&home, &home.join(".lnk")).unwrap(),
        Found::Lost(_)
    ));
    make_new_in(&home, &home.join(".lnk")).unwrap();
    assert_eq!(
        find_in(&home, &home.join(".lnk")).unwrap(),
        Found::Here(home.join("Link"))
    );
}

/// A moved folder's text files name where they are now, whole names only,
/// keeping their permissions; a file that isn't text is left.
#[test]
fn a_moved_folders_files_name_its_new_place() {
    use std::os::unix::fs::PermissionsExt;
    let home = home("rewrite");
    let was = home.join("Link");
    let now = home.join("Documents/Link");
    let dir = Hidden::Agents
        .at(&home.join(".lnk"))
        .join("main/harnesses/otter/.otter");
    std::fs::create_dir_all(dir.join("sessions")).unwrap();
    let text = format!(
        "{{\"workspace\":\"{w}/Agents/main/home/workspace\",\"old\":\"{w}-old\"}}",
        w = was.display()
    );
    std::fs::write(dir.join("otter.json"), &text).unwrap();
    std::fs::set_permissions(
        dir.join("otter.json"),
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    std::fs::write(
        dir.join("sessions/one.jsonl"),
        format!("{}\n", was.display()),
    )
    .unwrap();
    let binary = [0xffu8, 0xfe, 0x00];
    std::fs::write(dir.join("state.bin"), binary).unwrap();
    // A database and its log read as text, the path in them: skipped by
    // the header, not by failing to decode.
    let db = format!("SQLite format 3\0{}", was.display());
    std::fs::write(dir.join("state.db"), &db).unwrap();
    std::fs::write(dir.join("state.db-wal"), was.display().to_string()).unwrap();
    assert!(is_database(&dir.join("state.db-wal")));
    assert!(!is_database(&dir.join("otter.json")));
    assert_eq!(rewrite_tree(&dir, &was, &now).unwrap(), 2);
    let rewritten = std::fs::read_to_string(dir.join("otter.json")).unwrap();
    assert!(
        rewritten.contains(&format!("\"{}/Agents/main", now.display())),
        "{rewritten}"
    );
    assert!(
        rewritten.contains(&format!("\"{}-old\"", was.display())),
        "{rewritten}"
    );
    let mode = std::fs::metadata(dir.join("otter.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o640);
    assert_eq!(std::fs::read(dir.join("state.bin")).unwrap(), binary);
    assert_eq!(std::fs::read_to_string(dir.join("state.db")).unwrap(), db);
    assert_eq!(rewrite_tree(&now, &was, &now).unwrap(), 0);
}

/// A home that came from another machine names where it is here: each
/// absolute path ending in its layout's tail, whatever was before it.
#[test]
fn an_arrived_homes_paths_name_where_it_is() {
    let tail = format!("/{}/main/harnesses/otter", Hidden::AgentData.relative());
    for (text, want) in [
        (
            r#"{"w":"/home/ada/.lnk/data/agents/main/harnesses/otter/.otter/w"}"#,
            r#"{"w":"/N/.otter/w"}"#,
        ),
        (
            "cwd=/Users/b/Documents/.lnk/data/agents/main/harnesses/otter\n",
            "cwd=/N\n",
        ),
        (
            "/x/state/agents/main/harnesses/otter-old",
            "/x/state/agents/main/harnesses/otter-old",
        ),
        (
            "notes Agents/main/Harnesses/otter",
            "notes Agents/main/Harnesses/otter",
        ),
    ] {
        assert_eq!(replace_arrived(text, &tail, "/N"), want, "{text}");
    }
}

#[test]
fn output_is_a_table_of_the_settings_kept_only_when_set() {
    use link_format::look::{Choice, Symbols};
    let s: Settings = toml::from_str("[output]\nsymbols = \"plain\"\ncolor = \"never\"\n").unwrap();
    assert_eq!(
        (s.output.symbols, s.output.color),
        (Symbols::Plain, Choice::Never)
    );
    assert!(
        !toml::to_string(&Settings::default())
            .unwrap()
            .contains("output")
    );
    assert!(toml::from_str::<Settings>("[output]\ncolour = \"never\"\n").is_err());
}

#[test]
fn preferences_follow_home_while_the_locator_stays_hidden() {
    use link_format::look::Choice;
    let temp = link_test_kit::TempDir::new("home-preferences");
    let user = temp.canonicalize().unwrap();
    let hidden = user.join(".lnk");
    let mut settings = Settings::default();
    settings.output.color = Choice::Never;
    settings.save_in(&user, &hidden).unwrap();
    find_in(&user, &hidden).unwrap();
    let bootstrap = std::fs::read_to_string(Hidden::Settings.at(&hidden)).unwrap();
    assert!(!bootstrap.contains("output"), "{bootstrap}");
    assert!(Home::Settings.at(&user.join("Link")).is_file());
    let destination = user.join("Documents/Link");
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::fs::rename(user.join("Link"), &destination).unwrap();
    point_in(&user, &hidden, &destination).unwrap();
    assert_eq!(
        Settings::load_in(&user, &hidden).unwrap().output.color,
        Choice::Never
    );
    assert!(!user.join("Link").exists());
}

#[test]
fn inspection_is_read_only_and_first_need_marks_a_rediscoverable_home() {
    let user = home("first-needed-state");
    let machine = user.join(".lnk");
    if !link_test_kit::gate::settings(&Hidden::Settings.at(&machine)) {
        return;
    }
    let dir = user.join("Link");
    assert_eq!(
        inspect_in(&user, &machine).unwrap(),
        Found::Here(dir.clone())
    );
    assert!(!dir.exists());
    assert!(!machine.exists());
    assert_eq!(need_in(&user, &machine).unwrap(), dir);
    let marker = marker_id(&dir).unwrap();
    assert_eq!(
        Settings::load_in(&user, &machine).unwrap().id,
        Some(marker.clone())
    );
    assert!(Home::Safe.at(&dir).is_dir());
    std::fs::remove_file(Hidden::Settings.at(&machine)).unwrap();
    assert_eq!(
        inspect_in(&user, &machine).unwrap(),
        Found::Here(dir.clone())
    );
    assert!(!Hidden::Settings.at(&machine).exists());
    assert_eq!(need_in(&user, &machine).unwrap(), dir);
    assert_eq!(Settings::load_in(&user, &machine).unwrap().id, Some(marker));
    std::fs::rename(&dir, user.join("Moved")).unwrap();
    assert!(need_in(&user, &machine).is_err());
    assert!(!dir.exists());
}
