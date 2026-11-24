//! Each spec kind's type made a spec: its versions and steps from
//! `schema.toml`, its JSON Schema from the catalog, and its checks, so
//! `link-spec` reads it from a file or its field flags alike, prints it
//! and shows it.

use std::fmt::Write as _;

use anyhow::Context;
use link_actions::{Catalog, Module, TypeName};

use crate::names::pascal;

pub(crate) fn write(catalog: &Catalog, module: &Module) -> anyhow::Result<String> {
    let mut out = String::new();
    for (name, declared) in &module.types {
        let Some(kind) = &declared.spec else { continue };
        let versions = module.schemas.get(kind).with_context(|| {
            format!(
                "spec kind {kind} has no {} beside its types.toml",
                link_actions::schema_file(&module.identity.id, kind)
            )
        })?;
        let schema_path = versions
            .path
            .to_str()
            .context("the module's folder is text")?;
        let json = serde_json::to_string(
            &catalog.json_schema(&TypeName::new(&module.identity.id, name)?)?,
        )?;
        let rust = pascal(name);
        let _ = write!(
            out,
            "impl ::link_files::schema::Specification for {rust} {{\n    \
             fn format() -> &'static ::link_files::schema::Format {{\n        \
             static FORMAT: ::std::sync::LazyLock<::link_files::schema::Format> = ::std::sync::LazyLock::new(|| {{\n            \
             ::link_files::schema::Format::parse(include_str!({schema_path:?}))\n                \
             .map(::link_files::schema::Format::named)\n                \
             .expect(\"the schema.toml checked when this was generated\")\n        }});\n        &FORMAT\n    }}\n}}\n\n\
             impl ::schemars::JsonSchema for {rust} {{\n    \
             fn schema_name() -> ::std::borrow::Cow<'static, str> {{\n        {rust:?}.into()\n    }}\n    \
             fn inline_schema() -> bool {{\n        true\n    }}\n    \
             fn json_schema(_: &mut ::schemars::SchemaGenerator) -> ::schemars::Schema {{\n        \
             ::serde_json::from_str({json:?}).expect(\"the catalog's JSON Schema\")\n    }}\n}}\n\n\
             impl ::link_spec::Kind for {rust} {{\n    \
             fn check(&self) -> ::link_spec::__anyhow::Result<()> {{\n        \
             ::link_actions::value::Validate::validate(self, \"\")?;\n        Ok(())\n    }}\n}}\n\n"
        );
    }
    Ok(out)
}
