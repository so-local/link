//! Tool actions: an action offered on `tool` names its MCP call in its
//! `tool` table, the arguments its call's title shows (`x-link-summary`),
//! and what the program gives at runtime from `[tools]`: a `{fill}` in a
//! description, an argument's choices (`x-link-fill`) and the conditions
//! a call is offered on. Their declarations are checked here; a
//! generator renders them to a template, and [`offer`] fills it.

mod check;
mod offer;

pub(crate) use check::check;
pub use offer::{Fill, Fills, Template, TemplateTool, Uses, offer};

/// Argument `arg` as a model calls it: a declared name's dashes are
/// underscores (`start-coordinate` is `start_coordinate`), the shape the
/// tools models are trained on take. A declared name holds no
/// underscore, so each wire name is one argument's.
pub fn wire_name(arg: &str) -> String {
    arg.replace('-', "_")
}

/// A call's `arguments` read as its action's arguments `T` and checked
/// as a flag's are: an argument not declared, missing, of another type,
/// or breaking its pattern, bound or choice refused, by name. Each is
/// named as the model calls it ([`wire_name`]); a declared name given
/// with its dashes is refused as no argument. No arguments at all are
/// none given.
pub fn check_arguments<T>(arguments: &serde_json::Value) -> Result<(), crate::value::Refusal>
where
    T: serde::de::DeserializeOwned + crate::value::Validate,
{
    let refused = |at: String, why: String| crate::value::Refusal {
        at: wire_name(&at),
        why: wired(&why),
    };
    let given = match arguments {
        serde_json::Value::Null => serde_json::Value::Object(Default::default()),
        serde_json::Value::Object(given) => {
            let mut declared = serde_json::Map::new();
            for (key, value) in given {
                if key.contains('-') {
                    return Err(crate::value::Refusal {
                        at: String::new(),
                        why: format!("unknown field '{key}'"),
                    });
                }
                declared.insert(key.replace('_', "-"), value.clone());
            }
            serde_json::Value::Object(declared)
        }
        given => given.clone(),
    };
    let read = T::deserialize(&given).map_err(|err| refused(String::new(), err.to_string()))?;
    read.validate("").map_err(|r| refused(r.at, r.why))
}

/// `text` with each argument it names in backticks named as the model
/// calls it; a value in backticks, such as `-5`, as it is.
fn wired(text: &str) -> String {
    let mut parts = text.split('`');
    let mut out = parts.next().unwrap_or_default().to_string();
    for (n, part) in parts.enumerate() {
        out.push('`');
        out.push_str(&match n % 2 == 0 && crate::names::is_name(part) {
            true => wire_name(part),
            false => part.to_string(),
        });
    }
    out
}

/// A call no action of the tool declares.
pub fn no_call(name: &str) -> crate::value::Refusal {
    crate::value::Refusal {
        at: String::new(),
        why: format!("no call {name}"),
    }
}

/// The `_meta` key a tool's call names the arguments its title shows in,
/// in order.
pub const SUMMARY_META: &str = "link.local/summary";

/// The `{name}`s in `text`, each a fill's name; `{` and `}` mean nothing
/// else.
pub fn placeholders(text: &str) -> anyhow::Result<Vec<&str>> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(['{', '}']) {
        anyhow::ensure!(
            rest[open..].starts_with('{'),
            "a }} with no {{ before it: braces only name a fill, as {{timeout}}"
        );
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| anyhow::anyhow!("a {{ with no }}: braces only name a fill"))?;
        let name = &after[..close];
        anyhow::ensure!(
            crate::names::is_name(name),
            "{{{name}}} names no fill: braces hold a fill's lowercase name"
        );
        found.push(name);
        rest = &after[close + 1..];
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    /// A call's arguments, as a generated arguments struct reads them.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Find {
        pattern: String,
        #[serde(default)]
        most: Option<u32>,
    }

    impl crate::value::Validate for Find {
        fn validate(&self, at: &str) -> Result<(), crate::value::Refusal> {
            let refuse = |name: &str, why: &str| crate::value::Refusal {
                at: crate::value::field(at, name),
                why: why.into(),
            };
            if self.pattern.is_empty() {
                return Err(refuse("pattern", "is at least one character"));
            }
            if self.most == Some(0) {
                return Err(refuse("most", "is at least 1"));
            }
            Ok(())
        }
    }

    /// What reading refuses (an unknown, missing, or mistyped argument)
    /// and what `Validate` refuses are both refused, by name.
    #[test]
    fn arguments_are_held_to_their_declaration() {
        use serde_json::json;
        let check = |arguments| super::check_arguments::<Find>(&arguments);
        assert!(check(json!({"pattern": "fn main"})).is_ok());
        assert!(check(json!({"pattern": "fn", "most": 3})).is_ok());
        for (arguments, says) in [
            (
                json!({"pattern": "fn", "deep": true}),
                "unknown field `deep`",
            ),
            (json!({}), "missing field `pattern`"),
            (json!(null), "missing field `pattern`"),
            (json!({"pattern": "fn", "most": "3"}), "invalid type"),
            (json!({"pattern": ""}), "pattern: is at least one character"),
            (json!({"pattern": "fn", "most": 0}), "most: is at least 1"),
        ] {
            let refused = check(arguments).unwrap_err().to_string();
            assert!(refused.contains(says), "{refused}");
        }
        assert_eq!(super::no_call("dig").to_string(), "no call dig");
    }

    /// A declared argument with a dash, as its generated struct reads it.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Drag {
        #[serde(rename = "start-at")]
        start_at: u32,
    }

    impl crate::value::Validate for Drag {
        fn validate(&self, at: &str) -> Result<(), crate::value::Refusal> {
            match self.start_at {
                0 => Err(crate::value::Refusal {
                    at: crate::value::field(at, "start-at"),
                    why: "is at least 1".into(),
                }),
                _ => Ok(()),
            }
        }
    }

    /// A model calls a dashed argument by its underscores, is refused by
    /// that name, and a call giving the dashes is refused as no argument.
    #[test]
    fn a_dashed_argument_is_called_with_underscores() {
        use serde_json::json;
        let check = |arguments| super::check_arguments::<Drag>(&arguments);
        assert_eq!(super::wire_name("start-at"), "start_at");
        assert!(check(json!({"start_at": 3})).is_ok());
        for (arguments, says) in [
            (json!({"start_at": 0}), "start_at: is at least 1"),
            (json!({"start-at": 3}), "unknown field 'start-at'"),
            (json!({}), "missing field `start_at`"),
            (json!({"start_at": -5}), "integer `-5`"),
        ] {
            let refused = check(arguments).unwrap_err().to_string();
            assert!(refused.contains(says), "{refused}");
        }
    }

    #[test]
    fn braces_name_fills_and_nothing_else() {
        assert_eq!(
            super::placeholders("Stops after {timeout}, at most {most}.").unwrap(),
            ["timeout", "most"]
        );
        assert!(super::placeholders("No braces.").unwrap().is_empty());
        for bad in ["a {", "a }", "{Big}", "{a {b}}", "{}"] {
            assert!(super::placeholders(bad).is_err(), "{bad}");
        }
    }
}
