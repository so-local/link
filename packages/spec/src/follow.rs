//! Names that follow history: a module, a command or a model's tool named
//! in a spec (a field or map key whose type declares `x-link-ref`) or in
//! a recorded tool call is read as the name it has today. The release's
//! history is the core's, its modules renamed and removed, with each
//! module's retired commands ([`Wire`]); the core answers it, and a
//! program says how to ask once, at its start ([`provide`]), so nothing
//! is asked until a name is read.
//!
//! A model's tool is the module `<tool>-tool`, so a tool renamed is its
//! module renamed in the core's history, and a call (`memory__remember`)
//! is the command `<tool>-tool <call>`, renamed in its module's history.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, LazyLock, Mutex, OnceLock},
};

use anyhow::bail;
use link_files::schema::references::{Follow, Reference, Segment, Site};
use serde_json::Value;

use crate::history::{Wire, WireGone};

pub use crate::constants::TOOL_SEPARATOR;
use crate::constants::{DEFINITION_ENTRIES, TOOL_MODULE};

/// The release's history, as names follow it.
#[derive(Clone, Debug)]
pub struct References {
    wire: Wire,
}

impl Default for References {
    /// A release with no history: every name as written.
    fn default() -> Self {
        Self {
            wire: Wire::empty(),
        }
    }
}

impl From<Wire> for References {
    fn from(wire: Wire) -> Self {
        Self { wire }
    }
}

impl References {
    /// The module `module` is today, following every rename; removed, an
    /// error naming it.
    pub fn module(&self, module: &str) -> anyhow::Result<String> {
        let now = self.wire.name(module)?;
        if let Some(removed) = self.wire.removed.iter().find(|r| r.module == now) {
            bail!(
                "the {now} module is removed since {}. {}",
                removed.since,
                removed.why
            );
        }
        Ok(now.to_string())
    }

    /// The tool `tool` is today, by its module's name.
    pub fn tool(&self, tool: &str) -> anyhow::Result<String> {
        let module = self.module(&format!("{tool}{TOOL_MODULE}"))?;
        match module.strip_suffix(TOOL_MODULE) {
            Some(now) => Ok(now.to_string()),
            None => bail!("the tool {tool} is the {module} module today, no tool"),
        }
    }

    /// The command `command` (its module first) is today: its module
    /// followed, then each retirement naming it, to a current one.
    pub fn command(&self, command: &str) -> anyhow::Result<String> {
        let (module, rest) = command.split_once(' ').unwrap_or((command, ""));
        let mut now = match rest {
            "" => self.module(module)?,
            rest => format!("{} {rest}", self.module(module)?),
        };
        let mut seen = BTreeSet::new();
        while let Some(gone) = self.gone(&now) {
            anyhow::ensure!(seen.insert(now.clone()), "{command} is retired in a circle");
            match gone.now.as_deref().and_then(|n| n.strip_prefix("lnk ")) {
                Some(next) => now = next.to_string(),
                None => bail!(
                    "{now} is gone since {}. {}",
                    gone.since,
                    gone.why.as_deref().unwrap_or_default()
                ),
            }
        }
        Ok(now)
    }

    /// The name a recorded call (`memory__remember`) has today, for a
    /// program replaying a conversation to its model. It's never refused:
    /// a call to a tool or command since removed keeps its name, as does
    /// one that names no tool of Link's.
    pub fn call(&self, name: &str) -> String {
        let Some((tool, call)) = name.split_once(TOOL_SEPARATOR) else {
            return name.to_string();
        };
        let Ok(tool) = self.tool(tool) else {
            return name.to_string();
        };
        let Ok(now) = self.command(&format!("{tool}{TOOL_MODULE} {call}")) else {
            return name.to_string();
        };
        match now.split_once(' ') {
            Some((module, call)) if !call.contains(' ') => match module.strip_suffix(TOOL_MODULE) {
                Some(tool) => format!("{tool}{TOOL_SEPARATOR}{call}"),
                None => name.to_string(),
            },
            _ => name.to_string(),
        }
    }

    /// The retirement whose spelling is `command`, whole, in the core's
    /// history or a module's.
    fn gone(&self, command: &str) -> Option<&WireGone> {
        self.wire
            .gone
            .iter()
            .chain(self.wire.modules.values().flatten())
            .find(|g| g.command == command)
    }
}

impl Follow for References {
    fn follow(&self, kind: Reference, name: &str) -> anyhow::Result<String> {
        match kind {
            Reference::Module => self.module(name),
            Reference::Tool => self.tool(name),
            Reference::Command => self.command(name),
        }
    }
}

/// How this program asks for the release's history: the core's own, or a
/// module asking the core.
static PROVIDER: OnceLock<fn() -> anyhow::Result<Wire>> = OnceLock::new();

static RELEASE: LazyLock<Result<References, String>> = LazyLock::new(|| match PROVIDER.get() {
    Some(ask) => ask().map(References::from).map_err(|e| format!("{e:#}")),
    None => Ok(References::default()),
});

/// Says how this program asks for the release's history, once, at its
/// start: asked only when a name is first read. A program that says none
/// reads every name as written, as a release with no history would.
pub fn provide(ask: fn() -> anyhow::Result<Wire>) {
    let _ = PROVIDER.set(ask);
    link_files::schema::references::set_follower(&Release);
}

