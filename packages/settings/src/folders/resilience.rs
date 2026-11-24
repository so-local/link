//! Damage owned Home and Hidden files, then exercise the actual folder reader.
use super::*;
use link_test_kit::{TempDir, fault::FileFault};

fn fixture() -> (TempDir, PathBuf, Settings) {
    let home = TempDir::new("folder-resilience");
    std::fs::create_dir(home.join("Link")).unwrap();
    let hidden = home.join(".lnk");
    find_in(&home, &hidden).unwrap();
    let mut settings = Settings::load_in(&home, &hidden).unwrap();
    settings.output.color = link_format::look::Choice::Never;
    settings.save_in(&home, &hidden).unwrap();
    std::fs::write(home.join("Link/kept.txt"), b"unaffected user data").unwrap();
    (home, hidden, settings)
}

#[test]
fn a_missing_locator_recovers_the_existing_identity_and_preferences() {
    let (home, hidden, settings) = fixture();
    let marker = std::fs::read(Home::Marker.at(&home.join("Link"))).unwrap();
    let effect = FileFault::Remove
        .apply(&home, Hidden::Settings.at(Path::new(".lnk")))
        .unwrap();
    assert_eq!(effect.after, None);
    assert_eq!(
        find_in(&home, &hidden).unwrap(),
        Found::Here(home.join("Link"))
    );
    assert_eq!(Settings::load_in(&home, &hidden).unwrap(), settings);
    assert_eq!(
        std::fs::read(Home::Marker.at(&home.join("Link"))).unwrap(),
        marker
    );
    assert_eq!(
        std::fs::read(home.join("Link/kept.txt")).unwrap(),
        b"unaffected user data"
    );
}

#[test]
fn a_missing_marker_stays_lost_until_explicit_repair_without_changing_data() {
    let (home, hidden, settings) = fixture();
    let marker = Home::Marker.at(&home.join("Link"));
    let original = std::fs::read(&marker).unwrap();
    FileFault::Remove
        .apply(&home, marker.strip_prefix(home.path()).unwrap())
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            find_in(&home, &hidden).unwrap(),
            Found::Lost(home.join("Link"))
        );
        assert!(
            !marker.exists(),
            "detection must not invent a replacement identity"
        );
        assert_eq!(Settings::load_in(&home, &hidden).unwrap(), settings);
        assert_eq!(
            std::fs::read(home.join("Link/kept.txt")).unwrap(),
            b"unaffected user data"
        );
    }
    // The owner restores the original identity, not a newly generated one.
    link_files::write_private(&marker, &original).unwrap();
    assert_eq!(
        find_in(&home, &hidden).unwrap(),
        Found::Here(home.join("Link"))
    );
    assert_eq!(Settings::load_in(&home, &hidden).unwrap(), settings);
}

#[test]
fn truncated_preferences_are_reported_and_explicit_repair_restores_service() {
    let (home, hidden, settings) = fixture();
    let preferences = Home::Settings.at(&home.join("Link"));
    let original = std::fs::read(&preferences).unwrap();
    // Leave an unfinished table header rather than valid empty preferences.
    let unfinished = b"schema = 1\n[";
    assert!(original.starts_with(unfinished));
    FileFault::Truncate {
        bytes: unfinished.len() as u64,
    }
    .apply(&home, preferences.strip_prefix(home.path()).unwrap())
    .unwrap();
    for _ in 0..2 {
        let error = Settings::load_in(&home, &hidden).unwrap_err();
        assert!(format!("{error:#}").contains(&preferences.display().to_string()));
        assert_eq!(std::fs::read(&preferences).unwrap(), unfinished);
        assert!(find_in(&home, &hidden).is_err());
        assert_eq!(
            std::fs::read(home.join("Link/kept.txt")).unwrap(),
            b"unaffected user data"
        );
    }
    link_files::write_private(&preferences, &original).unwrap();
    assert_eq!(Settings::load_in(&home, &hidden).unwrap(), settings);
    assert_eq!(
        find_in(&home, &hidden).unwrap(),
        Found::Here(home.join("Link"))
    );
}
