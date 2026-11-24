//! Applicability facts annotate the existing declared-field inventory.
//! Field names still come only from link_spec::declared.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Presence {
    Required,
    Optional,
    Conditional,
    Unknown,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Facts {
    pub presence: Presence,
    pub required_when_present: Option<bool>,
    /// Missing schema metadata is unknown semantic default behavior.
    pub default: Option<Value>,
    pub choices: Vec<Value>,
    /// These literals are declared candidates; acceptance needs the owning
    /// reader when composition or additional constraints are not evaluated here.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub choices_need_review: bool,
    pub types: Vec<String>,
    pub constraints: Vec<String>,
    /// A containing collection's flag can represent nested item fields.
    pub flag: Option<String>,
    pub direct_flag: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Condition {
    Equals { path: String, value: Value },
    Present { path: String },
    Variant { path: String, reference: String },
    Schema { path: String, shape: Value },
}
#[derive(Clone)]
struct Context {
    conditions: Vec<Condition>,
    presence: Presence,
    required: Option<bool>,
    field: bool,
    required_fields: BTreeSet<String>,
}
struct Index<'a> {
    root: &'a Value,
    contexts: BTreeMap<(usize, String), Vec<Context>>,
    problems: Vec<String>,
}
fn address(value: &Value) -> usize {
    value as *const Value as usize
}
fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.into()
    } else {
        format!("{path}.{key}")
    }
}
fn target<'a>(root: &'a Value, node: &'a Value) -> Option<&'a Value> {
    node.get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| {
            if reference == "#" {
                Some(root)
            } else {
                reference
                    .strip_prefix("#/$defs/")
                    .and_then(|name| root.get("$defs")?.get(name))
            }
        })
}
fn predicate(node: &Value, path: &str) -> Vec<Condition> {
    node.get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, node)| {
            let value = node.get("const").cloned().or_else(|| {
                node.get("enum")
                    .and_then(Value::as_array)
                    .filter(|values| values.len() == 1)
                    .map(|values| values[0].clone())
            })?;
            Some(Condition::Equals {
                path: join(path, name),
                value,
            })
        })
        .collect()
}
fn required_names(root: &Value, node: &Value, seen: &mut BTreeSet<usize>) -> BTreeSet<String> {
    if !seen.insert(address(node)) {
        return BTreeSet::new();
    }
    let mut names: BTreeSet<String> = node
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    if let Some(target) = target(root, node) {
        names.extend(required_names(root, target, seen));
    }
    for arm in node
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        names.extend(required_names(root, arm, seen));
    }
    names
}
impl<'a> Index<'a> {
    fn visit(
        &mut self,
        node: &'a Value,
        path: &str,
        context: Context,
        ancestors: &mut BTreeSet<usize>,
    ) {
        let mut context = context;
        context
            .required_fields
            .extend(required_names(self.root, node, &mut BTreeSet::new()));
        self.contexts
            .entry((address(node), path.into()))
            .or_default()
            .push(context.clone());
        if !ancestors.insert(address(node)) {
            return;
        }
        if let Some(target) = target(self.root, node) {
            self.visit(target, path, context.clone(), ancestors);
        }
        if let Some(properties) = node.get("properties").and_then(Value::as_object) {
            for (name, child) in properties {
                let required = context.required_fields.contains(name)
                    || node
                        .get("required")
                        .and_then(Value::as_array)
                        .is_some_and(|names| {
                            names.iter().any(|value| value.as_str() == Some(name))
                        });
                let conditional = !context.conditions.is_empty()
                    || (!path.is_empty() && context.presence != Presence::Required);
                let mut conditions = context.conditions.clone();
                if conditional && !path.is_empty() && !conditions.iter().any(|condition|matches!(condition,Condition::Present{path:parent} if parent==path)) {conditions.push(Condition::Present{path:path.into()});}
                self.visit(
                    child,
                    &join(path, name),
                    Context {
                        conditions,
                        presence: if conditional {
                            Presence::Conditional
                        } else if required {
                            Presence::Required
                        } else {
                            Presence::Optional
                        },
                        required: Some(required),
                        field: true,
                        required_fields: BTreeSet::new(),
                    },
                    ancestors,
                );
            }
        }
        if let Some(child) = node
            .get("additionalProperties")
            .filter(|node| node.is_object())
        {
            self.visit(
                child,
                &join(path, "*"),
                Context {
                    presence: Presence::Conditional,
                    required: None,
                    field: true,
                    required_fields: BTreeSet::new(),
                    ..context.clone()
                },
                ancestors,
            );
        }
        if let Some(child) = node.get("items").filter(|node| node.is_object()) {
            self.visit(
                child,
                path,
                Context {
                    presence: Presence::Conditional,
                    field: false,
                    required_fields: BTreeSet::new(),
                    ..context.clone()
                },
                ancestors,
            );
        }
        for keyword in ["allOf", "oneOf", "anyOf"] {
            if let Some(arms) = node.get(keyword).and_then(Value::as_array) {
                for arm in arms {
                    let mut conditions = context.conditions.clone();
                    let tags = predicate(target(self.root, arm).unwrap_or(arm), path);
                    if keyword != "allOf" {
                        if tags.is_empty() {
                            if let Some(reference) = arm.get("$ref").and_then(Value::as_str) {
                                conditions.push(Condition::Variant {
                                    path: path.into(),
                                    reference: reference.into(),
                                });
                            } else if arm.get("properties").is_some() && arms.len() > 1 {
                                conditions.push(Condition::Schema {
                                    path: path.into(),
                                    shape: super::expanded(arm, self.root, &mut BTreeSet::new()),
                                });
                            }
                        }
                        conditions.extend(tags);
                    }
                    self.visit(
                        arm,
                        path,
                        Context {
                            conditions,
                            field: false,
                            ..context.clone()
                        },
                        ancestors,
                    );
                }
            }
        }
        ancestors.remove(&address(node));
    }
}
fn closed(root: &Value, node: &Value, seen: &mut BTreeSet<usize>) -> Option<Vec<Value>> {
    if !seen.insert(address(node)) {
        return None;
    }
    let mut domain = node
        .get("const")
        .map(|value| vec![value.clone()])
        .or_else(|| node.get("enum").and_then(Value::as_array).cloned())
        .or_else(|| {
            let kinds: Vec<_> = match node.get("type")? {
                Value::String(kind) => vec![kind.as_str()],
                Value::Array(kinds) => kinds.iter().filter_map(Value::as_str).collect(),
                _ => return None,
            };
            (!kinds.is_empty() && kinds.iter().all(|kind| matches!(*kind, "boolean" | "null")))
                .then(|| {
                    let mut values = vec![];
                    if kinds.contains(&"boolean") {
                        values.extend([Value::Bool(false), Value::Bool(true)]);
                    }
                    if kinds.contains(&"null") {
                        values.push(Value::Null);
                    }
                    values
                })
        });
    let intersect = |left: Option<Vec<Value>>, right: Option<Vec<Value>>| match (left, right) {
        (Some(left), Some(right)) => Some(
            left.into_iter()
                .filter(|value| right.contains(value))
                .collect(),
        ),
        (left, right) => left.or(right),
    };
    if let Some(target) = target(root, node) {
        domain = intersect(domain, closed(root, target, &mut seen.clone()));
    }
    for arm in node
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        domain = intersect(domain, closed(root, arm, &mut seen.clone()));
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(arms) = node.get(key).and_then(Value::as_array) {
            let union: Option<Vec<Vec<Value>>> = arms
                .iter()
                .map(|arm| closed(root, arm, &mut seen.clone()))
                .collect();
            domain = intersect(
                domain,
                union.map(|values| values.into_iter().flatten().collect()),
            );
        }
    }
    domain.map(|mut values| {
        values.sort_by_key(|value| serde_json::to_string(value).expect("literal"));
        values.dedup();
        values
    })
}
fn shape_types(root: &Value, node: &Value, seen: &mut BTreeSet<usize>) -> Option<BTreeSet<String>> {
    if !seen.insert(address(node)) {
        return None;
    }
    let mut types = match node.get("type") {
        Some(Value::String(kind)) => Some(BTreeSet::from([kind.clone()])),
        Some(Value::Array(kinds)) => Some(
            kinds
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        ),
        _ => None,
    };
    let intersect = |a: Option<BTreeSet<String>>, b: Option<BTreeSet<String>>| match (a, b) {
        (Some(a), Some(b)) => Some(a.intersection(&b).cloned().collect()),
        (a, b) => a.or(b),
    };
    if let Some(target) = target(root, node) {
        types = intersect(types, shape_types(root, target, &mut seen.clone()));
    }
    for arm in node
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        types = intersect(types, shape_types(root, arm, &mut seen.clone()));
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(arms) = node.get(key).and_then(Value::as_array) {
            let union: Option<Vec<BTreeSet<String>>> = arms
                .iter()
                .map(|arm| shape_types(root, arm, &mut seen.clone()))
                .collect();
            types = intersect(
                types,
                union.map(|sets| sets.into_iter().flatten().collect()),
            );
        }
    }
    types
}