/// The release's history, asked once: an error, refusing every name that
/// follows it, where this program said how to ask and the asking failed,
/// so no name is read as written in a release that renamed it.
pub fn release() -> anyhow::Result<&'static References> {
    RELEASE
        .as_ref()
        .map_err(|e| anyhow::anyhow!("the release's history couldn't be read: {e}"))
}

/// The name a recorded call (`memory__remember`) has today, for a program
/// replaying a conversation to its model ([`References::call`]). The
/// release's history is asked only for a name a call of Link's tools can
/// have, holding [`TOOL_SEPARATOR`]. Never refused: where the history
/// couldn't be read the call keeps its name, as one since removed does,
/// and its tool answers it as a name it doesn't know, never as another.
pub fn replayed(name: &str) -> String {
    if !name.contains(TOOL_SEPARATOR) {
        return name.to_string();
    }
    match release() {
        Ok(release) => release.call(name),
        Err(_) => name.to_string(),
    }
}

/// The release's history, asked only when a name is followed.
struct Release;

impl Follow for Release {
    fn follow(&self, kind: Reference, name: &str) -> anyhow::Result<String> {
        release()?.follow(kind, name)
    }
}

/// Each kind's sites, found once.
static PLACES: LazyLock<Mutex<BTreeMap<String, Arc<[Site]>>>> = LazyLock::new(Default::default);

/// Reads a spec of kind `T` with every name its shape marks followed.
pub(crate) fn read<T: crate::Kind>(text: &str) -> anyhow::Result<link_files::schema::Read<T>> {
    T::format().read_following::<T>(text, &places::<T>(), &Release)
}

/// Kind `T`'s sites, walked on its first read and kept.
fn places<T: crate::Kind>() -> Arc<[Site]> {
    let kind = &T::format().kind;
    let mut places = PLACES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    places
        .entry(kind.clone())
        .or_insert_with(|| sites(&crate::schema::<T>()).into())
        .clone()
}

/// The sites of `shape` under the key `key`, for a saved file holding a
/// spec there (`[behavior]`, holding another kind's spec): what its format's
/// `references` says ([`link_files::specification`]).
pub fn sites_under(key: &str, shape: &Value) -> Vec<Site> {
    sites(shape)
        .into_iter()
        .map(|mut site| {
            site.path.insert(0, Segment::Key(key.to_string()));
            site
        })
        .collect()
}

/// Where a shape holds names that follow history: each string declaring
/// `x-link-ref`, and each map whose keys do.
pub fn sites(shape: &Value) -> Vec<Site> {
    let mut found = BTreeSet::new();
    let mut walk = Walk {
        root: shape,
        path: Vec::new(),
        entered: Vec::new(),
        found: &mut found,
    };
    walk.node(shape);
    found.into_iter().collect()
}

/// A walk of one shape: where it stands, the definitions it's inside and
/// the sites it has found.
struct Walk<'a> {
    root: &'a Value,
    path: Vec<Segment>,
    entered: Vec<&'a str>,
    found: &'a mut BTreeSet<Site>,
}

impl<'a> Walk<'a> {
    /// The definition `target` names, unless it already stands on the path
    /// as often as [`DEFINITION_ENTRIES`] allows.
    fn definition(&self, target: &'a str) -> Option<(&'a str, &'a Value)> {
        let name = target.strip_prefix("#/$defs/")?;
        let named = self.root.get("$defs")?.get(name)?;
        let entries = self.entered.iter().filter(|e| **e == name).count();
        (entries < DEFINITION_ENTRIES).then_some((name, named))
    }

    fn under(&mut self, segment: Segment, node: &'a Value) {
        self.path.push(segment);
        self.node(node);
        self.path.pop();
    }

    fn node(&mut self, node: &'a Value) {
        let marked = |node: &Value| {
            node.get("x-link-ref")
                .and_then(Value::as_str)
                .and_then(|word| word.parse::<Reference>().ok())
        };
        if let Some(target) = node.get("$ref").and_then(Value::as_str) {
            if let Some((name, named)) = self.definition(target) {
                self.entered.push(name);
                self.node(named);
                self.entered.pop();
            }
            return;
        }
        if let Some(kind) = marked(node) {
            self.found.insert(Site {
                path: self.path.clone(),
                kind,
                keys: false,
            });
        }
        if let Some(properties) = node.get("properties").and_then(Value::as_object) {
            for (key, property) in properties {
                self.under(Segment::Key(key.clone()), property);
            }
        }
        if let Some(names) = node.get("propertyNames") {
            let names = match names.get("$ref").and_then(Value::as_str) {
                Some(target) => target
                    .strip_prefix("#/$defs/")
                    .and_then(|name| self.root.get("$defs")?.get(name))
                    .unwrap_or(names),
                None => names,
            };
            if let Some(kind) = marked(names) {
                self.found.insert(Site {
                    path: self.path.clone(),
                    kind,
                    keys: true,
                });
            }
        }
        if let Some(values) = node.get("additionalProperties").filter(|v| v.is_object()) {
            self.under(Segment::Any, values);
        }
        // A list's entries stand where the list does.
        if let Some(items) = node.get("items") {
            self.node(items);
        }
        for variant in node
            .get("oneOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.node(variant);
        }
    }
}

#[cfg(test)]
mod tests;
