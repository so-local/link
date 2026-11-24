//! No code kept for an older version: function code reads and writes
//! only the current shape, and support for an older one is declared data
//! in one migration channel (the `migrations` skill). A source file whose
//! line says one of [`PHRASES`] (old-format wording, an old-path
//! fallback, a format sniffed, backward compatibility or a deprecated
//! interface) or names an alias in code ([`ALIASES`], a serde or clap
//! alias however it's spaced) is code for an old `lnk`, module, file or
//! name, and fails unless its workspace's `legacy.toml` lists it with at
//! least as many such lines. The list may shrink, never grow, as
//! `large-files.toml` does, so new legacy comes in only by saying why, in
//! review. A line naming a migration or saying its words ([`MIGRATION`])
//! is refused outside a `migrations/` folder, which no list admits. Each
//! workspace runs it on its own root.

use std::collections::BTreeMap;
use std::path::Path;

/// What legacy says of itself, matched in any case: an older shape kept,
/// a file looked for at its former place, and a format told apart by its
/// bytes rather than its schema.
pub const PHRASES: [&str; 22] = [
    concat!("from ", "before"),
    concat!("leg", "acy "),
    concat!("older", " than"),
    concat!("migrate", " from"),
    concat!("backward ", "compatibility"),
    concat!("depre", "cated"),
    concat!("compatibility ", "shim"),
    // An old-path fallback.
    concat!("old ", "location"),
    concat!("previous ", "location"),
    concat!("previous ", "path"),
    concat!("fall back ", "to the old"),
    concat!("falls back ", "to the old"),
    concat!("fallback ", "to the old"),
    concat!("where it ", "used to be"),
    // A format told apart by its bytes.
    concat!("sniff ", "the format"),
    concat!("sniffs ", "the format"),
    concat!("format ", "sniff"),
    concat!("guess ", "the format"),
    concat!("either ", "shape"),
    concat!("both ", "shapes"),
    concat!("whichever ", "shape"),
    concat!("old or ", "new shape"),
];

/// An alias named in code, a serde field's or a clap command's or flag's,
/// visible or not, matched with the line's spaces taken out: the name
/// written as a literal. A generator handing clap a group's alias from
/// its declaration, `[aliases]` in `actions.toml`, names none: that
/// alias is a short form a person types, never an older name.
pub const ALIASES: [&str; 4] = [
    concat!("ali", "as=\""),
    concat!("ali", "as(\""),
    concat!("ali", "ases=["),
    concat!("ali", "ases([\""),
];

/// What names a migration, or says a value was migrated or is supported
/// for an older shape, matched in any case: the wording of a schema step's
/// code, which only a `migrations/` folder holds ([`MIGRATIONS`]).
pub const MIGRATION: [&str; 8] = [
    concat!("migration ", "code"),
    concat!("schema ", "migration"),
    concat!("migrations", "::"),
    concat!("migrated ", "value"),
    concat!("migrated ", "spec"),
    concat!("migrated ", "file"),
    concat!("migrated ", "shape"),
    concat!("supported for an ", "older"),
];

/// The folder a schema step's code lives in, the only place its words
/// and names are said.
pub const MIGRATIONS: &str = "migrations";

/// The source files: what Link runs, its tests, scripts, workflows and
/// migrations.
const SOURCE: [&str; 11] = [
    "rs", "ts", "tsx", "js", "mjs", "cjs", "sh", "py", "yml", "yaml", "sql",
];

/// How many lines of `text` say a phrase.
pub fn count(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let line = line.to_lowercase();
            let bare: String = line.chars().filter(|c| !c.is_whitespace()).collect();
            PHRASES.iter().any(|phrase| line.contains(phrase))
                || ALIASES.iter().any(|alias| bare.contains(alias))
        })
        .count()
}

/// The lines of `text` that name a migration or say its words: a
/// [`MIGRATION`] phrase, or a name of one schema to the next: a kind,
/// its schema, `_to_` and the next.
pub fn migration_lines(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.to_lowercase();
            MIGRATION.iter().any(|phrase| line.contains(phrase)) || names_a_step(&line)
        })
        .map(|(at, _)| at + 1)
        .collect()
}