fn keywords(
    root: &Value,
    node: &Value,
    seen: &mut BTreeSet<usize>,
    constraints: &mut BTreeSet<String>,
) {
    if !seen.insert(address(node)) {
        return;
    }
    for key in [
        "enum",
        "const",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "multipleOf",
        "minLength",
        "maxLength",
        "pattern",
        "minItems",
        "maxItems",
        "uniqueItems",
    ] {
        if node.get(key).is_some() {
            constraints.insert(key.into());
        }
    }
    if let Some(target) = target(root, node) {
        keywords(root, target, seen, constraints);
    }
    for key in ["allOf"] {
        for arm in node
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            keywords(root, arm, seen, constraints);
        }
    }
}

fn candidate_constraints(node: &Value) -> bool {
    let Some(object) = node.as_object() else {
        return false;
    };
    let constrained = [
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "multipleOf",
        "minLength",
        "maxLength",
        "pattern",
        "minItems",
        "maxItems",
        "uniqueItems",
        "contains",
        "minContains",
        "maxContains",
        "minProperties",
        "maxProperties",
        "not",
        "if",
        "then",
        "else",
        "format",
    ];
    object.keys().any(|key| constrained.contains(&key.as_str()))
        || node.get("oneOf").is_some()
        || ["allOf", "anyOf"].iter().any(|key| {
            node.get(*key)
                .and_then(Value::as_array)
                .is_some_and(|arms| arms.iter().any(candidate_constraints))
        })
}

