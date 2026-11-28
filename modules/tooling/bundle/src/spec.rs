//! A bundle's spec: a built-in bundle or its file, with the spec's field
//! flags over it, every path absolute.

use std::path::{Path, PathBuf};

use anyhow::Context;
use link_spec::FieldFlags;

use crate::absolute;
use crate::cli::{Bundle, BundleCreateArgs};

/// What `lnk bundle create` makes: its spec, the name it's kept by and
/// the secret that signs it.
#[derive(Debug)]
pub struct Merged {
    /// The built-in bundle's name or its spec file's, else `bundle`.
    pub name: String,
    pub fields: Bundle,
    pub signing_secret: Option<PathBuf>,
}

/// `create`'s spec, a built-in bundle by name or its file, with `flags`
/// over it, read once through the spec's reader. A file's relative paths
/// are its folder's, a built-in bundle's and a flag's the folder it's run
/// in, `here`.
pub fn merged(create: BundleCreateArgs, flags: &FieldFlags, here: &Path) -> anyhow::Result<Merged> {
    let BundleCreateArgs {
        name,
        spec,
        signing_secret,
    } = create;
    let signing_secret = signing_secret.map(|path| absolute(&path, here));
    let (mut fields, dir) = match (&name, &spec) {
        (Some(_), Some(_)) => {
            anyhow::bail!("it takes a built-in bundle by name or a spec file (--spec), never both")
        }
        (Some(name), None) => (link_spec::over(&named(name)?, flags)?, here.to_path_buf()),
        (None, Some(path)) => {
            let made = link_spec::read_with::<Bundle>(Some(&absolute(path, here)), flags)?;
            (made.spec, made.dir)
        }
        (None, None) => (
            link_spec::over(&Bundle::default(), flags)?,
            here.to_path_buf(),
        ),
    };
    // A signer the flags named is the folder it's run in's.
    let dir = match flags.get(&["signer"]) {
        Some(_) => here.to_path_buf(),
        None => dir,
    };
    rooted(&mut fields, &dir);
    let name = name
        .or_else(|| {
            spec.as_ref()
                .and_then(|s| s.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
        })
        .filter(|s| s != "bundle")
        .unwrap_or_else(|| "bundle".to_string());
    Ok(Merged {
        name,
        fields,
        signing_secret,
    })
}

/// A built-in bundle's spec, by its name (`lnk bundle spec show devbox`),
/// its paths as written.
pub fn named(name: &str) -> anyhow::Result<Bundle> {
    let text = shipped(name).with_context(|| {
        let names: Vec<&str> = SHIPPED.iter().map(|(n, _)| *n).collect();
        format!(
            "no bundle is named {name}: it comes with {}, and a spec file is --spec FILE",
            names.join(", ")
        )
    })?;
    read(text).with_context(|| format!("the built-in bundle {name} isn't a bundle spec"))
}

/// A bundle spec file's text, read strictly.
pub fn read(text: &str) -> anyhow::Result<Bundle> {
    link_spec::parse_spec(text)
}

/// The bundles the module comes with, by name, each a spec file in `bundles/`.
pub const SHIPPED: &[(&str, &str)] = &[
    ("devbox", include_str!("../bundles/devbox.toml")),
    ("link", include_str!("../bundles/link.toml")),
    ("prodbox", include_str!("../bundles/prodbox.toml")),
    ("staging", include_str!("../bundles/staging.toml")),
];

/// A built-in bundle's spec, by its name.
pub fn shipped(name: &str) -> Option<&'static str> {
    SHIPPED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| *text)
}

