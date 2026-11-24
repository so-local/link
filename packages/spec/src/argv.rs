//! A command's arguments written back as the command line that makes
//! them: a caller that runs another program builds its argv from the
//! program's own typed flags, never by spelling them.

/// The arguments `value` holds, as the command line its type parses:
/// each flag by its long name, a `true` boolean as the bare flag, a list
/// as one flag a value, its operands in their order, a trailing one after
/// `--`. A flag left at none, `false` or an empty list is left out, so the
/// line parses back to `value` wherever the type's defaults are those.
pub fn argv<T: clap::Args + serde::Serialize>(value: &T) -> anyhow::Result<Vec<String>> {
    let command = T::augment_args(clap::Command::new("argv"));
    let json = serde_json::to_value(value)?;
    let fields = json
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("a command's arguments are a struct"))?;
    // A field whose serde key differs from its argument's id would be
    // left out of the line unseen: every key names an argument.
    let ids: std::collections::BTreeSet<&str> = command
        .get_arguments()
        .map(|a| a.get_id().as_str())
        .collect();
    if let Some(key) = fields.keys().find(|key| !ids.contains(key.as_str())) {
        anyhow::bail!(
            "{key} names no argument of the command: its serde key and its argument differ"
        );
    }
    let mut line = Vec::new();
    let mut operands = Vec::new();
    for arg in command.get_arguments() {
        let id = arg.get_id().as_str();
        let Some(field) = fields.get(id) else {
            continue;
        };
        let values: Vec<String> = match field {
            serde_json::Value::Null => continue,
            serde_json::Value::Bool(false) if !arg.get_action().takes_values() => continue,
            serde_json::Value::Bool(true) if !arg.get_action().takes_values() => Vec::new(),
            serde_json::Value::Array(items) if items.is_empty() => continue,
            serde_json::Value::Array(items) => items.iter().map(word).collect::<Result<_, _>>()?,
            other => vec![word(other)?],
        };
        if arg.is_positional() {
            operands.push((arg.get_index().unwrap_or(0), arg.is_last_set(), values));
            continue;
        }
        let flag = match (arg.get_long(), arg.get_short()) {
            (Some(long), _) => format!("--{long}"),
            (None, Some(short)) => format!("-{short}"),
            (None, None) => anyhow::bail!("{id} is neither a flag nor an operand"),
        };
        if values.is_empty() {
            line.push(flag);
        } else {
            for value in values {
                line.extend([flag.clone(), value]);
            }
        }
    }
    operands.sort_by_key(|(index, _, _)| *index);
    for (_, last, values) in operands {
        if last {
            line.push("--".into());
        }
        line.extend(values);
    }
    Ok(line)
}

/// One value as a command line spells it.
fn word(value: &serde_json::Value) -> anyhow::Result<String> {
    Ok(match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(flag) => flag.to_string(),
        other => anyhow::bail!("{other} is no command line's value"),
    })
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[derive(Parser, serde::Serialize, Debug, PartialEq)]
    struct Example {
        folder: String,
        #[arg(long)]
        index: Option<String>,
        #[arg(long)]
        tokens: Option<u32>,
        #[arg(long)]
        from: bool,
        #[arg(long)]
        allow: Vec<String>,
        #[arg(last = true)]
        command: Vec<String>,
    }

    #[test]
    fn a_line_parses_back_to_its_arguments() {
        let example = Example {
            folder: "/f".into(),
            index: Some("/i".into()),
            tokens: Some(9),
            from: true,
            allow: vec!["a".into(), "b".into()],
            command: vec!["ls".into(), "-l".into()],
        };
        let line = super::argv(&example).unwrap();
        assert_eq!(
            line,
            [
                "--index", "/i", "--tokens", "9", "--from", "--allow", "a", "--allow", "b", "/f",
                "--", "ls", "-l"
            ]
        );
        let back = Example::parse_from(std::iter::once("example".to_string()).chain(line));
        assert_eq!(back, example);
    }

    #[derive(clap::Args, serde::Serialize)]
    struct Renamed {
        #[arg(long)]
        #[serde(rename = "index-folder")]
        index: Option<String>,
    }

    #[test]
    fn a_field_whose_serde_key_differs_is_refused_never_left_out() {
        let refused = super::argv(&Renamed {
            index: Some("/i".into()),
        })
        .unwrap_err();
        assert!(refused.to_string().contains("index-folder"), "{refused}");
    }

    #[test]
    fn what_is_left_at_none_is_left_out() {
        let example = Example {
            folder: "/f".into(),
            index: None,
            tokens: None,
            from: false,
            allow: Vec::new(),
            command: Vec::new(),
        };
        assert_eq!(super::argv(&example).unwrap(), ["/f"]);
    }
}
