//! How far `link-spec` walks a spec's shape: a spec is a page, so a
//! shape past these is refused rather than walked on.

/// The deepest dotted path a shape walk follows, by its parts.
pub const MAX_DEPTH: usize = 16;

/// The most tables one discovery records.
pub const MAX_TABLES: usize = 2048;

/// The most single-path probes one discovery runs.
pub const MAX_PROBES: usize = 16384;

/// The prefix the spec's own properties' flags take, `--spec.version`:
/// every other field's flag is its key path alone, `--os`.
pub const TOP: &str = "spec";

/// The spec's own properties, whose flags are `--spec.<key>`: what it's
/// called, its version and what it starts from.
pub const SPEC_OWN: &[&str] = &["name", "version", "extends"];

/// The JSON Schema keyword naming the property a bare value on a
/// record's own flag sets (`x-link-shorthand`).
pub const SHORTHAND: &str = "x-link-shorthand";

/// The format envelope's keys, outside every configurable type: they
/// take no flag, and no spec has a field by their names.
pub const RESERVED: &[&str] = &["kind", "schema"];

/// How help writes a key the writer chooses, `--variables.<name>`.
pub const FREE_KEY: &str = "<name>";

/// The JSON Schema `format` of a path, which resolves from its own
/// source: a file's folder, or the current folder for a flag.
pub const PATH_FORMAT: &str = "path";

/// A host path confined to the spec's source folder after input resolution.
pub const CONFINED_PATH_FORMAT: &str = "confined-path";

/// A backtick, which reference pages and help put around code.
pub const TICK: char = '`';

/// The words of a kind's name a title writes in capitals, as the
/// abbreviations they are: `vm` is the VM Spec.
pub const ABBREVIATIONS: &[&str] = &["vm"];

/// What a tool's module is named after, as names follow history: the tool
/// `memory` is the module `memory-tool`.
pub const TOOL_MODULE: &str = "-tool";

/// What joins a tool and its call in the name a model calls
/// (`memory__remember`): the one spelling every reader of such a name
/// splits it by.
pub const TOOL_SEPARATOR: &str = "__";

/// How often one definition stands on a walk's path as names that follow
/// history are looked for: a section's own and once more as each
/// platform's overlay (`tools.linux`), never a third time, so a shape
/// naming itself is walked a bounded number of times.
pub const DEFINITION_ENTRIES: usize = 2;