/// Every path of `spec` made absolute against `dir`. A signer by its
/// name stays a name.
fn rooted(spec: &mut Bundle, dir: &Path) {
    for list in [
        &mut spec.source,
        &mut spec.components,
        &mut spec.outside,
        &mut spec.file,
        &mut spec.from,
        &mut spec.archives,
    ] {
        for path in list.iter_mut().flatten() {
            *path = absolute(path, dir);
        }
    }
    if let Some(out) = &mut spec.out {
        *out = absolute(out, dir);
    }
    // A signer by name (`link`, `dev`) stays a name; one with a path's
    // marks is a signers file.
    if let Some(signer) = &mut spec.signer
        && signer.contains(['/', '.', '~'])
    {
        *signer = absolute(Path::new(signer), dir)
            .to_string_lossy()
            .into_owned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The field flags `words` give `lnk bundle create`.
    fn flags(words: &[&str]) -> FieldFlags {
        let args = std::iter::once("lnk")
            .chain(words.iter().copied())
            .map(Into::into);
        let binding = link_spec::Binding::of::<Bundle>();
        let (flags, rest) = link_spec::split(args, Some(&binding), None).unwrap();
        assert_eq!(rest.len(), 1, "{rest:?}");
        flags
    }

    /// `lnk bundle create <name>`.
    fn shipped_one(name: &str) -> BundleCreateArgs {
        BundleCreateArgs {
            name: Some(name.into()),
            ..BundleCreateArgs::default()
        }
    }

    /// `lnk bundle create --spec <file>`.
    fn from_file(spec: impl Into<std::path::PathBuf>) -> BundleCreateArgs {
        BundleCreateArgs {
            spec: Some(spec.into()),
            ..BundleCreateArgs::default()
        }
    }

    #[test]
    fn a_flag_takes_its_fields_place() {
        let dir = link_test_kit::temp::TempDir::new("bundle-spec");
        let spec = dir.path().join("public.toml");
        std::fs::write(
            &spec,
            "schema = 1\nsource = [\"work\"]\nprogram = [\"lnk\", \"lnk-measure\"]\nbuild = \"prod\"\nsigner = \"link\"\n",
        )
        .unwrap();
        let given = flags(&[
            "--target",
            "x86_64-unknown-linux-musl",
            "--build",
            "staging",
        ]);
        let merged = merged(from_file(&spec), &given, Path::new("/work")).unwrap();
        assert_eq!(merged.name, "public");
        let merged = merged.fields;
        assert_eq!(
            merged.program.as_deref().unwrap_or_default(),
            ["lnk", "lnk-measure"]
        );
        assert_eq!(
            merged.target.as_deref().unwrap_or_default(),
            [crate::cli::Target::X8664UnknownLinuxMusl]
        );
        assert_eq!(merged.build, Some(crate::cli::Build::Staging));
        assert_eq!(merged.signer.as_deref(), Some("link"));
        assert_eq!(
            merged.source.as_deref().unwrap_or_default(),
            [dir.path().join("work")]
        );
    }

    #[test]
    fn every_shipped_bundle_is_a_spec() {
        for (name, text) in SHIPPED {
            let spec = read(text).unwrap_or_else(|e| panic!("{name}: {e:#}"));
            assert!(spec.signer.is_some(), "{name} names its signer");
        }
    }

    #[test]
    fn a_shipped_name_reads_its_spec() {
        let dir = link_test_kit::temp::TempDir::new("bundle-shipped");
        let merged = merged(shipped_one("devbox"), &flags(&[]), dir.path()).unwrap();
        assert_eq!(merged.name, "devbox");
        assert_eq!(merged.fields.build, Some(crate::cli::Build::Dev));
        assert!(
            merged
                .fields
                .program
                .as_deref()
                .unwrap_or_default()
                .contains(&"lnk-box".to_string())
        );
    }

    #[test]
    fn an_unknown_name_is_refused_with_the_shipped_ones() {
        let error = merged(shipped_one("nope"), &flags(&[]), Path::new("/work")).unwrap_err();
        assert!(format!("{error:#}").contains("devbox"), "{error:#}");
    }

    /// A spec file that says `field = ...`, read as `lnk bundle create` does.
    fn refused(field: &str) -> String {
        let dir = link_test_kit::temp::TempDir::new("bundle-spec");
        let spec = dir.path().join("bad.toml");
        std::fs::write(&spec, format!("schema = 1\n{field} = \"x\"\n")).unwrap();
        let error = merged(from_file(spec), &flags(&[]), Path::new("/work"))
            .expect_err("a field no spec has was read");
        format!("{error:#}")
    }

    #[test]
    fn a_dev_box_holds_what_an_agent_moved_there_runs() {
        let dir = link_test_kit::temp::TempDir::new("bundle-devbox");
        let merged = merged(shipped_one("devbox"), &flags(&[]), dir.path()).unwrap();
        // lnk agent move adds agent, secret and model there, the agent's
        // start its harness, and agent requires sandbox, mcp, which
        // requires manual, and model, which serves its model calls.
        for program in [
            "lnk-agent",
            "lnk-secret",
            "lnk-model",
            "lnk-harness",
            "lnk-sandbox",
            "lnk-mcp",
            "lnk-manual",
        ] {
            assert!(
                merged
                    .fields
                    .program
                    .as_deref()
                    .unwrap_or_default()
                    .contains(&program.to_string()),
                "devbox holds {program}"
            );
        }
    }

    #[test]
    fn a_target_link_never_releases_is_refused_from_a_file_or_a_flag() {
        let dir = link_test_kit::temp::TempDir::new("bundle-target");
        let spec = dir.path().join("odd.toml");
        std::fs::write(
            &spec,
            "schema = 1\ntarget = [\"riscv64gc-unknown-linux-gnu\"]\n",
        )
        .unwrap();
        let error = merged(from_file(&spec), &flags(&[]), Path::new("/work")).unwrap_err();
        assert!(format!("{error:#}").contains("riscv64gc"), "{error:#}");
        let args = ["lnk", "--target", "riscv64gc-unknown-linux-gnu"]
            .into_iter()
            .map(Into::into);
        let binding = link_spec::Binding::of::<Bundle>();
        let refused = link_spec::split(args, Some(&binding), None)
            .and_then(|(given, _)| merged(shipped_one("devbox"), &given, Path::new("/work")));
        let error = refused.expect_err("an unknown target was read");
        assert!(format!("{error:#}").contains("riscv64gc"), "{error:#}");
    }

    #[test]
    fn an_unknown_field_is_refused() {
        assert!(refused("programs").contains("programs"));
    }

    #[test]
    fn a_spec_file_never_names_a_spec() {
        let refused = refused("spec");
        assert!(refused.contains("unknown field `spec`"), "{refused}");
    }

    #[test]
    fn a_spec_file_never_holds_the_signing_secret() {
        assert!(refused("signing_secret").contains("signing_secret"));
    }

    #[test]
    fn the_signing_secret_is_the_flags_alone() {
        let dir = link_test_kit::temp::TempDir::new("bundle-shipped");
        let create = BundleCreateArgs {
            signing_secret: Some("secret".into()),
            ..shipped_one("devbox")
        };
        let merged = merged(create, &flags(&[]), dir.path()).unwrap();
        assert_eq!(merged.signing_secret, Some(dir.path().join("secret")));
    }
}
