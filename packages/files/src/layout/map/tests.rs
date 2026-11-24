use super::*;

struct Machine {
    _dir: link_test_kit::TempDir,
    user: PathBuf,
    hidden: PathBuf,
}

fn machine() -> Machine {
    let dir = link_test_kit::TempDir::new("map");
    let user = dir.join("home");
    let hidden = user.join(".lnk");
    std::fs::create_dir_all(&hidden).unwrap();
    Machine {
        user,
        hidden,
        _dir: dir,
    }
}

fn roots(at: &Machine) -> Roots {
    Roots {
        user_home: at.user.clone(),
        hidden: at.hidden.clone(),
    }
}

#[test]
fn every_location_is_pinned_once_and_kept_when_a_default_changes() {
    let at = machine();
    pin(&at.user, &at.hidden).unwrap();
    let text = std::fs::read_to_string(Hidden::Map.at(&at.hidden)).unwrap();
    assert!(text.starts_with("schema = 1\nlink = \"Link\"\n"), "{text}");
    assert!(text.contains("\"link.files\" = \"Files\""), "{text}");
    assert!(
        text.contains("\"lnk.agent-data\" = \"data/agents\""),
        "{text}"
    );
    assert!(!text.contains("lnk.map"), "{text}");
    // A machine pinned under an older default keeps it.
    let older = text.replace(
        "\"link.files\" = \"Files\"",
        "\"link.files\" = \"Personal\"",
    );
    crate::write_private(&Hidden::Map.at(&at.hidden), older.as_bytes()).unwrap();
    pin(&at.user, &at.hidden).unwrap();
    let map = load(&at.user, &at.hidden).unwrap();
    assert_eq!(
        map.path(Entry::Home(Home::Files), &roots(&at)),
        at.user.join("Link/Personal")
    );
    assert_eq!(map.relative(Entry::Hidden(Hidden::Agents)), "safe/agents");
}

#[test]
fn set_moves_a_location_its_files_and_what_is_inside_it() {
    let at = machine();
    pin(&at.user, &at.hidden).unwrap();
    let link = at.user.join("Link");
    std::fs::create_dir_all(link.join("Safe/Agents/old")).unwrap();
    let was = set(
        &at.user,
        &at.hidden,
        Entry::Home(Home::Safe),
        &at.user.join("Kept"),
    )
    .unwrap();
    assert_eq!(was, link.join("Safe"));
    assert!(at.user.join("Kept/Agents/old").is_dir());
    assert!(!link.join("Safe").exists());
    let map = load(&at.user, &at.hidden).unwrap();
    assert_eq!(
        map.path(Entry::Home(Home::AgentSettings), &roots(&at)),
        at.user.join("Kept/Agents")
    );
    // The Link folder moves with its locations, which stay relative to it.
    std::fs::create_dir_all(link.join("Logs")).unwrap();
    set(
        &at.user,
        &at.hidden,
        Entry::Link,
        &at.user.join("Documents/Link"),
    )
    .unwrap();
    let map = load(&at.user, &at.hidden).unwrap();
    assert_eq!(map.link.as_deref(), Some("Documents/Link"));
    assert_eq!(
        map.path(Entry::Home(Home::Logs), &roots(&at)),
        at.user.join("Documents/Link/Logs")
    );
    assert_eq!(
        crate::layout::link_in(&at.user, &at.hidden).unwrap(),
        at.user.join("Documents/Link")
    );
}

#[test]
fn set_points_at_files_already_moved_and_refuses_two_copies() {
    let at = machine();
    std::fs::create_dir_all(at.user.join("Link/Files")).unwrap();
    std::fs::create_dir_all(at.user.join("Elsewhere")).unwrap();
    let to = at.user.join("Elsewhere");
    let refused = set(&at.user, &at.hidden, Entry::Home(Home::Files), &to).unwrap_err();
    assert!(refused.to_string().contains("both exist"), "{refused:#}");
    std::fs::remove_dir(at.user.join("Link/Files")).unwrap();
    set(&at.user, &at.hidden, Entry::Home(Home::Files), &to).unwrap();
    let map = load(&at.user, &at.hidden).unwrap();
    assert_eq!(map.path(Entry::Home(Home::Files), &roots(&at)), to);
}

