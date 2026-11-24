//! The API's types, made at every build from its spec,
//! `docs/cloud/api/openapi.yaml`, by typify: every schema in its
//! `components`, each object closed as the spec says. Two rules are
//! Link's own, applied before and after typify:
//!
//! - A schema marked `x-secret: true` (a device code) is
//!   a `link_secretive::Secret`: never printed, zeroed when dropped.
//! - A time, `format: date-time`, is a `jiff::Timestamp`, read and
//!   written as RFC 3339.
//! - A type holding a secret, itself or in a field of a type that does,
//!   isn't `Clone` (a secret is duplicated by name, never copied by
//!   accident), and writes its secrets as the bare strings they are
//!   (`link_secretive::bare`).
//!
//! - A scope's grants, the spec's `Scope` `x-grants`, are `Scope::grants`,
//!   written from that table alone.
//!
//! And typify reads JSON Schema as draft 7 does, so a `const` is written
//! as the one-value `enum` it means, and OpenAPI's `discriminator`, which
//! only says what each `oneOf`'s own `kind` already does, is left out. A
//! `pattern` is ECMAScript's, as JSON Schema's are, in its Unicode mode
//! (`u`), which `\p{L}` (any letter, an installation's name) needs.
//! typify checks no integer's `minimum` or `maximum` beyond the type it
//! picks, so a caller holds a number that matters to its bounds itself
//! (a login's timings, in the accounts module).

use std::collections::BTreeSet;

use quote::ToTokens;
use serde_json::{Value, json};
use syn::punctuated::Punctuated;

/// The spec, from this crate's folder.
const SPEC: &str = "schemas/account.yaml";
const REMOTE: &str = "schemas/remote.yaml";

/// The type a secret is held in.
const SECRET: &str = "link_secretive::Secret";

/// The type a time (`format: date-time`) is held in: an instant, read
/// and written as RFC 3339.
const TIME: &str = "jiff::Timestamp";

/// The spec's schemas defined in a crate of their own, for the crates
/// that name them without this API: each schema's name and its type.
/// `jwt.rs`'s test holds that crate's grammar to the spec's.
const DEFINED: &[(&str, &str)] = &[
    ("Principal", "link_principal::Principal"),
    ("PrincipalKind", "link_principal::PrincipalKind"),
];

fn main() {
    println!("cargo::rerun-if-changed={SPEC}");
    let text = std::fs::read_to_string(SPEC).unwrap_or_else(|e| panic!("reading {SPEC}: {e}"));
    let spec: Value = serde_norway::from_str(&text).unwrap_or_else(|e| panic!("{SPEC}: {e}"));
    let mut schemas = spec["components"]["schemas"].clone();
    assert!(schemas.is_object(), "{SPEC} has no components.schemas");
    println!("cargo::rerun-if-changed={REMOTE}");
    let remote: Value =
        serde_norway::from_str(&std::fs::read_to_string(REMOTE).expect("remote spec"))
            .expect("remote spec is YAML");
    for (name, schema) in remote["components"]["schemas"]
        .as_object()
        .expect("remote schemas")
    {
        assert!(schemas.get(name).is_none(), "duplicate schema {name}");
        schemas
            .as_object_mut()
            .unwrap()
            .insert(name.clone(), schema.clone());
    }
    let grants = scope_grants(&schemas["Scope"]);
    for (name, path) in DEFINED {
        let schema = schemas
            .get_mut(*name)
            .unwrap_or_else(|| panic!("{SPEC} has no {name}"));
        *schema = json!({
            "x-rust-type": { "crate": "link-principal", "version": "*", "path": path }
        });
    }
    prepare(&mut schemas);
    let root = serde_json::from_value(json!({ "definitions": schemas }))
        .unwrap_or_else(|e| panic!("{SPEC}'s schemas aren't JSON Schema: {e}"));

    let mut settings = typify::TypeSpaceSettings::default();
    settings
        .with_struct_builder(false)
        .with_crate("link-secretive", typify::CrateVers::Any, None)
        .with_crate("jiff", typify::CrateVers::Any, None)
        .with_crate("link-principal", typify::CrateVers::Any, None);
    let mut space = typify::TypeSpace::new(&settings);
    space
        .add_root_schema(root)
        .unwrap_or_else(|e| panic!("typify on {SPEC}: {e}"));
    let mut file: syn::File = syn::parse2(space.to_stream()).expect("typify's output is Rust");
    hold_secrets(&mut file);
    syn::visit_mut::visit_file_mut(&mut Unicode, &mut file);

    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(out.join("types.rs"), prettyplease::unparse(&file) + &grants)
        .expect("writing types.rs");
}