/// Whether `line` holds `_<schema>_to_<schema>`, a migration's name.
fn names_a_step(line: &str) -> bool {
    let separator = concat!("_to", "_");
    line.match_indices(separator).any(|(at, _)| {
        let before = line[..at].trim_end_matches(|c: char| c.is_ascii_digit());
        let after = &line[at + separator.len()..];
        before.len() < at
            && before.ends_with('_')
            && after.starts_with(|c: char| c.is_ascii_digit())
    })
}

/// Whether `file`, a path from the root, is in a `migrations/` folder.
pub fn in_migrations(file: &str) -> bool {
    Path::new(file)
        .parent()
        .is_some_and(|dir| dir.components().any(|part| part.as_os_str() == MIGRATIONS))
}

/// The source files git tracks under `root`, by their paths from it.
#[expect(clippy::disallowed_methods, reason = "git lists the tracked files")]
fn sources(root: &Path) -> Vec<String> {
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .expect("git ls-files");
    assert!(listed.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|file| {
            Path::new(file)
                .extension()
                .is_some_and(|ext| SOURCE.contains(&ext.to_string_lossy().as_ref()))
        })
        .map(str::to_string)
        .collect()
}

/// Each source file git tracks under `root` that says a phrase, by its
/// path from `root`, with its count.
pub fn found(root: &Path) -> BTreeMap<String, usize> {
    sources(root)
        .into_iter()
        .filter_map(|file| {
            let text = std::fs::read_to_string(root.join(&file)).ok()?;
            let n = count(&text);
            (n > 0).then_some((file, n))
        })
        .collect()
}

