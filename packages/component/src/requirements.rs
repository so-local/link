//! Required closure for one operation on its execution platform.
use crate::{Binding, Dependency, Requirement, Resolved, Value, ValueType};
use anyhow::{Context, ensure};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Environment<'a> {
    pub os: &'a str,
    pub arch: &'a str,
    pub libc: &'a str,
}
impl Environment<'static> {
    pub fn here() -> Self {
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            libc: if cfg!(target_env = "musl") {
                "musl"
            } else if cfg!(target_env = "gnu") {
                "gnu"
            } else {
                "none"
            },
        }
    }
}

#[derive(Debug, Default)]
pub struct Closure<'a> {
    pub modules: Vec<&'a str>,
    pub requirements: Vec<(&'a str, &'a str, &'a Dependency)>,
}

pub fn applicable(
    edge: &Requirement,
    environment: &Environment<'_>,
    values: &BTreeMap<String, Value>,
) -> bool {
    edge.when.as_ref().is_none_or(|when| {
        (when.os.is_empty() || when.os.iter().any(|v| v == environment.os))
            && (when.arch.is_empty() || when.arch.iter().any(|v| v == environment.arch))
            && (when.libc.is_empty() || when.libc.iter().any(|v| v == environment.libc))
            && when
                .options
                .iter()
                .all(|(name, value)| values.get(name) == Some(value))
    })
}

/// Apply the owning defaults and reject values outside its typed schema.
pub fn effective_options(
    command: Option<&crate::Command>,
    values: &BTreeMap<String, Value>,
) -> anyhow::Result<BTreeMap<String, Value>> {
    let mut effective: BTreeMap<String, Value> = command
        .into_iter()
        .flat_map(|c| c.options.iter())
        .filter_map(|(name, schema)| schema.default.clone().map(|v| (name.clone(), v)))
        .collect();
    effective.extend(values.clone());
    for (name, value) in &effective {
        let schema = command
            .and_then(|c| c.options.get(name))
            .with_context(|| format!("unknown option {name}"))?;
        let valid = matches!(
            (&schema.r#type, value),
            (ValueType::String, Value::String(_))
                | (ValueType::Boolean, Value::Boolean(_))
                | (ValueType::Integer, Value::Integer(_))
        );
        ensure!(
            valid
                && schema
                    .values
                    .as_ref()
                    .is_none_or(|allowed| allowed.contains(value)),
            "mistyped option {name}"
        );
    }
    Ok(effective)
}

pub fn required<'a>(
    registry: &'a BTreeMap<String, Resolved>,
    module: &str,
    command: Option<&str>,
    values: &BTreeMap<String, Value>,
    environment: &Environment<'_>,
) -> anyhow::Result<Closure<'a>> {
    struct Resolver<'a, 'b> {
        registry: &'a BTreeMap<String, Resolved>,
        environment: &'b Environment<'b>,
        active: Vec<String>,
        done: BTreeSet<String>,
        closure: Closure<'a>,
    }
    impl<'a> Resolver<'a, '_> {
        fn visit(
            &mut self,
            module: &str,
            command: Option<&str>,
            values: &BTreeMap<String, Value>,
        ) -> anyhow::Result<()> {
            let descriptor = self
                .registry
                .get(module)
                .with_context(|| format!("unknown module {module}"))?;
            let operation = command
                .map(|name| {
                    descriptor
                        .commands
                        .get(name)
                        .with_context(|| format!("{module} has no declared command {name}"))
                })
                .transpose()?;
            let effective = effective_options(operation, values)?;
            let values = &effective;
            let state = format!(
                "{module}:{}:{}",
                command.unwrap_or("baseline"),
                serde_json::to_string(values)?
            );
            ensure!(
                !self.active.contains(&state),
                "required operation cycle: {} -> {state}",
                self.active.join(" -> ")
            );
            if self.done.contains(&state) {
                return Ok(());
            }
            self.active.push(state.clone());
            for edge in descriptor
                .requires
                .iter()
                .chain(operation.into_iter().flat_map(|c| &c.requires))
            {
                if !applicable(edge, self.environment, values) {
                    continue;
                }
                let dependency = if edge.dependency == "self" {
                    None
                } else {
                    Some(
                        descriptor
                            .dependencies
                            .get(&edge.dependency)
                            .with_context(|| {
                                format!("unknown dependency {module}:{}", edge.dependency)
                            })?,
                    )
                };
                match dependency {
                    None | Some(Dependency::Module { .. }) => {
                        let next = match dependency {
                            Some(Dependency::Module { id, protocol }) => {
                                ensure!(
                                    protocol.is_none_or(|v| v == crate::SCHEMA),
                                    "unsupported required module protocol"
                                );
                                id.as_str()
                            }
                            _ => module,
                        };
                        let bound = edge
                            .options
                            .iter()
                            .map(|(name, binding)| {
                                Ok((
                                    name.clone(),
                                    match binding {
                                        Binding::Literal { literal } => literal.clone(),
                                        Binding::From { from } => values
                                            .get(from)
                                            .with_context(|| {
                                                format!("missing projected option {from}")
                                            })?
                                            .clone(),
                                    },
                                ))
                            })
                            .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
                        self.visit(next, edge.command.as_deref(), &bound)?;
                    }
                    Some(dependency) => {
                        ensure!(
                            edge.command.is_none() && edge.options.is_empty(),
                            "external targets have no module command"
                        );
                        let request = (
                            descriptor.component.id.as_str(),
                            edge.dependency.as_str(),
                            dependency,
                        );
                        if !self
                            .closure
                            .requirements
                            .iter()
                            .any(|(owner, alias, _)| owner == &request.0 && alias == &request.1)
                        {
                            self.closure.requirements.push(request);
                        }
                    }
                }
            }
            self.active.pop();
            self.done.insert(state);
            if !self
                .closure
                .modules
                .contains(&descriptor.component.id.as_str())
            {
                self.closure.modules.push(descriptor.component.id.as_str());
            }
            Ok(())
        }
    }
    let mut resolver = Resolver {
        registry,
        environment,
        active: vec![],
        done: BTreeSet::new(),
        closure: Closure::default(),
    };
    resolver.visit(module, command, values)?;
    Ok(resolver.closure)
}

