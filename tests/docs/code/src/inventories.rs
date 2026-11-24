//! Inventories enumerate an existing source, never a second handwritten list.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use link_docs_check::navigation::Update;
use link_docs_check::{Finding, Tree, fail};

mod command_names;
mod contribute_here;
mod credentials;

/// An inventory's entries, each with the files that declare it.
pub type Items = BTreeMap<String, BTreeSet<PathBuf>>;

pub const SPECS: &str = "docs/config/specs/README.md";
pub const COMMANDS: &str = "docs/more/reference/commands.md";
pub const CREDENTIALS: &str = "modules/security/docs/config/credentials/README.md";
pub const STATE: &str = "packages/files/docs/more/libraries/files/files.md";
pub const LAYOUT: &str = "docs/more/reference/locations.md";

/// Verify credential producers in any workspace containing their provider.
pub fn credential_implementations(tree: &Tree) -> Result<(), String> {
    credentials::implementations(tree)
}

fn sources(tree: &Tree) -> BTreeMap<String, PathBuf> {
    tree.files
        .iter()
        .filter_map(|(path, text)| {
            if path.file_name().is_none_or(|name| name != "Cargo.toml") {
                return None;
            }
            let value: toml::Value = toml::from_str(text).ok()?;
            let name = value.get("package")?.get("name")?.as_str()?.to_owned();
            let owner = path.parent()?;
            let source = ["actions.toml", "module.toml", "Cargo.toml"]
                .into_iter()
                .map(|name| owner.join(name))
                .find(|path| tree.files.contains_key(path))?;
            Some((name, source))
        })
        .collect()
}

fn entry(items: &mut Items, name: String, source: &Path) {
    items.entry(name).or_default().insert(source.to_path_buf());
}

fn declarations(
    commands: &mut Items,
    declared: &BTreeMap<String, link_component::Command>,
    source: &Path,
    core: bool,
) {
    let routes: Vec<_> = declared
        .iter()
        .filter(|(_, c)| c.route)
        .map(|(p, _)| p)
        .collect();
    // A person's commands: plumbing, which Link runs and never offers, is
    // in no index.
    for (path, _) in declared.iter().filter(|(_, command)| !command.hidden) {
        if path != "root"
            && (core
                || routes
                    .iter()
                    .any(|route| path == *route || path.starts_with(&format!("{route} "))))
        {
            entry(commands, format!("lnk {path}"), source);
        }
    }
}

/// A page's title, its first H1, past the generated marker.
pub fn title_of(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|title| title.trim().to_string())
}

/// One inventory's entries by its kind (`environment`, `state`, ...):
/// the set its generated list is made from.
pub fn catalog(tree: &Tree, kind: &str) -> Result<Items, String> {
    catalogs(tree)?
        .into_iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, _, items)| items)
        .ok_or_else(|| format!("no {kind} inventory"))
}

/// Each inventory: its kind, the page its list is written into (none for
/// one another generator writes, as the variables reference), and its
/// entries.
type Inventory = (&'static str, Option<&'static str>, Items);

