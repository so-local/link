//! Strict, typed TOML specifications and their portable paths.

use anyhow::{Context, bail};
use std::path::{Component, Path, PathBuf};

/// The versioned documents' steps, `link_files::schema`, shared with the
/// saved files below this package.
pub use link_files::schema;
pub use link_files::schema::{Specification, parse, print, print_spec};
mod argv;
mod constants;
pub use constants::{CONFINED_PATH_FORMAT, FREE_KEY, RESERVED};
pub mod entry;
pub mod fields;
pub mod follow;
pub mod history;
mod io;
pub mod probe;
pub mod reference;
mod shape;
pub mod shared;
pub use argv::argv;
pub use entry::{Parsed, Program, run, try_run};
pub use fields::{
    Binding, FieldFlags, Made, Show, SpecCommand, Trailing, clashes, flags_of, named_schema, over,
    read_text_at, read_text_with, read_with, show, split, split_claimed,
};
pub use io::{
    Converted, MAX_BYTES, Read, SECRET_WORDS, Seen, check_name, check_variable, convert,
    named_like_a_secret, parse_current, parse_spec, read, read_spec,
};
pub use shape::{Declared, Exported, Kind, check_shape, declared, kind_doc, schema};
mod format;
pub use format::{check_before, check_format};
mod roundtrip;
pub use roundtrip::check_roundtrip;
mod refusals;
pub use refusals::{
    ExampleExclusion, ExampleSample, ExampleSamples, Omission, OmissionState, Omissions,
    RequiredRefusal, RequiredRefusals, ScalarChoice, ScalarChoices, StructuralExclusion,
    StructuralType, StructuralTypes, TypeExclusion, TypeRefusal, TypeRefusals,
    check_container_type_refusals, check_example_readers, check_omissions, check_required_refusals,
    check_scalar_choices, check_structural_types, check_type_refusals, container_type_refusals,
    example_readers, omissions, required_refusals, scalar_choices, structural_types, type_refusals,
};

/// A path as a spec writes it, made absolute: `~/` is `home`, anything
/// else relative is from `dir`, the spec's folder. `..` is refused, so a
/// spec saved anywhere can't climb out of where it says.
pub fn path(written: &Path, dir: &Path, home: &Path) -> anyhow::Result<PathBuf> {
    if written
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        bail!(
            "{}: a spec's path can't have ..; write it from ~/ or in full",
            written.display()
        );
    }
    let text = written.to_string_lossy();
    let absolute = if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest)
    } else if written.is_absolute() {
        written.to_path_buf()
    } else {
        dir.join(written)
    };
    // `.` and `./`: the folder itself.
    Ok(absolute
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect())
}

/// `text` as milliseconds: a number and a unit, `ms`, `s`, `m`, `h` or
/// `d` (`"10m"`).
pub fn duration_ms(text: &str) -> anyhow::Result<u64> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: u64 = number
        .parse()
        .ok()
        .with_context(|| format!("{:?} isn't a duration, such as \"10m\"", text))?;
    let times = match unit.trim() {
        "ms" => 1,
        "s" => 1000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => bail!(
            "{:?} isn't a duration: ms, s, m, h or d after the number",
            text
        ),
    };
    number
        .checked_mul(times)
        .context("that duration is too long")
}

/// Register a spec kind: its type, which derives `schemars::JsonSchema`
/// (or declares its schema by hand where it's read by hand), and its one
/// built-in declaration, whose steps are data, with `migrations` when a
/// step names code its `migrations/` folder registers
/// ([`schema::Migrations`]). Its files name their
/// kind beside their schema. Its crate's tests then check that the
/// declared shape agrees with what the type's reader takes
/// ([`check_shape`]), so no kind joins without it. That test is the
/// module's `link_spec_shape`, so each kind registers in a module of its
/// own.
#[macro_export]
macro_rules! specification {
    (@check $ty:ty, $check:expr) => {
        impl $crate::Kind for $ty {
            fn check(&self) -> $crate::__anyhow::Result<()> {
                $check(self)
            }
        }
    };
    (@registered $ty:ty, $source:expr $(, $code:ident)?) => {
        $crate::__files::specification!($ty, $source, named $(, $code)?);
        #[cfg(test)]
        mod link_spec_shape {
            #[allow(
                unused_imports,
                reason = "a kind's type may need nothing else of its module"
            )]
            use super::*;
            #[test]
            fn the_declared_shape_is_what_the_reader_takes() {
                let mut problems = $crate::check_shape::<$ty>();
                problems.extend($crate::check_roundtrip::<$ty>());
                problems.extend($crate::check_type_refusals::<$ty>());
                problems.extend($crate::check_structural_types::<$ty>());
                problems.extend($crate::check_container_type_refusals::<$ty>());
                problems.extend($crate::check_required_refusals::<$ty>());
                problems.extend($crate::check_omissions::<$ty>());
                problems.extend($crate::check_scalar_choices::<$ty>());
                problems.extend($crate::check_example_readers::<$ty>());
                let schema = $crate::schema::<$ty>();
                problems.extend($crate::fields::check_spelling(&schema));
                problems.extend($crate::fields::uncovered(&schema));
                problems.extend($crate::check_format(
                    <$ty as $crate::Specification>::format(),
                ));
                problems.extend($crate::check_before(
                    <$ty as $crate::Specification>::format(),
                    &schema,
                ));
                assert!(
                    problems.is_empty(),
                    "{}: {}",
                    <$ty as $crate::Specification>::format().kind,
                    problems.join("\n")
                );
            }
        }
    };
    ($ty:ty, $source:expr, migrations, check = $check:expr $(,)?) => {
        $crate::specification!(@registered $ty, $source, migrations);
        $crate::specification!(@check $ty, $check);
    };
    ($ty:ty, $source:expr, migrations $(,)?) => {
        $crate::specification!(@registered $ty, $source, migrations);
        impl $crate::Kind for $ty {}
    };
    ($ty:ty, $source:expr, check = $check:expr $(,)?) => {
        $crate::specification!(@registered $ty, $source);
        $crate::specification!(@check $ty, $check);
    };
    ($ty:ty, $source:expr $(,)?) => {
        $crate::specification!(@registered $ty, $source);
        impl $crate::Kind for $ty {}
    };
}

#[doc(hidden)]
pub use anyhow as __anyhow;
#[doc(hidden)]
pub use link_files as __files;

extern crate self as link_spec;

#[cfg(test)]
link_test_kit::program!(system);
