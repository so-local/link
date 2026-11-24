//! Every ready-made agent in `agents/` starts blank: no row, list item or
//! filled field in its Home, so it never holds an example its owner never
//! gave, and comes with the tools `ready-mades.toml` pins for it
//! (`link_repository_check::ready_mades`, by `ready-mades.toml`).

use std::path::Path;

use link_repository_check::ready_mades::{entries, problems, tool_problems};

#[test]
fn every_ready_made_home_holds_only_headings_and_empty_fields() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = problems(&root, &here.join("ready-mades.toml"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_ready_made_ships_the_tools_it_pins() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = tool_problems(&root, &here.join("ready-mades.toml"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_csv_holds_its_header_and_no_row() {
    assert!(entries("log.csv", "date,food\n").is_empty());
    let found = entries("foods.csv", "food,kcal\nOats,379\n\n");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("2: a row"), "{found:?}");
}

#[test]
fn markdown_holds_headings_table_headers_and_empty_fields() {
    let blank = "# Plan\n\nWhat it holds: one line each.\n\nstatus:  <!-- \"active\" -->\nweeks: \n\n\
                 | zone | name |\n|---|---|\n\n## Notes\n";
    assert!(
        entries("plan.md", blank).is_empty(),
        "{:?}",
        entries("plan.md", blank)
    );
    let filled = "# Shopping\n\n- pasta\n2. tomatoes\n\n| zone | name |\n|---|---|\n| Z1 | Recovery |\n\
                  status: none\nweeks: 0\n";
    let found = entries("shopping.md", filled);
    let lines: Vec<&str> = found.iter().map(|f| f.split(':').next().unwrap()).collect();
    assert_eq!(lines, ["3", "4", "8", "9", "10"], "{found:?}");
}

#[test]
fn toml_holds_only_empty_values() {
    let blank =
        "onboarded = false\nname = \"\"\nweight_kg = 0.0\n[availability]\ndays = []\nminutes = 0\n";
    assert!(entries("profile.toml", blank).is_empty());
    let found = entries(
        "profile.toml",
        "name = \"Ana\"\n[[goals]]\ntitle = \"\"\n[health]\nmax_hr_bpm = 190\n",
    );
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().any(|f| f.contains("`name`")), "{found:?}");
    assert!(found.iter().any(|f| f.contains("`goals`")), "{found:?}");
    assert!(
        found.iter().any(|f| f.contains("`health.max_hr_bpm`")),
        "{found:?}"
    );
}

#[test]
fn a_home_file_of_another_kind_is_refused() {
    assert_eq!(entries("photo.png", "").len(), 1);
}
