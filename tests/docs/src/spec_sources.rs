//! Registered spec source locations paired with their owning typed schemas:
//! a `link_spec::specification!` registration in Rust, or a version 1
//! module's `types.toml` shape with `x-link-spec`. Unsupported
//! registrations and missing typed catalogues stay explicit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use quote::ToTokens;
use serde_json::{Value, json};
use syn::parse::{Parse, ParseStream};
mod inputs;
mod metadata;
mod modules;
pub use metadata::{Facts as FieldFacts, Presence};

/// One configurable field, including fields in records, variants, and maps.
#[derive(Clone, Debug)]
pub struct SpecField {
    /// Repository-relative member folder and registered Rust type.
    pub owner: String,
    pub path: String,
    /// Source location, excluded from the reviewed-content fingerprint.
    pub source: PathBuf,
    /// Canonical contract data, without whitespace or source line numbers.
    pub content: Value,
    pub branch: String,
    pub facts: FieldFacts,
}

#[derive(Default, Debug)]
pub struct Discovery {
    pub fields: Vec<SpecField>,
    pub problems: Vec<String>,
}

struct Registration {
    ty: syn::Type,
    schema: syn::LitStr,
    check: Option<syn::Expr>,
}
impl Parse for Registration {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let ty = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let source: syn::ExprMacro = input.parse()?;
        if !source.mac.path.is_ident("include_str") {
            return Err(input.error("a registered spec needs include_str!(schema)"));
        }
        let schema = source.mac.parse_body()?;
        let mut check = None;
        if !input.is_empty() {
            input.parse::<syn::Token![,]>()?;
            if !input.is_empty() {
                let name: syn::Ident = input.parse()?;
                if name != "check" {
                    return Err(input.error("unsupported spec registration option"));
                }
                input.parse::<syn::Token![=]>()?;
                check = Some(input.parse()?);
                if !input.is_empty() {
                    input.parse::<syn::Token![,]>()?;
                }
            }
        }
        Ok(Self { ty, schema, check })
    }
}

fn joined(parent: &Path, relative: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for component in parent.join(relative).components() {
        match component {
            Component::Normal(name) => path.push(name),
            Component::CurDir => {}
            Component::ParentDir if path.pop() => {}
            _ => return None,
        }
    }
    Some(path)
}

fn member(files: &BTreeMap<PathBuf, String>, source: &Path) -> Option<PathBuf> {
    source
        .ancestors()
        .skip(1)
        .find(|folder| files.contains_key(&folder.join("Cargo.toml")))
        .map(Path::to_path_buf)
}

fn items<'a>(items: &'a [syn::Item], out: &mut Vec<&'a syn::Item>) {
    for item in items {
        let attrs: &[syn::Attribute] = match item {
            syn::Item::Mod(item) => &item.attrs,
            syn::Item::Macro(item) => &item.attrs,
            syn::Item::Fn(item) => &item.attrs,
            syn::Item::Struct(item) => &item.attrs,
            syn::Item::Enum(item) => &item.attrs,
            syn::Item::Impl(item) => &item.attrs,
            _ => &[],
        };
        if crate::rust_source::test_only(attrs) {
            continue;
        }
        out.push(item);
        if let syn::Item::Mod(module) = item
            && let Some((_, nested)) = &module.content
        {
            self::items(nested, out);
        }
    }
}

fn imports(tree: &syn::UseTree, prefix: &str, aliases: &mut BTreeMap<String, String>) {
    let qualify = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}::{name}")
        }
    };
    match tree {
        syn::UseTree::Path(path) => imports(&path.tree, &qualify(&path.ident.to_string()), aliases),
        syn::UseTree::Name(name) => {
            aliases.insert(name.ident.to_string(), qualify(&name.ident.to_string()));
        }
        syn::UseTree::Rename(name) => {
            aliases.insert(name.rename.to_string(), qualify(&name.ident.to_string()));
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                imports(tree, prefix, aliases);
            }
        }
        _ => {}
    }
}

fn production_sources(
    files: &BTreeMap<PathBuf, String>,
    problems: &mut Vec<String>,
) -> BTreeMap<PathBuf, syn::File> {
    let mut parsed = BTreeMap::new();
    for (path, text) in files.iter().filter(|(path, _)| {
        inputs::source_input(path) && path.extension().is_some_and(|ext| ext == "rs")
    }) {
        match syn::parse_file(text) {
            Ok(file) => {
                parsed.insert(path.clone(), file);
            }
            Err(error) => problems.push(format!("{}: Rust source: {error}", path.display())),
        }
    }
    let production = modules::production(&parsed);
    parsed.retain(|source, _| production.contains(source));
    parsed
}