/// Each line outside a `migrations/` folder that names a migration or
/// says its words: function code never does, and no list admits one.
pub fn misplaced(root: &Path) -> Vec<String> {
    sources(root)
        .into_iter()
        .filter(|file| !in_migrations(file))
        .flat_map(|file| {
            let text = std::fs::read_to_string(root.join(&file)).unwrap_or_default();
            migration_lines(&text)
                .into_iter()
                .map(move |line| {
                    format!(
                        "{file}:{line}: names a migration or says its words outside a \
                         {MIGRATIONS}/ folder: a schema step's code and all it says live there"
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The problems of `found` against `listed`, a `legacy.toml`'s `[lines]`:
/// none when every file is listed at no fewer lines than it has, and
/// every listed file still has them all.
pub fn compare(found: &BTreeMap<String, usize>, listed: &BTreeMap<String, usize>) -> Vec<String> {
    let mut problems = Vec::new();
    for (file, &n) in found {
        match listed.get(file) {
            None => problems.push(format!(
                "{file}: {n} lines say {PHRASES:?}: delete the code for the older version; \
                 legacy.toml takes no new file"
            )),
            Some(&was) if n > was => problems.push(format!(
                "{file}: {n} lines say {PHRASES:?}, up from {was}: delete the new legacy"
            )),
            Some(&was) if n < was => problems.push(format!(
                "{file}: {n} lines say {PHRASES:?}, down from {was}: lower it in legacy.toml"
            )),
            Some(_) => {}
        }
    }
    for file in listed.keys().filter(|file| !found.contains_key(*file)) {
        problems.push(format!(
            "{file} says none of {PHRASES:?}: take it out of legacy.toml"
        ));
    }
    problems
}

/// The check on `root`, by the list at `list`.
pub fn check(root: &Path, list: &Path) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct List {
        lines: BTreeMap<String, usize>,
    }
    let text = std::fs::read_to_string(list).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    let list: List = toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    let mut problems = compare(&found(root), &list.lines);
    problems.extend(misplaced(root));
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_lines_in_any_case() {
        assert_eq!(
            count(&format!(
                "a {} name\n{} specs, {}\nnew",
                PHRASES[1],
                PHRASES[0],
                PHRASES[1].to_uppercase()
            )),
            2
        );
        assert_eq!(count("plain code"), 0);
    }

    #[test]
    fn moving_an_agent_is_not_code_for_an_older_format() {
        assert_eq!(count("lnk agent migrate --harness openclaw"), 0);
        assert_eq!(count("native_jobs_migrate_between_harnesses"), 0);
        assert_eq!(count("Plan::LegacyProMonthly"), 0);
        assert_eq!(count("protocol is incompatible"), 0);
        assert_eq!(count("shim by basename"), 0);
        assert_eq!(count("roles = [\"current\", \"previous\", \"legacy\"]"), 0);
        assert_eq!(count(&format!("{} an older format", PHRASES[3])), 1);
        assert_eq!(count("legacy reader for a saved config"), 1);
        assert_eq!(count("backward compatibility for the old wire"), 1);
        assert_eq!(count("#[serde(alias = \"previous_name\")]"), 1);
    }

    #[test]
    fn an_alias_however_spaced_an_old_path_fallback_and_a_sniffed_format() {
        let alias = concat!("ali", "as");
        for line in [
            format!("#[serde({alias}=\"old\")]"),
            format!("#[arg(long, {alias} =  \"old\")]"),
            format!(".visible_{alias}(\"old\")"),
            format!(".{alias}es([\"a\", \"b\"])"),
            format!("// {} to the old file", concat!("falls ", "back")),
            format!(
                "// read from the {} when missing",
                concat!("old ", "location")
            ),
            format!("// {} format from the first byte", concat!("sniff ", "the")),
            format!("// takes {} of the file", concat!("either ", "shape")),
        ] {
            assert_eq!(count(&line), 1, "{line}");
        }
        for line in [
            "let aliased = names(\"x\");",
            "x-content-type-options: nosniff",
            "the old path is refused",
            "short_aliases are listed",
        ] {
            assert_eq!(count(line), 0, "{line}");
        }
        // A generator handing clap a declared alias names no old one.
        let visible = concat!("visible_ali", "as");
        for line in [
            format!("built += &format!(\".{visible}({{alias:?}})\");"),
            format!("command.{visible}(declared)"),
        ]
        .iter()
        {
            assert_eq!(count(line), 0, "{line}");
        }
    }

    #[test]
    fn a_migrations_name_and_words_are_found_and_only_its_folder_holds_them() {
        let name = format!("{}{}", "sandbox_2", concat!("_to", "_3"));
        for line in [
            format!("let x = {name}::run;"),
            format!("migration = \"{name}\""),
            format!("// the {} for disks", MIGRATION[0]),
            format!("use crate::{}run;", MIGRATION[2]),
            format!("// a {} keeps its name", MIGRATION[3].to_uppercase()),
        ] {
            assert_eq!(migration_lines(&line), [1], "{line}");
        }
        for line in [
            "lnk agent migrate --harness openclaw",
            "mod migrations;",
            "fn map_to_2(x: u32)",
            "a_to_b and v1_to_v2",
            "let named = migration::name(kind, from);",
            "supabase db push runs the migrations",
        ] {
            assert!(migration_lines(line).is_empty(), "{line}");
        }
        assert!(in_migrations("src/migrations/disks.rs"));
        assert!(in_migrations("src/schema/tests/coded/migrations/mod.rs"));
        assert!(!in_migrations("src/migrations.rs"));
        assert!(!in_migrations("src/schema.rs"));
    }

    #[test]
    fn the_list_only_shrinks() {
        let list = |pairs: &[(&str, usize)]| {
            pairs
                .iter()
                .map(|(f, n)| (f.to_string(), *n))
                .collect::<BTreeMap<_, _>>()
        };
        assert!(compare(&list(&[("a.rs", 2)]), &list(&[("a.rs", 2)])).is_empty());
        for (found, listed, says) in [
            (list(&[("b.rs", 1)]), list(&[]), "takes no new file"),
            (list(&[("a.rs", 3)]), list(&[("a.rs", 2)]), "up from 2"),
            (list(&[("a.rs", 1)]), list(&[("a.rs", 2)]), "lower it"),
            (list(&[]), list(&[("a.rs", 2)]), "take it out"),
        ] {
            let problems = compare(&found, &listed);
            assert_eq!(problems.len(), 1, "{problems:?}");
            assert!(problems[0].contains(says), "{problems:?}");
        }
    }
}