#[test]
fn reading_refuses_control_outside_a_safe_and_data_inside_one() {
    let at = machine();
    let roots = roots(&at);
    let refused = |entries: &[(&str, &str)]| {
        let mut map = Map::default();
        for (key, value) in entries {
            map.locations.insert(key.to_string(), value.to_string());
        }
        map.check(&roots).unwrap_err().to_string()
    };
    // Control into data, into exec, or anywhere a run reaches.
    assert!(refused(&[("lnk.secrets", "data/secrets.toml")]).contains("control"));
    assert!(refused(&[("lnk.agents", "exec/agents")]).contains("control"));
    assert!(refused(&[("link.agent-settings", "Agents/Settings")]).contains("control"));
    assert!(refused(&[("lnk.agents", "/srv/agents")]).contains("control"));
    // Data into a Safe or exec, and programs out of exec.
    assert!(refused(&[("link.files", "Safe/Files")]).contains("data"));
    assert!(refused(&[("lnk.agent-data", "exec/agents/homes")]).contains("data"));
    assert!(refused(&[("lnk.claude-programs", "data/claude")]).contains("programs"));
    // Overlap the defaults don't have.
    assert!(refused(&[("link.files", "Agents")]).contains("overlap"));
    assert!(refused(&[("link.files", "Teams/Files")]).contains("overlap"));
    // Traversal, the map itself and unknown keys.
    assert!(refused(&[("link.files", "../Files")]).contains("plain path"));
    assert!(refused(&[("lnk.map", "safe/other.toml")]).contains("never mapped"));
    assert!(refused(&[("link.personal", "Personal")]).contains("isn't a location"));
    let hidden_link = Map {
        link: Some(".lnk/Link".into()),
        ..Map::default()
    };
    assert!(hidden_link.check(&roots).is_err());
    // Data moved elsewhere whole is fine, and so are the defaults.
    let mut map = Map::default();
    map.locations
        .insert("link.files".into(), "/mnt/disk/Files".into());
    map.check(&roots).unwrap();
    let mut pinned = Map::default();
    pinned.pin();
    pinned.check(&roots).unwrap();
}

#[test]
fn a_map_newer_than_this_release_is_refused_naming_the_upgrade() {
    let at = machine();
    crate::write_private(&Hidden::Map.at(&at.hidden), b"schema = 2\n").unwrap();
    let refused = load(&at.user, &at.hidden).unwrap_err();
    assert!(
        format!("{refused:#}").contains("lnk upgrade"),
        "{refused:#}"
    );
}

#[test]
fn a_location_reads_its_mapped_path() {
    let at = machine();
    let mut map = Map::default();
    map.pin();
    map.locations
        .insert("lnk.harness-measure-log".into(), "/mnt/measures".into());
    let lock = crate::settings::lock(&Hidden::Map.at_default(&at.hidden)).unwrap();
    write(&lock, &at.user, &at.hidden, &map).unwrap();
    drop(lock);
    assert_eq!(
        Hidden::HarnessMeasureLog.at(&at.hidden),
        PathBuf::from("/mnt/measures")
    );
    assert_eq!(
        Hidden::HarnessMeasureLog.at_default(&at.hidden),
        at.hidden.join("data/measures/measures.jsonl")
    );
    assert_eq!(Hidden::Map.at(&at.hidden), at.hidden.join("safe/map.toml"));
}

/// A location renamed by a step of the map's schema: an older map's entry
/// under the old name reads as the current one, its path and comment
/// kept, and the map holds only current names. Without the step, the old
/// name is refused as no location.
#[test]
fn a_renamed_location_follows_by_the_maps_step() {
    let at = machine();
    let older = include_str!("../../../tests/fixtures/map-1.toml");
    let renamed = crate::schema::Format::parse(include_str!(
        "../../../tests/fixtures/map-renamed.schema.toml"
    ))
    .unwrap();
    let read = renamed.read_saved::<Map>(older).unwrap();
    read.spec.check(&roots(&at)).unwrap();
    assert_eq!(
        read.spec.entry(Entry::Home(Home::Files)),
        Some("/srv/files")
    );
    assert!(!read.spec.locations.contains_key("link.documents"));
    assert!(
        read.text
            .contains("# Kept apart, on the larger disk.\n\"link.files\" = \"/srv/files\""),
        "{}",
        read.text
    );
    let unrenamed = crate::saved::parse::<Map>(older).unwrap();
    let refused = unrenamed.check(&roots(&at)).unwrap_err().to_string();
    assert!(
        refused.contains("\"link.documents\" isn't a location"),
        "{refused}"
    );
}

thread_local! {
    /// How many times this test's thread read a map file.
    pub(super) static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn reads() -> usize {
    READS.with(std::cell::Cell::get)
}

/// A command resolves dozens of paths in the Link folder, each through the
/// map: it's read and checked once while its file is unchanged, read again
/// when another file replaces it, and a refused one is read at each asking.
#[test]
fn the_link_folder_reads_the_map_once_while_it_is_unchanged() {
    let at = machine();
    let map_file = Hidden::Map.at_default(&at.hidden);
    crate::write_private(&map_file, b"schema = 1\nlink = 'Documents/Link'\n").unwrap();
    let before = reads();
    for _ in 0..50 {
        assert_eq!(
            crate::layout::link_in(&at.user, &at.hidden).unwrap(),
            at.user.join("Documents/Link")
        );
    }
    assert_eq!(reads() - before, 1);
    crate::write_private(&map_file, b"schema = 1\nlink = 'Elsewhere/Link'\n").unwrap();
    assert_eq!(
        crate::layout::link_in(&at.user, &at.hidden).unwrap(),
        at.user.join("Elsewhere/Link")
    );
    assert_eq!(reads() - before, 2);
    crate::write_private(&map_file, b"schema = 1\nlink = '.private/Link'\n").unwrap();
    assert!(crate::layout::link_in(&at.user, &at.hidden).is_err());
    assert!(crate::layout::link_in(&at.user, &at.hidden).is_err());
    assert_eq!(reads() - before, 4);
}