/// Source registrations are discovered independently of the typed catalogue.
/// Callers supply tracked repository-relative files and exported kinds from
/// each selected workspace's actual verification catalogue. A registration
/// without a supplied kind, or a supplied kind without source, is a problem.
pub fn discover(files: &BTreeMap<PathBuf, String>, kinds: &[link_spec::Exported]) -> Discovery {
    let mut out = Discovery::default();
    let parsed = production_sources(files, &mut out.problems);
    let mut seen = BTreeSet::new();
    for (source, file) in &parsed {
        if source.components().any(|part| part.as_os_str() == "tests") {
            continue;
        }
        let mut flattened = Vec::new();
        items(&file.items, &mut flattened);
        let mut aliases = BTreeMap::new();
        for item in &flattened {
            if let syn::Item::Use(item) = item {
                imports(&item.tree, "", &mut aliases);
            }
        }
        for item in flattened {
            let syn::Item::Macro(item) = item else {
                continue;
            };
            let path = &item.mac.path;
            let mut name = path
                .segments
                .iter()
                .map(|part| part.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            if path.segments.len() == 1
                && let Some(target) = aliases.get(&name)
            {
                name = target.clone();
            }
            if name == "link_files::specification" {
                continue;
            }
            if name == "crate::specification"
                && member(files, source)
                    .and_then(|folder| {
                        files
                            .get(&folder.join("Cargo.toml"))
                            .and_then(|text| toml::from_str::<toml::Value>(text).ok())
                    })
                    .and_then(|manifest| {
                        manifest
                            .get("package")?
                            .get("name")?
                            .as_str()
                            .map(str::to_owned)
                    })
                    .as_deref()
                    == Some("link-files")
            {
                continue;
            }
            if name != "link_spec::specification" {
                if name.ends_with("::specification") || name == "specification" {
                    out.problems.push(format!(
                        "{}: unsupported registration macro {name}",
                        source.display()
                    ));
                }
                continue;
            }
            let result = registration(files, &parsed, source, &item.mac, kinds, &mut seen);
            match result {
                Ok(fields) => out.fields.extend(fields),
                Err(error) => out.problems.push(format!("{}: {error}", source.display())),
            }
        }
    }
    declared_kinds(files, &parsed, kinds, &mut seen, &mut out);
    for kind in kinds {
        if !seen.contains(&kind.kind) {
            out.problems.push(format!(
                "{}: typed kind has no source registration",
                kind.kind
            ));
        }
    }
    out.fields
        .sort_by(|a, b| (&a.owner, &a.path).cmp(&(&b.owner, &b.path)));
    out.problems.sort();
    out
}

fn registration(
    files: &BTreeMap<PathBuf, String>,
    parsed: &BTreeMap<PathBuf, syn::File>,
    source: &Path,
    mac: &syn::Macro,
    kinds: &[link_spec::Exported],
    seen: &mut BTreeSet<String>,
) -> Result<Vec<SpecField>, String> {
    let registration: Registration = mac
        .parse_body()
        .map_err(|e| format!("spec registration: {e}"))?;
    let schema = joined(
        source.parent().unwrap_or(Path::new("")),
        &registration.schema.value(),
    )
    .ok_or("spec schema escapes the repository")?;
    // A schema beside its module's `types.toml` is a version 1 kind, which
    // its declaration registers ([`declared_kinds`]); a reader that still
    // names it is no second source.
    if declares(files, &schema) {
        return Ok(Vec::new());
    }
    let ty = registration.ty.to_token_stream().to_string();
    paired(
        files,
        parsed,
        source,
        &schema,
        &ty,
        registration.check.as_ref(),
        kinds,
        seen,
    )
}

/// Whether the `types.toml` beside `schema` declares the kind that
/// schema versions: a version 1 kind, its declaration its registration.
fn declares(files: &BTreeMap<PathBuf, String>, schema: &Path) -> bool {
    let Some(folder) = schema.parent() else {
        return false;
    };
    let Some(kind) = files
        .get(schema)
        .and_then(|text| link_spec::schema::Format::parse(text).ok())
        .map(|format| format.kind)
    else {
        return false;
    };
    let Some(id) = files
        .get(&folder.join("module.toml"))
        .and_then(|text| toml::from_str::<toml::Value>(text).ok())
        .and_then(|module| Some(module.get("module")?.get("id")?.as_str()?.to_owned()))
    else {
        return false;
    };
    files
        .get(&folder.join("types.toml"))
        .and_then(|text| link_actions::read_types(text, &id).ok())
        .is_some_and(|shapes| {
            shapes
                .into_values()
                .any(|shape| shape.spec.as_deref() == Some(kind.as_str()))
        })
}

/// Each spec kind a version 1 module declares in its `types.toml` (a
/// shape with `x-link-spec`), registered by that declaration: its versions
/// in its `schema.toml` or `<kind>-schema.toml` beside it, its Rust type
/// generated from it.
fn declared_kinds(
    files: &BTreeMap<PathBuf, String>,
    parsed: &BTreeMap<PathBuf, syn::File>,
    kinds: &[link_spec::Exported],
    seen: &mut BTreeSet<String>,
    out: &mut Discovery,
) {
    for (source, text) in files {
        if source.file_name().and_then(|n| n.to_str()) != Some("types.toml")
            || source
                .components()
                .any(|part| matches!(part.as_os_str().to_str(), Some("tests" | "fixtures")))
        {
            continue;
        }
        let folder = source.parent().unwrap_or(Path::new(""));
        let Some(id) = files
            .get(&folder.join("module.toml"))
            .and_then(|text| toml::from_str::<toml::Value>(text).ok())
            .and_then(|module| Some(module.get("module")?.get("id")?.as_str()?.to_owned()))
        else {
            continue;
        };
        let shapes = match link_actions::read_types(text, &id) {
            Ok(shapes) => shapes,
            Err(error) => {
                out.problems
                    .push(format!("{}: {error:#}", source.display()));
                continue;
            }
        };
        for (name, shape) in shapes {
            let Some(kind) = shape.spec else { continue };
            let schema = folder.join(link_actions::schema_file(&id, &kind));
            match paired(files, parsed, source, &schema, &name, None, kinds, seen) {
                Ok(fields) => out.fields.extend(fields),
                Err(error) => out.problems.push(format!("{}: {error}", source.display())),
            }
        }
    }
}

/// A registered kind, declared at `source` with its versions in `schema`
/// and its type named `ty`, paired with the typed kind it exports: each of
/// its fields, or why they don't pair.
#[allow(
    clippy::too_many_arguments,
    reason = "each is one fact of the registration"
)]
fn paired(
    files: &BTreeMap<PathBuf, String>,
    parsed: &BTreeMap<PathBuf, syn::File>,
    source: &Path,
    schema: &Path,
    ty: &str,
    check: Option<&syn::Expr>,
    kinds: &[link_spec::Exported],
    seen: &mut BTreeSet<String>,
) -> Result<Vec<SpecField>, String> {
    let schema = schema.to_path_buf();
    let format = link_spec::schema::Format::parse(
        files
            .get(&schema)
            .ok_or_else(|| format!("{}: registered schema is not tracked", schema.display()))?,
    )
    .map_err(|e| format!("{}: {e}", schema.display()))?;
    if !seen.insert(format.kind.clone()) {
        return Err(format!("{}: duplicate source registration", format.kind));
    }
    let matching: Vec<_> = kinds
        .iter()
        .filter(|kind| kind.kind == format.kind)
        .collect();
    let [kind] = matching.as_slice() else {
        return Err(format!(
            "{}: expected one typed kind, found {}",
            format.kind,
            matching.len()
        ));
    };
    if kind.schema != format.current
        || kind.command != format.command
        || kind.steps != format.steps
        || kind.oldest != format.oldest
        || kind.about != format.about
    {
        return Err(format!(
            "{}: typed kind differs from its source schema",
            format.kind
        ));
    }
    let member = member(files, source).ok_or("registered spec has no tracked Cargo member")?;
    let owner = format!("{}::{ty}", member.display());
    let validation = validation(parsed, &member, check);
    let declared = link_spec::declared(&kind.shape)?;
    let (mut metadata, metadata_problems) = metadata::fields(kind, &declared);
    if !metadata_problems.is_empty() {
        return Err(format!("{owner}: {}", metadata_problems.join("; ")));
    }
    let mut fields = Vec::new();
    for (table, keys) in &declared.fields {
        for key in keys {
            let path = if table.is_empty() {
                key.clone()
            } else {
                format!("{table}.{key}")
            };
            let nodes: Vec<_> = declared
                .nodes
                .get(&path)
                .into_iter()
                .flatten()
                .map(|node| expanded(node, &kind.shape, &mut BTreeSet::new()))
                .collect();
            let nodes = if nodes.is_empty() {
                // Recursive types list their fields once without descending
                // again. Keep their resolved parent rather than an empty
                // field contract that would conceal a later type change.
                declared.nodes.get(table).into_iter().flatten()
                    .map(|node| json!({"recursive_parent":expanded(node,&kind.shape,&mut BTreeSet::new()),"field":key})).collect()
            } else {
                nodes
            };
            for metadata in metadata
                .remove(&path)
                .expect("metadata covers declared fields")
            {
                let content = json!({"kind":kind.kind,"schema":kind.schema,"command":kind.command,
                    "history":kind.steps,"type":ty,"field":path,"nodes":if metadata.facts.presence==Presence::Unknown {&nodes}else{&metadata.nodes},"validation":validation,
                    "branch":metadata.branch,"facts":metadata.facts});
                fields.push(SpecField {
                    owner: owner.clone(),
                    path: path.clone(),
                    source: source.to_path_buf(),
                    content,
                    branch: metadata.branch,
                    facts: metadata.facts,
                });
            }
        }
    }
    Ok(fields)
}

