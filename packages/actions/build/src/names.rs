//! Declared names as Rust names.

/// `post-id` as `PostId`, `ubuntu-24.04` as `Ubuntu2404`.
pub(crate) fn pascal(text: &str) -> String {
    let out: String = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map_or(String::new(), |c| {
                c.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect();
    if out.starts_with(|c: char| c.is_ascii_digit()) || out.is_empty() {
        format!("V{out}")
    } else {
        out
    }
}

/// A property or argument as a Rust field: `no-sandbox` as `no_sandbox`,
/// a keyword prefixed.
pub(crate) fn field(property: &str) -> String {
    // Rust's strict and reserved keywords: a reserved one, as `try`, is
    // refused as a field name just like a strict one.
    const KEYWORDS: &[&str] = &[
        "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
        "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
        "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub",
        "ref", "return", "self", "static", "struct", "super", "trait", "true", "try", "type",
        "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
    ];
    // A camel-case word, as MCP's `structuredContent`, is snake case too.
    let mut snake = String::new();
    for (i, c) in property.chars().enumerate() {
        match c {
            c if c.is_ascii_uppercase() => {
                if i > 0 {
                    snake.push('_');
                }
                snake.push(c.to_ascii_lowercase());
            }
            c if c.is_ascii_alphanumeric() => snake.push(c),
            _ => snake.push('_'),
        }
    }
    if KEYWORDS.contains(&snake.as_str()) || snake.starts_with(|c: char| c.is_ascii_digit()) {
        format!("r#{snake}")
            .replace("r#self", "self_")
            .replace("r#crate", "crate_")
            .replace("r#super", "super_")
    } else {
        snake
    }
}

/// An action's method and type stem: its resource and verb,
/// `approval_add`, `env_create`.
pub(crate) fn method(id: &link_actions::ActionId) -> String {
    field(&format!("{}_{}", id.resource(), id.verb()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_rust_names() {
        assert_eq!(pascal("post-id"), "PostId");
        assert_eq!(pascal("ubuntu-24.04"), "Ubuntu2404");
        assert_eq!(pascal("24h"), "V24h");
        assert_eq!(field("no-sandbox"), "no_sandbox");
        assert_eq!(field("box"), "r#box");
        assert_eq!(field("type"), "r#type");
        assert_eq!(field("try"), "r#try");
        assert_eq!(method(&"env.env.create".parse().unwrap()), "env_create");
        assert_eq!(method(&"env.approval.add".parse().unwrap()), "approval_add");
    }
}
