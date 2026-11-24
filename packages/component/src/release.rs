//! What a release ships. Every member is released unless its manifest's
//! `[package.metadata.link]` says `released = false`: it then stays in the
//! code, built and tested, and dev and staging builds keep it, while a release's
//! registry, archives and website leave it out.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, bail};

use crate::{Dependency, Requirement, Resolved};

/// Whether a member is released, from its `[package.metadata.link]` as
/// `cargo metadata` gives it. `released` is absent or `false`: `true` says
/// nothing absence doesn't, and any other value is refused.
pub fn released(link: Option<&serde_json::Value>) -> anyhow::Result<bool> {
    match link.and_then(|link| link.get("released")) {
        None => Ok(true),
        Some(serde_json::Value::Bool(false)) => Ok(false),
        Some(other) => bail!("released is false or absent, not {other}"),
    }
}

/// Whether the member a `Cargo.toml` describes is released.
pub fn manifest_released(manifest: &toml::Value) -> anyhow::Result<bool> {
    let link = manifest
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("link"))
        .map(serde_json::to_value)
        .transpose()?;
    released(link.as_ref())
}

/// A release's registry: the released components alone. A released one
/// may recommend a held one, a choice the release drops, but never require
/// one, which would install a program the release doesn't have.
pub fn registry(
    mut registry: BTreeMap<String, Resolved>,
    held: &BTreeSet<String>,
) -> anyhow::Result<BTreeMap<String, Resolved>> {
    registry.retain(|id, _| !held.contains(id));
    for (id, descriptor) in &mut registry {
        let aliases: BTreeSet<String> = descriptor
            .dependencies
            .iter()
            .filter(|(_, dependency)| {
                matches!(dependency, Dependency::Module { id, .. } if held.contains(id))
            })
            .map(|(alias, _)| alias.clone())
            .collect();
        let named = |edge: &Requirement| aliases.contains(&edge.dependency);
        let mut requires = descriptor.requires.iter().map(|edge| ("", edge)).chain(
            descriptor.commands.iter().flat_map(|(name, command)| {
                command.requires.iter().map(move |e| (name.as_str(), e))
            }),
        );
        if let Some((command, edge)) = requires.find(|(_, edge)| named(edge)) {
            bail!(
                "released {id}{}{command} requires {}, which isn't released",
                if command.is_empty() { "" } else { " " },
                edge.dependency
            );
        }
        if let Some(upgrade) = descriptor
            .upgrade_requires
            .iter()
            .find(|u| held.contains(*u))
        {
            bail!("released {id} upgrades with {upgrade}, which isn't released");
        }
        descriptor.recommends.retain(|edge| !named(edge));
        descriptor
            .dependencies
            .retain(|alias, _| !aliases.contains(alias));
    }
    crate::validate_registry(&registry).context("a release's registry")?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(id: &str, source: &str) -> (String, Resolved) {
        let component = crate::Component {
            id: id.into(),
            cargo_name: format!("link-{id}"),
            version: "1.0.0".into(),
            license: "MIT".into(),
            description: "An example".into(),
            program: format!("lnk-{id}"),
            kind: crate::Kind::Module,
            reads_control: true,
        };
        let source = format!("schema = 1\n{source}\n[module]\nid = \"{id}\"\nkind = \"module\"\n");
        let resolved = crate::Source::parse(&source)
            .unwrap()
            .resolve(component, vec![])
            .unwrap();
        (id.into(), resolved)
    }

    fn held(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn only_false_holds_a_member_back() {
        assert!(released(None).unwrap());
        assert!(released(Some(&serde_json::json!({"role": "library"}))).unwrap());
        assert!(!released(Some(&serde_json::json!({"released": false}))).unwrap());
        for refused in [serde_json::json!(true), serde_json::json!("false")] {
            assert!(released(Some(&serde_json::json!({"released": refused}))).is_err());
        }
        let manifest = toml::from_str("[package.metadata.link]\nreleased = false").unwrap();
        assert!(!manifest_released(&manifest).unwrap());
        assert!(manifest_released(&toml::from_str("[package]\nname = 'a'").unwrap()).unwrap());
    }

    #[test]
    fn a_release_drops_held_components_and_recommendations_of_them() {
        let registry = BTreeMap::from([
            component(
                "box",
                "recommends = [{ dependency = \"measure\" }, { dependency = \"cloud\" }]\n\
                 [dependencies.measure]\nkind = \"module\"\nid = \"measure\"\n\
                 [dependencies.cloud]\nkind = \"module\"\nid = \"cloud\"",
            ),
            component("cloud", ""),
            component("measure", ""),
        ]);
        let release = super::registry(registry.clone(), &held(&["measure"])).unwrap();
        assert_eq!(release.keys().collect::<Vec<_>>(), ["box", "cloud"]);
        let recommended: Vec<_> = release["box"]
            .recommends
            .iter()
            .map(|e| &e.dependency)
            .collect();
        assert_eq!(recommended, ["cloud"]);
        assert!(!release["box"].dependencies.contains_key("measure"));
        assert_eq!(super::registry(registry, &held(&[])).unwrap().len(), 3);
    }

    #[test]
    fn a_release_refuses_a_released_component_requiring_a_held_one() {
        let registry = BTreeMap::from([
            component(
                "box",
                "requires = [{ dependency = \"storage\" }]\n\
                 [dependencies.storage]\nkind = \"module\"\nid = \"storage\"",
            ),
            component("storage", ""),
        ]);
        let error = super::registry(registry, &held(&["storage"])).unwrap_err();
        assert!(
            error.to_string().contains("box requires storage"),
            "{error}"
        );
    }
}