fn expanded(node: &Value, root: &Value, seen: &mut BTreeSet<String>) -> Value {
    match node {
        Value::Array(values) => {
            Value::Array(values.iter().map(|v| expanded(v, root, seen)).collect())
        }
        Value::Object(map) => {
            let mut map = map.clone();
            if let Some(reference) = map.get("$ref").and_then(Value::as_str).map(str::to_owned)
                && seen.insert(reference.clone())
            {
                let target = if reference == "#" {
                    Some(root)
                } else {
                    reference
                        .strip_prefix("#/$defs/")
                        .and_then(|name| root.get("$defs").and_then(|defs| defs.get(name)))
                };
                if let Some(target) = target {
                    map.insert("resolved".into(), expanded(target, root, seen));
                }
                seen.remove(&reference);
            }
            Value::Object(
                map.into_iter()
                    .map(|(k, v)| {
                        let v = if k == "resolved" {
                            v
                        } else {
                            expanded(&v, root, seen)
                        };
                        (k, v)
                    })
                    .collect(),
            )
        }
        value => value.clone(),
    }
}

fn validation(
    parsed: &BTreeMap<PathBuf, syn::File>,
    member: &Path,
    check: Option<&syn::Expr>,
) -> BTreeMap<String, String> {
    let Some(check) = check else {
        return BTreeMap::new();
    };
    let mut candidates: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (_, file) in parsed.iter().filter(|(path, _)| path.starts_with(member)) {
        let mut flat = Vec::new();
        items(&file.items, &mut flat);
        for item in flat {
            let pair = match item {
                syn::Item::Fn(item) => Some((
                    item.sig.ident.to_string(),
                    item.to_token_stream().to_string(),
                )),
                syn::Item::Const(item) => {
                    Some((item.ident.to_string(), item.to_token_stream().to_string()))
                }
                _ => None,
            };
            if let Some((name, tokens)) = pair {
                candidates.entry(name).or_default().push(tokens);
            }
            if let syn::Item::Impl(item) = item {
                for item in &item.items {
                    if let syn::ImplItem::Fn(item) = item
                        && !crate::rust_source::test_only(&item.attrs)
                    {
                        candidates
                            .entry(item.sig.ident.to_string())
                            .or_default()
                            .push(item.to_token_stream().to_string());
                    }
                }
            }
        }
    }
    let mut selected =
        BTreeMap::from([("registration".into(), check.to_token_stream().to_string())]);
    loop {
        let words: BTreeSet<_> = selected
            .values()
            .flat_map(|tokens| tokens.split(|c: char| !c.is_alphanumeric() && c != '_'))
            .map(str::to_owned)
            .collect();
        let mut changed = false;
        for name in words {
            if !selected.contains_key(&name)
                && let Some(tokens) = candidates.get(&name)
            {
                selected.insert(name, tokens.join("\n"));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    selected
}

#[cfg(test)]
mod tests;