fn catalogs(tree: &Tree) -> Result<Vec<Inventory>, String> {
    // The specs index, from every kind's binding to its page.
    let mut specs = Items::new();
    for kind in crate::references::kinds::kinds() {
        let page = tree.root.join(kind.page);
        if let Some(title) = tree.text(&page).and_then(title_of) {
            entry(&mut specs, title, &page);
        }
    }
    let mut commands = Items::new();
    let mut parts = Items::new();
    for relative in crate::references::composed_pages()? {
        let page = tree.root.join(relative);
        if let Some(title) = tree.text(&page).and_then(title_of) {
            entry(&mut parts, title, &page);
        }
    }
    let mut variables = Items::new();
    let sources = sources(tree);
    for component in link_cli::registry().values() {
        declarations(
            &mut commands,
            &component.commands,
            sources
                .get(&component.component.cargo_name)
                .expect("a registered component source"),
            false,
        );
    }
    let core = tree.root.join("core/cli/actions.toml");
    let actions =
        link_component::actions::parse(tree.text(&core).expect("core command declarations"))
            .expect("valid core declarations");
    declarations(
        &mut commands,
        &link_component::actions::commands(&actions),
        &core,
        true,
    );
    let declared = crate::environment::declarations(tree);
    if !declared.errors.is_empty() {
        return Err(format!(
            "invalid environment declarations: {:?}",
            declared.errors
        ));
    }
    for (name, paths) in declared.names {
        variables.entry(name).or_default().extend(paths);
    }
    let mut namespaces = Items::new();
    for (owner, patterns) in declared.namespaces {
        for pattern in patterns {
            entry(&mut namespaces, pattern, &owner.join("variables.toml"));
        }
    }
    let (command_names, command_aliases) = command_names::catalog(tree)?;
    let credentials = credentials::catalog(tree)?;
    let mut state = Items::new();
    let state_source = tree.root.join("packages/files/src/state.rs");
    for area in link_files::state::Area::ALL {
        entry(
            &mut state,
            format!("~/.lnk/{}", area.relative()),
            &state_source,
        );
    }
    let mut layout = Items::new();
    let layout_source = tree.root.join("packages/files/layout.toml");
    for (location, path) in link_files::layout::map::Entry::all().map(|location| {
        let path = match location {
            link_files::layout::map::Entry::Link => "~/Link".to_owned(),
            link_files::layout::map::Entry::Home(home) => format!("~/Link/{}", home.relative()),
            link_files::layout::map::Entry::Hidden(hidden) => {
                format!("~/.lnk/{}", hidden.relative())
            }
        };
        (location, path)
    }) {
        let owner = match location {
            link_files::layout::map::Entry::Link => "link",
            link_files::layout::map::Entry::Home(home) => home.owner(),
            link_files::layout::map::Entry::Hidden(hidden) => hidden.owner(),
        };
        let kind = format!("{:?}", location.kind()).to_lowercase();
        let owner = if owner == "link" {
            "Link's own".to_owned()
        } else {
            format!("{owner}'")
        };
        entry(
            &mut layout,
            format!("`{path}`: {kind}, {owner}, location `{}`", location.name()),
            &layout_source,
        );
    }
    entry(
        &mut layout,
        "`~/.lnk/safe/map.toml`: control, Link's own, never mapped".to_owned(),
        &layout_source,
    );
    Ok(vec![
        ("layout", Some(LAYOUT), layout),
        ("state", Some(STATE), state),
        ("specs", Some(SPECS), specs),
        ("spec-parts", Some(SPECS), parts),
        ("commands", Some(COMMANDS), commands),
        (
            "namespace-commands",
            Some(command_names::COMMANDS),
            command_names,
        ),
        (
            "command-aliases",
            Some(command_names::ALIASES),
            command_aliases,
        ),
        // The variables reference lists them, generated from the same
        // declarations (`link-actions-surfaces`).
        ("environment", None, variables),
        ("environment-namespaces", None, namespaces),
        ("credentials", Some(CREDENTIALS), credentials),
        (
            "contribute-here",
            Some(contribute_here::PAGE),
            contribute_here::catalog(tree),
        ),
    ])
}

fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut path = PathBuf::new();
    for _ in common..from.len() {
        path.push("..");
    }
    for part in &to[common..] {
        path.push(part.as_os_str());
    }
    path.to_string_lossy().replace('\\', "/")
}

fn render(kind: &str, page: &Path, items: &Items) -> String {
    let mut block = format!("<!-- inventory:{kind}:start -->\n\n");
    for (name, sources) in items {
        let name = if matches!(
            kind,
            "specs" | "spec-parts" | "credentials" | "layout" | "contribute-here"
        ) {
            name.clone()
        } else if kind == "command-aliases" {
            let (alias, target) = name.split_once(" → ").expect("alias mapping");
            format!("`{alias}` → `{target}`")
        } else {
            format!("`{name}`")
        };
        if kind == "contribute-here" {
            let source = sources.iter().next().expect("a page");
            block += &contribute_here::line(&name, page, source);
            continue;
        }
        if kind == "layout" {
            // One source, layout.toml, linked once above the list.
            block += &format!("- {name}\n");
            continue;
        }
        let sources: Vec<_> = sources.iter().collect();
        block += &format!(
            "- [{name}]({})",
            relative(page.parent().unwrap(), sources[0])
        );
        if sources.len() > 1 {
            let others: Vec<_> = sources[1..]
                .iter()
                .enumerate()
                .map(|(i, path)| format!("[{}]({})", i + 2, relative(page.parent().unwrap(), path)))
                .collect();
            block += &format!(" — also {}", others.join(", "));
        }
        block.push('\n');
    }
    block += &format!("\n<!-- inventory:{kind}:end -->");
    block
}

