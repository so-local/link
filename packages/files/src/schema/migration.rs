//! A step that runs its owner's code: the change from one schema to the
//! next that no `rename`, `remove` or `refuse` can say. The step names
//! it in `migration`, its owner registers it in its `migrations/` folder
//! ([`Migrations`]), and the steps apply it in the step's place, so a file
//! reaches the current type by the steps in order.

use anyhow::{Context, bail, ensure};
use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item, TableLike};

/// The body of a document in one schema, without its `schema` and `kind`.
pub type Body = Map<String, Value>;

/// One schema's change to the next, written as code. Its name is its
/// owner's kind and its two schemas ([`name`]), and it reads the first
/// schema's shape and writes the next one's, nothing more.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    pub name: &'static str,
    pub run: fn(Body) -> anyhow::Result<Body>,
}

/// Every migration an owner's steps name, registered in its `migrations/`
/// folder and given to its format by the `migrations` word of
/// [`crate::specification!`].
pub trait Migrations {
    const ALL: &'static [Migration];
}

/// The name a step from `from` of `kind` gives its migration.
pub fn name(kind: &str, from: u32) -> String {
    format!("{kind}_{from}_to_{}", from + 1)
}

/// `registered` held to `format`'s steps: each step's migration is named
/// for its kind and schemas and registered once, it says nothing else,
/// and each registered one is named by a step.
pub(super) fn check(format: &super::Format, registered: &[Migration]) -> anyhow::Result<()> {
    for step in &format.steps {
        let Some(named) = &step.migration else {
            ensure!(
                step.flags.is_empty(),
                "{} schema step {} answers flags without running code: its rename, remove \
                 and refuse answer them",
                format.kind,
                step.from
            );
            continue;
        };
        let wanted = name(&format.kind, step.from);
        ensure!(
            *named == wanted,
            "{} schema step {} names {named:?}: name it {wanted:?}, its kind and two schemas",
            format.kind,
            step.from
        );
        ensure!(
            step.rename.is_empty() && step.remove.is_empty() && step.refuse.is_empty(),
            "{} schema step {} runs {named} and edits keys: give the edits their own step",
            format.kind,
            step.from
        );
        ensure!(
            registered.iter().filter(|m| m.name == named).count() == 1,
            "{} schema step {} names {named}, which its owner doesn't register once in its \
             migrations folder",
            format.kind,
            step.from
        );
    }
    for transfer in &format.transfers {
        let Some(named) = &transfer.migration else {
            continue;
        };
        ensure!(
            registered.iter().filter(|m| m.name == named).count() == 1,
            "{}'s transfer from {} names {named}, which its owner doesn't register once in its \
             migrations folder",
            format.kind,
            transfer.from
        );
    }
    for migration in registered {
        ensure!(
            format
                .steps
                .iter()
                .map(|s| &s.migration)
                .chain(format.transfers.iter().map(|t| &t.migration))
                .any(|named| named.as_deref() == Some(migration.name)),
            "{} registers {}, which no schema step names, nor a transfer",
            format.kind,
            migration.name
        );
    }
    Ok(())
}

impl super::Format {
    /// The code a step names, which only its owner holds: an exported
    /// format, read by a program that isn't the owner, has none.
    fn migration(&self, named: &str, version: u32) -> anyhow::Result<&Migration> {
        self.migrations
            .iter()
            .find(|m| m.name == named)
            .with_context(|| {
                let printed = match &self.command {
                    Some(command) => format!(", and {command} prints it in the current schema"),
                    None => String::new(),
                };
                format!(
                    "{} schema {version} reaches schema {} by its owner's code, which only its \
                     owner runs: its commands read the file as it is{printed}",
                    self.kind,
                    version + 1,
                )
            })
    }

    /// What a transfer's code named `named` makes of the records it took,
    /// `taken`, held at their paths in this format's file.
    pub(crate) fn run_transfer(&self, named: &str, taken: Body) -> anyhow::Result<Body> {
        let migration = self
            .migrations
            .iter()
            .find(|m| m.name == named)
            .with_context(|| format!("{} doesn't register {named}", self.kind))?;
        let made =
            (migration.run)(taken).with_context(|| format!("{} running {named}", self.kind))?;
        ensure!(
            !made.contains_key("schema") && !made.contains_key("kind"),
            "{named} writes schema or kind, which the steps own"
        );
        Ok(made)
    }

