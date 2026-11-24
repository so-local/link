//! Projections of the Cargo-bound component registry, with no domain routing data.

use std::{collections::BTreeMap, sync::LazyLock};

use link_component::{Dependency, Resolved};

/// The registry, read once a process ([`link_cli::registry`]).
pub static REGISTRY: LazyLock<&'static BTreeMap<String, Resolved>> =
    LazyLock::new(link_cli::registry);

pub static MODULES: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    REGISTRY
        .values()
        .map(|d| (d.component.id.as_str(), d.component.description.as_str()))
        .collect()
});

pub static GROUPS: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    REGISTRY
        .values()
        .flat_map(|d| {
            d.commands
                .iter()
                .filter(|(_, c)| c.route)
                .map(|(name, _)| (name.as_str(), d.component.id.as_str()))
        })
        .collect()
});

/// Each module group's alias and the group it stands for, as its
/// `actions.toml` declares them (`[aliases]`): `man` for `manual`.
pub static ALIASES: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    REGISTRY
        .values()
        .flat_map(|d| {
            d.commands
                .iter()
                .filter(|(_, c)| c.route)
                .filter_map(|(name, c)| Some((c.alias.as_deref()?, name.as_str())))
        })
        .collect()
});

/// The group `word` names: the one it's the alias of, or itself.
pub fn unaliased(word: &str) -> &str {
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == word)
        .map_or(word, |(_, group)| group)
}

pub static COMMANDS: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    REGISTRY
        .values()
        .flat_map(|d| d.commands.keys().filter_map(|name| name.split_once(' ')))
        .collect()
});

static MENU_ITEMS: LazyLock<BTreeMap<&'static str, Vec<&'static str>>> = LazyLock::new(|| {
    let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for d in REGISTRY.values() {
        if let Some(menu) = &d.menu {
            groups
                .entry(menu.group.as_deref().unwrap_or(&d.component.id))
                .or_default()
                .push(d.component.id.as_str());
        }
    }
    groups
});

type Choice = (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static str,
);
pub static USES: LazyLock<Vec<Choice>> = LazyLock::new(|| {
    MENU_ITEMS
        .iter()
        .map(|(group, names)| {
            let menu = REGISTRY[names[0]].menu.as_ref().expect("a menu group");
            (
                *group,
                menu.label.as_str(),
                names.as_slice(),
                menu.next.as_deref().unwrap_or("lnk module list"),
            )
        })
        .collect()
});

static RECOMMENDS: LazyLock<BTreeMap<&'static str, Vec<(&'static str, &'static str)>>> =
    LazyLock::new(|| {
        REGISTRY
            .values()
            .map(|d| {
                (
                    d.component.id.as_str(),
                    module_edges(d, &d.recommends)
                        .into_iter()
                        .filter_map(|id| {
                            REGISTRY
                                .get(id)
                                .map(|p| (id, p.component.description.as_str()))
                        })
                        .collect(),
                )
            })
            .collect()
    });

fn module_edges(
    d: &'static Resolved,
    edges: &'static [link_component::Requirement],
) -> Vec<&'static str> {
    edges
        .iter()
        .filter(|e| {
            e.when.as_ref().is_none_or(|p| {
                (p.os.is_empty() || p.os.iter().any(|s| s == std::env::consts::OS))
                    && (p.arch.is_empty() || p.arch.iter().any(|s| s == std::env::consts::ARCH))
                    && p.options.is_empty()
            })
        })
        .filter_map(|e| match d.dependencies.get(&e.dependency) {
            Some(Dependency::Module { id, .. }) => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

pub fn recommends(module: &str) -> &'static [(&'static str, &'static str)] {
    RECOMMENDS.get(module).map(Vec::as_slice).unwrap_or(&[])
}

#[cfg(test)]
pub fn declared_modules(module: &str) -> Vec<&'static str> {
    REGISTRY
        .get(module)
        .into_iter()
        .flat_map(|d| d.dependencies.values())
        .filter_map(|dep| match dep {
            Dependency::Module { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// Canonical IDs for installed files.
pub fn installed<'a>(
    registry: &'a BTreeMap<String, Resolved>,
    directory: &std::path::Path,
) -> Vec<&'a str> {
    registry
        .values()
        .filter(|d| directory.join(&d.component.program).is_file())
        .map(|d| d.component.id.as_str())
        .collect()
}

/// The source menu's default selects a choice; recommendations do not.
pub fn default_choices() -> Vec<usize> {
    USES.iter()
        .enumerate()
        .filter(|(_, (_, _, ids, _))| {
            ids.iter()
                .any(|id| REGISTRY[*id].menu.as_ref().is_some_and(|m| m.default))
        })
        .map(|(index, _)| index)
        .collect()
}
