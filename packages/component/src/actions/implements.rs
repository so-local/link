//! The contracts a version 1 module implements (`[implements]`), found
//! through its Cargo dependencies: an implementer depends on its contract's
//! crate for the contract's types, so that crate's folder holds the
//! contract's `module.toml`, `actions.toml` and `types.toml`.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::Context;
use link_actions::{ActionId, ActionsFile};

/// The folder of each contract `implements` names, by its ID, from the
/// dependencies of the crate in `directory`.
pub(crate) fn contracts(
    directory: &Path,
    implements: &BTreeMap<String, Vec<ActionId>>,
) -> anyhow::Result<BTreeMap<String, PathBuf>> {
    let mut found = BTreeMap::new();
    if implements.is_empty() {
        return Ok(found);
    }
    for folder in dependencies(directory)? {
        if let Some(id) = module_id(&folder)?
            && implements.contains_key(&id)
        {
            found.insert(id, folder);
        }
    }
    Ok(found)
}

/// The ID `module.toml` in `directory` gives its module, if it has one.
pub(crate) fn module_id(directory: &Path) -> anyhow::Result<Option<String>> {
    let path = directory.join("module.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let table: toml::Table =
        toml::from_str(&text).with_context(|| format!("{}", path.display()))?;
    Ok(table
        .get("module")
        .and_then(|m| m.get("id"))
        .and_then(toml::Value::as_str)
        .map(str::to_string))
}

/// The folders of the path dependencies of the crate in `directory`, its
/// own or its workspace's.
pub(crate) fn dependencies(directory: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let manifest = |path: &Path| -> anyhow::Result<toml::Table> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("{}", path.display()))
    };
    let path = directory.join("Cargo.toml");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let own = manifest(&path)?;
    let workspace = directory.ancestors().skip(1).find_map(|p| {
        let table = manifest(&p.join("Cargo.toml")).ok()?;
        table
            .contains_key("workspace")
            .then(|| (p.to_path_buf(), table))
    });
    let mut folders = Vec::new();
    let Some(listed) = own.get("dependencies").and_then(toml::Value::as_table) else {
        return Ok(folders);
    };
    for (name, dependency) in listed {
        if let Some(path) = dependency.get("path").and_then(toml::Value::as_str) {
            folders.push(directory.join(path));
        } else if dependency.get("workspace").and_then(toml::Value::as_bool) == Some(true)
            && let Some((root, table)) = &workspace
            && let Some(path) = table
                .get("workspace")
                .and_then(|w| w.get("dependencies"))
                .and_then(|d| d.get(name))
                .and_then(|d| d.get("path"))
                .and_then(toml::Value::as_str)
        {
            folders.push(root.join(path));
        }
    }
    Ok(folders)
}

/// `file` with each action it implements copied in from its contract,
/// renamed into `namespace` as the catalog renames it: the contract's own
/// resource becomes the implementer's. What each copied action diagnoses
/// stays the contract's, so one kind of name has one diagnosis: the IDs it
/// copied in, which diagnose nothing.
pub(crate) fn import(
    file: &mut ActionsFile,
    namespace: &str,
    contracts: &BTreeMap<String, String>,
) -> anyhow::Result<Vec<ActionId>> {
    let mut imported = Vec::new();
    for (contract, ids) in &file.implements {
        let text = contracts.get(contract).with_context(|| {
            format!(
                "module {namespace} implements {contract}, whose actions.toml isn't found: its Cargo.toml depends on the contract's crate"
            )
        })?;
        let declared = link_actions::read_actions(text)
            .with_context(|| format!("contract {contract}'s actions.toml"))?;
        for id in ids {
            let action = declared.actions.get(id).with_context(|| {
                format!(
                    "module {namespace} implements {id}, which contract {contract} doesn't declare"
                )
            })?;
            let resource = match id.resource() == contract {
                true => namespace,
                false => id.resource(),
            };
            let mine = ActionId::new(namespace, resource, id.verb())?;
            anyhow::ensure!(
                !file.actions.contains_key(&mine),
                "module {namespace} declares {mine}, which is contract {contract}'s {id}"
            );
            // Its verb, where it's the contract's own, comes with it, so
            // its effect does too.
            if let Some(verb) = declared.verbs.get(id.verb()) {
                file.verbs
                    .entry(id.verb().to_string())
                    .or_insert_with(|| verb.clone());
            }
            let mut action = action.clone();
            action.diagnoses = None;
            imported.push(mine.clone());
            file.actions.insert(mine, action);
        }
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use link_test_kit::TempDir;

    /// A workspace holding the declarations fixture's store contract and
    /// an implementer of it that declares nothing of its own.
    fn workspace() -> TempDir {
        let root = TempDir::new("implements");
        let contract = root.path().join("contract");
        let store = root.path().join("store");
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/declarations/contract");
        let write = |path: std::path::PathBuf, text: &str| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            root.path().join("Cargo.toml"),
            "[workspace]\n[workspace.dependencies.store-contract]\npath = \"contract\"\n",
        );
        for file in ["module.toml", "actions.toml", "types.toml"] {
            write(
                contract.join(file),
                &std::fs::read_to_string(fixture.join(file)).unwrap(),
            );
        }
        write(
            store.join("Cargo.toml"),
            "[package]\nname = \"store\"\n[dependencies.store-contract]\nworkspace = true\n",
        );
        write(
            store.join("module.toml"),
            "schema = 1\n[module]\nid = \"memory-store\"\nkind = \"module\"\n",
        );
        write(
            store.join("actions.toml"),
            "schema = 1\n[implements]\nstore = [\"store.store.serve\", \"store.record.show\"]\n",
        );
        root
    }

    #[test]
    fn an_implementer_lists_its_contracts_actions_as_its_commands() {
        let root = workspace();
        let actions = super::super::read_module(&root.path().join("store"), None).unwrap();
        let commands: Vec<&String> = actions.keys().collect();
        assert_eq!(
            commands,
            [
                "memory-store",
                "memory-store record",
                "memory-store record show",
                "memory-store serve"
            ],
            "the contract's own resource is the implementer's, and record delete isn't implemented"
        );
        // A contract's resource is said as the contract describes it.
        assert_eq!(actions["memory-store record"].help, "One stored record");
        // A contract's show, copied in, diagnoses nothing of its own.
        assert_eq!(actions["memory-store record show"].diagnoses, None);
    }

    #[test]
    fn a_contract_its_crate_doesnt_depend_on_is_refused_by_name() {
        let root = workspace();
        std::fs::write(
            root.path().join("store/Cargo.toml"),
            "[package]\nname = \"store\"\n",
        )
        .unwrap();
        let error = super::super::read_module(&root.path().join("store"), None).unwrap_err();
        assert!(
            format!("{error:#}").contains("implements store, whose actions.toml isn't found"),
            "{error:#}"
        );
    }
}