pub(super) struct Metadata {
    pub facts: Facts,
    pub branch: String,
    pub nodes: Vec<Value>,
}
pub(super) fn fields(
    kind: &link_spec::Exported,
    declared: &link_spec::Declared<'_>,
) -> (BTreeMap<String, Vec<Metadata>>, Vec<String>) {
    let mut index = Index {
        root: &kind.shape,
        contexts: BTreeMap::new(),
        problems: vec![],
    };
    index.visit(
        &kind.shape,
        "",
        Context {
            conditions: vec![],
            presence: Presence::Required,
            required: Some(true),
            field: false,
            required_fields: BTreeSet::new(),
        },
        &mut BTreeSet::new(),
    );
    let flags = link_spec::fields::fields(&kind.shape);
    let mut out = BTreeMap::new();
    // A recursive boundary lists its immediate keys without a child schema or
    // branch context. Unknown facts must not borrow the root's object type,
    // constraints or defaults; the caller retains the recursive parent digest.
    let unknown = Value::Object(Default::default());
    for (table, keys) in &declared.fields {
        for key in keys {
            let path = join(table, key);
            let nodes: Vec<_> = declared
                .nodes
                .get(&path)
                .into_iter()
                .flatten()
                .copied()
                .collect();
            let mut found = BTreeMap::<String, Vec<(Context, &Value)>>::new();
            for node in &nodes {
                for context in index
                    .contexts
                    .get(&(address(node), path.clone()))
                    .into_iter()
                    .flatten()
                    .filter(|context| context.field)
                {
                    let mut context = context.clone();
                    context.conditions.sort_by_key(|condition| {
                        serde_json::to_string(condition).expect("condition serializes")
                    });
                    context.conditions.dedup();
                    let branch = if context.conditions.is_empty() {
                        String::new()
                    } else {
                        serde_json::to_string(&context.conditions).expect("conditions serialize")
                    };
                    found.entry(branch).or_default().push((context, *node));
                }
            }
            if found.is_empty() {
                found.insert(
                    String::new(),
                    vec![(
                        Context {
                            conditions: vec![],
                            presence: Presence::Unknown,
                            required: None,
                            field: true,
                            required_fields: BTreeSet::new(),
                        },
                        nodes.first().copied().unwrap_or(&unknown),
                    )],
                );
            }
            let mut metadata = vec![];
            for (branch, entries) in found {
                let context = &entries[0].0;
                let required = if entries.iter().any(|(c, _)| c.required == Some(true)) {
                    Some(true)
                } else if entries.iter().all(|(c, _)| c.required == Some(false)) {
                    Some(false)
                } else {
                    None
                };
                let presence = if entries.iter().any(|(c, _)| c.presence == Presence::Unknown) {
                    Presence::Unknown
                } else if entries
                    .iter()
                    .any(|(c, _)| c.presence == Presence::Conditional)
                {
                    Presence::Conditional
                } else if required == Some(true) {
                    Presence::Required
                } else {
                    context.presence
                };
                let mut expanded: Vec<_> = entries
                    .iter()
                    .map(|(_, node)| super::expanded(node, &kind.shape, &mut BTreeSet::new()))
                    .collect();
                expanded
                    .sort_by_key(|node| serde_json::to_string(node).expect("schema serializes"));
                expanded.dedup();
                let combined = serde_json::json!({"allOf": expanded});
                let mut choices =
                    closed(&kind.shape, &combined, &mut BTreeSet::new()).unwrap_or_default();
                choices.retain(|value| !value.is_null());
                let types =
                    shape_types(&kind.shape, &combined, &mut BTreeSet::new()).unwrap_or_default();
                let choices_need_review = !choices.is_empty()
                    && (candidate_constraints(&combined)
                        || choices
                            .iter()
                            .any(|value| value.is_array() || value.is_object())
                        || choices.iter().any(|value| {
                            !types.is_empty()
                                && !types.iter().any(|kind| match kind.as_str() {
                                    "null" => value.is_null(),
                                    "boolean" => value.is_boolean(),
                                    "string" => value.is_string(),
                                    "number" => value.is_number(),
                                    "integer" => value.is_i64() || value.is_u64(),
                                    "array" => value.is_array(),
                                    "object" => value.is_object(),
                                    _ => false,
                                })
                        }));
                let mut constraints = BTreeSet::new();
                keywords(
                    &kind.shape,
                    &combined,
                    &mut BTreeSet::new(),
                    &mut constraints,
                );
                let defaults: Vec<_> = entries
                    .iter()
                    .filter_map(|(_, node)| node.get("default"))
                    .collect();
                let default = defaults
                    .first()
                    .filter(|first| defaults.iter().all(|value| value == *first))
                    .map(|value| (*value).clone());
                let flag = flags
                    .iter()
                    .filter(|flag| {
                        let flag = flag.path.join(".").replace("<name>", "*");
                        path == flag || path.starts_with(&format!("{flag}."))
                    })
                    .max_by_key(|flag| flag.path.len());
                metadata.push(Metadata {
                    nodes: expanded,
                    branch,
                    facts: Facts {
                        presence,
                        required_when_present: required,
                        default,
                        choices,
                        choices_need_review,
                        constraints: constraints.into_iter().collect(),
                        types: types.into_iter().collect(),
                        flag: flag.map(|flag| flag.flag.clone()),
                        direct_flag: flag
                            .is_some_and(|flag| flag.path.join(".").replace("<name>", "*") == path),
                    },
                });
            }
            out.insert(path, metadata);
        }
    }
    index.problems.sort();
    index.problems.dedup();
    (out, index.problems)
}