/// `Scope::grants`, from `x-grants`: one entry for each of the enum's
/// scopes, and no other.
fn scope_grants(scope: &Value) -> String {
    let named: BTreeSet<&str> = scope["enum"]
        .as_array()
        .expect("Scope's enum")
        .iter()
        .map(|s| s.as_str().expect("a scope"))
        .collect();
    let grants = scope["x-grants"]
        .as_object()
        .expect("Scope's x-grants: the grants each scope is given");
    let given: BTreeSet<&str> = grants.keys().map(String::as_str).collect();
    assert_eq!(
        named, given,
        "Scope's x-grants names each of its scopes once"
    );
    let rows: String = grants
        .iter()
        .map(|(scope, grants)| {
            let grants = grants.as_str().expect("a scope's grants, a string");
            format!("    ({scope:?}, {grants:?}),\n")
        })
        .collect();
    format!(
        "\n/// Each scope's grants, as the spec's `Scope` `x-grants` says them.\n\
         const SCOPE_GRANTS: &[(&str, &str)] = &[\n{rows}];\n\n\
         impl Scope {{\n\
         \x20   /// The grants a token asked for as this is given (the spec's\n\
         \x20   /// `x-grants`): what the account service writes for an installation's\n\
         \x20   /// assertion, which still names one of these.\n\
         \x20   pub fn grants(self) -> Grants {{\n\
         \x20       let scope = self.to_string();\n\
         \x20       let (_, grants) = SCOPE_GRANTS\n\
         \x20           .iter()\n\
         \x20           .find(|(named, _)| *named == scope)\n\
         \x20           .expect(\"x-grants names every scope\");\n\
         \x20       grants.parse().expect(\"the spec's grants\")\n\
         \x20   }}\n\
         }}\n"
    )
}

/// The spec's schemas as typify reads them: references to
/// `#/definitions/`, a `const` as its one-value `enum`, no
/// `discriminator`, a schema marked `x-secret` a [`SECRET`] and a
/// `date-time` a [`TIME`].
fn prepare(value: &mut Value) {
    match value {
        Value::Object(schema) => {
            let held = if schema.get("x-secret") == Some(&Value::Bool(true)) {
                Some(("link-secretive", SECRET))
            } else if schema.get("format") == Some(&json!("date-time")) {
                Some(("jiff", TIME))
            } else {
                None
            };
            if let Some((owner, path)) = held {
                let description = schema.get("description").cloned();
                schema.clear();
                schema.insert(
                    "x-rust-type".into(),
                    json!({ "crate": owner, "version": "*", "path": path }),
                );
                if let Some(description) = description {
                    schema.insert("description".into(), description);
                }
                return;
            }
            if let Some(Value::String(to)) = schema.get_mut("$ref") {
                *to = to.replace("account.yaml", "");
                *to = to.replace("#/components/schemas/", "#/definitions/");
            }
            schema.remove("discriminator");
            if let Some(one) = schema.remove("const") {
                schema.insert("enum".into(), Value::Array(vec![one]));
            }
            schema.values_mut().for_each(prepare);
        }
        Value::Array(values) => values.iter_mut().for_each(prepare),
        _ => {}
    }
}

/// Each `pattern`'s regex in ECMAScript's Unicode mode:
/// `regress::Regex::new(p)` made `regress::Regex::with_flags(p, "u")`.
struct Unicode;

impl syn::visit_mut::VisitMut for Unicode {
    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        syn::visit_mut::visit_expr_call_mut(self, call);
        let func = call.func.to_token_stream().to_string().replace(' ', "");
        if func == "::regress::Regex::new" && call.args.len() == 1 {
            call.func = syn::parse_quote!(::regress::Regex::with_flags);
            call.args.push(syn::parse_quote!("u"));
        }
    }
}

/// Every type holding a secret made not `Clone`, and each of its secret
/// fields written bare.
fn hold_secrets(file: &mut syn::File) {
    let holding = holding_secrets(file);
    for item in &mut file.items {
        let (name, attrs, fields): (_, _, Vec<&mut syn::Field>) = match item {
            syn::Item::Struct(s) => (
                s.ident.to_string(),
                &mut s.attrs,
                s.fields.iter_mut().collect(),
            ),
            syn::Item::Enum(e) => (
                e.ident.to_string(),
                &mut e.attrs,
                e.variants
                    .iter_mut()
                    .flat_map(|v| v.fields.iter_mut())
                    .collect(),
            ),
            _ => continue,
        };
        if !holding.contains(&name) {
            continue;
        }
        for attr in attrs.iter_mut().filter(|a| a.path().is_ident("derive")) {
            let derives = attr
                .parse_args_with(Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
                .expect("a derive lists paths");
            let kept: Punctuated<syn::Path, syn::Token![,]> = derives
                .into_iter()
                .filter(|path| !path.is_ident("Clone"))
                .collect();
            *attr = syn::parse_quote!(#[derive(#kept)]);
        }
        for field in fields {
            let ty = field.ty.to_token_stream().to_string().replace(' ', "");
            if ty == format!("::{SECRET}") {
                field
                    .attrs
                    .push(syn::parse_quote!(#[serde(with = "::link_secretive::bare")]));
            } else {
                assert!(
                    !ty.contains("Secret<") && !ty.ends_with("::Secret>"),
                    "{name} holds a secret inside {ty}: a secret is a field of its own"
                );
            }
        }
    }
}

/// The names of the types holding a secret: in a field, or in a field of
/// a type that holds one, to any depth.
fn holding_secrets(file: &syn::File) -> BTreeSet<String> {
    let mut holding = BTreeSet::from(["Secret".to_string()]);
    loop {
        let before = holding.len();
        for item in &file.items {
            let (name, types): (_, Vec<&syn::Type>) = match item {
                syn::Item::Struct(s) => (&s.ident, s.fields.iter().map(|f| &f.ty).collect()),
                syn::Item::Enum(e) => (
                    &e.ident,
                    e.variants
                        .iter()
                        .flat_map(|v| v.fields.iter().map(|f| &f.ty))
                        .collect(),
                ),
                _ => continue,
            };
            let holds = types.iter().any(|ty| {
                ty.to_token_stream()
                    .to_string()
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .any(|word| holding.contains(word))
            });
            if holds {
                holding.insert(name.to_string());
            }
        }
        if holding.len() == before {
            return holding;
        }
    }
}