/// Only the named, unique marker pair is writable. Everything around it stays authored.
fn replace(text: &str, kind: &str, block: &str) -> Result<String, String> {
    let start = format!("<!-- inventory:{kind}:start -->");
    let end = format!("<!-- inventory:{kind}:end -->");
    let starts: Vec<_> = text.match_indices(&start).collect();
    let ends: Vec<_> = text.match_indices(&end).collect();
    match (starts.as_slice(), ends.as_slice()) {
        ([(a, _)], [(b, _)]) if b > a => Ok(format!(
            "{}{}{}",
            &text[..*a],
            block,
            &text[b + end.len()..]
        )),
        _ => Err(format!(
            "{kind}: expected one ordered inventory marker pair"
        )),
    }
}

pub fn updates(tree: &Tree) -> Result<Vec<Update>, String> {
    let mut pages = BTreeMap::<PathBuf, String>::new();
    for (kind, relative, items) in catalogs(tree)? {
        let Some(relative) = relative else { continue };
        let path = tree.root.join(relative);
        let old = pages
            .get(&path)
            .map(String::as_str)
            .or_else(|| tree.text(&path))
            .ok_or_else(|| format!("{}: inventory page missing", path.display()))?;
        let text = replace(old, kind, &render(kind, &path, &items))?;
        pages.insert(path, text);
    }
    Ok(pages
        .into_iter()
        .filter(|(path, text)| tree.text(path) != Some(text.as_str()))
        .map(|(path, text)| Update { path, text })
        .collect())
}

pub fn check(tree: &Tree) -> Vec<Finding> {
    match updates(tree) {
        Ok(updates) => updates
            .into_iter()
            .map(|update| {
                fail(format!(
                    "{}: source-backed inventory is missing entries or stale",
                    tree.shown(&update.path)
                ))
            })
            .collect(),
        Err(error) => vec![fail(error)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_define_the_command_inventory_scope() {
        let option: link_component::OptionDeclaration =
            toml::from_str("type = 'string'\nenvironment = 'LNK_FROM_OPTION'\n").unwrap();
        let root = link_component::Command::default();
        let declared = BTreeMap::from([
            (
                "agent".into(),
                link_component::Command {
                    route: true,
                    ..Default::default()
                },
            ),
            (
                "agent send".into(),
                link_component::Command {
                    options: BTreeMap::from([("model".into(), option)]),
                    ..Default::default()
                },
            ),
            ("standalone".into(), link_component::Command::default()),
            ("root".into(), root),
        ]);
        let mut commands = Items::new();
        declarations(
            &mut commands,
            &declared,
            Path::new("/repo/actions.toml"),
            false,
        );
        assert_eq!(
            commands.keys().map(String::as_str).collect::<Vec<_>>(),
            ["lnk agent", "lnk agent send"]
        );
    }

    #[test]
    fn additions_removals_and_duplicates_change_the_checked_output() {
        let page = Path::new("/repo/docs/catalog.md");
        let mut items = Items::new();
        entry(
            &mut items,
            "LNK_A".into(),
            Path::new("/repo/core/actions.toml"),
        );
        let first = render("environment", page, &items);
        entry(
            &mut items,
            "LNK_A".into(),
            Path::new("/repo/core/actions.toml"),
        );
        assert_eq!(render("environment", page, &items), first);
        entry(
            &mut items,
            "LNK_B".into(),
            Path::new("/repo/module/actions.toml"),
        );
        let second = render("environment", page, &items);
        assert_ne!(first, second);
        items.remove("LNK_A");
        let third = render("environment", page, &items);
        assert!(!third.contains("LNK_A"));
        assert!(third.contains("LNK_B"));
        assert_ne!(second, third);
        let text = format!("# Catalog\n\nIntro.\n\n{first}\n\n## Details\n\nPreserved.\n");
        let changed = replace(&text, "environment", &third).unwrap();
        assert!(changed.starts_with("# Catalog\n\nIntro.\n"));
        assert!(changed.ends_with("## Details\n\nPreserved.\n"));
        assert_eq!(replace(&changed, "environment", &third).unwrap(), changed);
        let forged = changed.replace("- [`LNK_B`]", "- [`LNK_B`]\n- [`LNK_B`]");
        assert_ne!(replace(&forged, "environment", &third).unwrap(), forged);
    }

    #[test]
    fn malformed_markers_cannot_overwrite_authored_content() {
        for text in [
            "",
            "<!-- inventory:x:end -->\n<!-- inventory:x:start -->",
            "<!-- inventory:x:start -->\n<!-- inventory:x:start -->\n<!-- inventory:x:end -->",
        ] {
            assert!(replace(text, "x", "replacement").is_err());
        }
    }
}