    /// `object`, in schema `version`, in the next one.
    pub(super) fn run_json(
        &self,
        named: &str,
        version: u32,
        object: &mut Body,
    ) -> anyhow::Result<()> {
        let migration = self.migration(named, version)?;
        *object = (migration.run)(std::mem::take(object))
            .with_context(|| format!("{} schema {version}", self.kind))?;
        ensure!(
            !object.contains_key("schema") && !object.contains_key("kind"),
            "{} writes schema or kind, which the steps own",
            migration.name
        );
        Ok(())
    }

    /// `doc`, in schema `version`, in the next one: what the code left
    /// unchanged keeps its comments and place.
    pub(super) fn run_document(
        &self,
        named: &str,
        version: u32,
        doc: &mut DocumentMut,
    ) -> anyhow::Result<()> {
        let mut table: toml::Table = toml::from_str(&doc.to_string())?;
        let envelope: Vec<(String, toml::Value)> = ["schema", "kind"]
            .into_iter()
            .filter_map(|key| Some((key.to_string(), table.remove(key)?)))
            .collect();
        let mut body = match serde_json::to_value(&table)? {
            Value::Object(body) => body,
            _ => bail!("a TOML document is a table"),
        };
        self.run_json(named, version, &mut body)?;
        let Ok(mut next) = toml::Value::try_from(Value::Object(body)) else {
            bail!("{} schema {} isn't a TOML table", self.kind, version + 1);
        };
        datetimes(&mut next);
        let toml::Value::Table(mut next) = next else {
            bail!("{} schema {} isn't a TOML table", self.kind, version + 1);
        };
        next.extend(envelope);
        merge(doc.as_table_mut(), &next)
    }
}

/// The key the `toml` crate holds a datetime under as it passes through
/// another format, JSON here: `{ "$__toml_private_datetime" = "..." }`.
const DATETIME: &str = "$__toml_private_datetime";

/// Each datetime in `value` made one again after its pass through JSON,
/// which holds it as a table of [`DATETIME`], so a code step keeps a
/// TOML datetime it doesn't touch.
fn datetimes(value: &mut toml::Value) {
    let held = match value {
        toml::Value::Table(table) if table.len() == 1 => table
            .get(DATETIME)
            .and_then(toml::Value::as_str)
            .and_then(|text| text.parse::<toml::value::Datetime>().ok()),
        _ => None,
    };
    if let Some(datetime) = held {
        *value = toml::Value::Datetime(datetime);
        return;
    }
    match value {
        toml::Value::Table(table) => table.iter_mut().for_each(|(_, v)| datetimes(v)),
        toml::Value::Array(items) => items.iter_mut().for_each(datetimes),
        _ => {}
    }
}

/// `table` made `next`, keeping each item whose value didn't change.
fn merge(table: &mut dyn TableLike, next: &toml::Table) -> anyhow::Result<()> {
    let gone: Vec<String> = table
        .iter()
        .map(|(key, _)| key.to_string())
        .filter(|key| !next.contains_key(key))
        .collect();
    for key in gone {
        table.remove(&key);
    }
    for (key, value) in next {
        if let (Some(inner), toml::Value::Table(next)) =
            (table.get_mut(key).and_then(Item::as_table_like_mut), value)
        {
            merge(inner, next)?;
            continue;
        }
        if table.get(key).and_then(plain).as_ref() == Some(value) {
            continue;
        }
        table.insert(key, item(key, value)?);
    }
    Ok(())
}

/// The value `item` holds.
pub(crate) fn plain(item: &Item) -> Option<toml::Value> {
    let mut doc = DocumentMut::new();
    doc.insert("v", item.clone());
    toml::from_str::<toml::Table>(&doc.to_string())
        .ok()?
        .remove("v")
}

/// `value` as a document item, written as the serializer writes it.
pub(crate) fn item(key: &str, value: &toml::Value) -> anyhow::Result<Item> {
    let mut wrapped = toml::Table::new();
    wrapped.insert(key.to_string(), value.clone());
    let mut doc: DocumentMut = toml::to_string(&wrapped)?.parse()?;
    doc.remove(key).context("the item just written")
}
