//! Production reachability excludes fixtures, dev-only libraries and build-only inputs.

use link_docs_check::{Finding, Tree, fail};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub struct Source {
    pub path: PathBuf,
    pub owner: PathBuf,
    pub module: String,
    pub file: syn::File,
}

fn dependency_names(value: &toml::Value, kind: &str, out: &mut BTreeSet<String>) {
    if let Some(table) = value.get(kind).and_then(toml::Value::as_table) {
        for (name, declaration) in table {
            out.insert(
                declaration
                    .get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(name)
                    .to_owned(),
            );
        }
    }
    if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            dependency_names(target, kind, out);
        }
    }
}

struct TestModules {
    base: PathBuf,
    excluded: Vec<PathBuf>,
}
impl<'ast> syn::visit::Visit<'ast> for TestModules {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let old = self.base.clone();
        let at = self.base.join(item.ident.to_string());
        if super::collect::tested(&item.attrs) {
            self.excluded.extend([at.with_extension("rs"), at]);
            for attr in &item.attrs {
                if attr.path().is_ident("path")
                    && let syn::Meta::NameValue(value) = &attr.meta
                    && let syn::Expr::Lit(value) = &value.value
                    && let syn::Lit::Str(value) = &value.lit
                {
                    self.excluded.push(self.base.join(value.value()));
                }
            }
        } else {
            self.base = at;
            syn::visit::visit_item_mod(self, item);
            self.base = old;
        }
    }
}

pub(super) fn sources(tree: &Tree) -> (Vec<Source>, Vec<Finding>) {
    let mut members = BTreeMap::new();
    let mut runtime = BTreeSet::new();
    for (file, text) in &tree.files {
        if file.file_name().is_none_or(|n| n != "Cargo.toml") {
            continue;
        }
        let Ok(value) = toml::from_str::<toml::Value>(text) else {
            continue;
        };
        let Some(package) = value.get("package") else {
            continue;
        };
        let Some(name) = package.get("name").and_then(toml::Value::as_str) else {
            continue;
        };
        let owner = file.parent().unwrap();
        if package
            .get("metadata")
            .and_then(|m| m.get("link"))
            .and_then(|m| m.get("role"))
            .and_then(toml::Value::as_str)
            == Some("fixture")
        {
            continue;
        }
        let mut deps = BTreeSet::new();
        dependency_names(&value, "dependencies", &mut deps);
        if tree.files.contains_key(&owner.join("src/main.rs")) || value.get("bin").is_some() {
            runtime.insert(name.to_owned());
        }
        members.insert(owner.to_path_buf(), (name.to_owned(), deps));
    }
    loop {
        let before = runtime.len();
        for (name, deps) in members.values() {
            if runtime.contains(name) {
                runtime.extend(deps.iter().cloned());
            }
        }
        if before == runtime.len() {
            break;
        }
    }
    let mut result = Vec::new();
    let mut errors = Vec::new();
    for (file, text) in &tree.files {
        if file.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let Some((owner, (name, _))) = members
            .iter()
            .filter(|(owner, _)| file.starts_with(owner))
            .max_by_key(|(owner, _)| owner.components().count())
        else {
            continue;
        };
        if !runtime.contains(name) {
            continue;
        }
        let Ok(relative) = file.strip_prefix(owner.join("src")) else {
            continue;
        };
        if relative
            .components()
            .any(|p| ["tests", "tests.rs"].contains(&p.as_os_str().to_string_lossy().as_ref()))
        {
            continue;
        }
        let mut module = name.replace('-', "_");
        for part in relative.with_extension("").components() {
            let part = part.as_os_str().to_string_lossy();
            if !["lib", "main", "mod"].contains(&part.as_ref()) {
                module += &format!("::{part}");
            }
        }
        match syn::parse_file(text) {
            Ok(parsed) if !super::collect::tested(&parsed.attrs) => result.push(Source {
                path: file.clone(),
                owner: owner.clone(),
                module,
                file: parsed,
            }),
            Ok(_) => {}
            Err(error) => errors.push(fail(format!(
                "{}: cannot audit environment reads: {error}",
                tree.shown(file)
            ))),
        }
    }
    let mut excluded = Vec::new();
    use syn::visit::Visit;
    for source in &result {
        let parent = source.path.parent().unwrap();
        let stem = source.path.file_stem().unwrap().to_string_lossy();
        let base = if ["main", "lib", "mod"].contains(&stem.as_ref()) {
            parent.to_owned()
        } else {
            parent.join(stem.as_ref())
        };
        let mut modules = TestModules {
            base,
            excluded: vec![],
        };
        modules.visit_file(&source.file);
        excluded.extend(modules.excluded);
    }
    result.retain(|source| {
        !excluded
            .iter()
            .any(|path| source.path == *path || source.path.starts_with(path))
    });
    (result, errors)
}