/// What a command's args bind ([`command_values`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Values {
    /// Each known option's typed value, its default where it's not given.
    Bound(BTreeMap<String, Value>),
    /// Option `option` was given a value its type or its `values` refuse,
    /// on the command line or through its environment variable: the
    /// owning parser refuses the args, so nothing runs that needs
    /// anything.
    Refused { option: String },
}

/// Bind the owner-declared CLI option schema. Unknown syntax remains the owning
/// parser's error; dependency selection consumes only its known typed settings.
/// A malformed value of a known option is the owning parser's error too:
/// [`Values::Refused`], never an error here.
pub fn command_values(command: &crate::Command, args: &[String]) -> Values {
    fn value(schema: &crate::OptionDeclaration, text: &str) -> Option<Value> {
        let bound = match schema.r#type {
            ValueType::Boolean => Value::Boolean(text.parse().ok()?),
            ValueType::Integer => Value::Integer(text.parse().ok()?),
            ValueType::String => Value::String(text.into()),
        };
        schema
            .values
            .as_ref()
            .is_none_or(|v| v.contains(&bound))
            .then_some(bound)
    }
    let mut values: BTreeMap<String, Value> = command
        .options
        .iter()
        .filter_map(|(n, s)| s.default.clone().map(|v| (n.clone(), v)))
        .collect();
    let mut explicit = BTreeSet::new();
    let mut index = 0;
    let mut position = 1;
    let mut positional = false;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if arg == "--" {
            positional = true;
            continue;
        }
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        let found = command.options.iter().find(|(_, s)| {
            if !positional && flag.starts_with("--") {
                s.long.as_ref().is_some_and(|l| flag == format!("--{l}"))
            } else if !positional && flag.starts_with('-') {
                s.short.as_ref().is_some_and(|l| flag == format!("-{l}"))
            } else {
                s.position == Some(position)
            }
        });
        let Some((name, schema)) = found else {
            continue;
        };
        let text = if schema.position.is_some() {
            position += 1;
            arg.as_str()
        } else if let Some(v) = inline {
            v
        } else if matches!(schema.r#type, ValueType::Boolean) {
            "true"
        } else {
            let Some(v) = args.get(index) else { continue };
            index += 1;
            v
        };
        let Some(bound) = value(schema, text) else {
            return Values::Refused {
                option: name.clone(),
            };
        };
        explicit.insert(name.clone());
        values.insert(name.clone(), bound);
    }
    for (name, schema) in &command.options {
        if let Some(env) = schema
            .environment
            .as_ref()
            .and_then(|e| std::env::var(e).ok())
            && !explicit.contains(name)
        {
            let Some(bound) = value(schema, &env) else {
                return Values::Refused {
                    option: name.clone(),
                };
            };
            values.insert(name.clone(), bound);
        }
    }
    Values::Bound(values)
}

/// The declared command a module's args ask for ([`operation_for_args`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Operation<'a> {
    /// No declared command matches: the module's own needs alone.
    Unmatched,
    /// Command `name`, with what its args bind.
    Matched { name: &'a str, values: Values },
}

/// Match the deepest declared command while preserving global options wherever
/// the owning parser allows them. Option values never become command words.
pub fn operation_for_args<'a>(descriptor: &'a Resolved, args: &[String]) -> Operation<'a> {
    let mut candidates: Vec<_> = descriptor
        .commands
        .iter()
        .filter(|(name, _)| name.as_str() != "root")
        .collect();
    candidates.sort_by_key(|(name, _)| std::cmp::Reverse(name.split_whitespace().count()));
    for (name, command) in candidates {
        let words: Vec<_> = name.split_whitespace().collect();
        let mut matched = 0;
        let mut index = 0;
        let mut options = vec![];
        while index < args.len() {
            let word = &args[index];
            index += 1;
            if word == "--" {
                options.push(word.clone());
                options.extend_from_slice(&args[index..]);
                break;
            }
            if word.starts_with('-') {
                options.push(word.clone());
                let flag = word.split('=').next().unwrap_or(word);
                let schema = command.options.values().find(|s| {
                    s.long.as_ref().is_some_and(|l| flag == format!("--{l}"))
                        || s.short.as_ref().is_some_and(|l| flag == format!("-{l}"))
                });
                if !word.contains('=')
                    && schema.is_some_and(|s| !matches!(s.r#type, ValueType::Boolean))
                    && index < args.len()
                {
                    options.push(args[index].clone());
                    index += 1;
                }
            } else if matched < words.len() {
                if word != words[matched] {
                    break;
                }
                matched += 1;
            } else {
                options.push(word.clone());
            }
        }
        if matched == words.len() {
            return Operation::Matched {
                name,
                values: command_values(command, &options),
            };
        }
    }
    Operation::Unmatched
}
