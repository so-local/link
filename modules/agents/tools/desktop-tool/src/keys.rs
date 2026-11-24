//! Keys by name, and characters as X keysyms.

use anyhow::{Result, bail};

/// The keys by name, read once from `keys.toml`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Keys {
    function_first: u32,
    functions: u32,
    names: std::collections::BTreeMap<String, u32>,
}

fn keys() -> &'static Keys {
    static KEYS: std::sync::LazyLock<Keys> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("keys.toml")).expect("keys.toml names the keys")
    });
    &KEYS
}

/// A key `key`'s keysym: a name `keys.toml` gives (`Return`, `ctrl`,
/// `minus`, `KP_Enter`), in any case, a function key (`F5`), or one
/// character.
pub fn named(key: &str) -> Result<u32> {
    let keys = keys();
    let lower = key.to_ascii_lowercase();
    if let Some(keysym) = keys.names.get(&lower) {
        return Ok(*keysym);
    }
    if let Some(n) = lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| (1..=keys.functions).contains(n))
    {
        return Ok(keys.function_first + n - 1);
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(character(c)),
        _ => bail!(
            "no key {key:?}: a key is a character, F1 to F{}, or a key's name, such as \
             Return, Tab, Escape, BackSpace, Delete, Home, PageDown, space, shift, ctrl, alt, \
             super, minus, period, or KP_Enter",
            keys.functions
        ),
    }
}

/// A combination, `ctrl+shift+t`: its keys' keysyms, pressed in order.
/// A capital letter among several keys is its key, as a person names
/// it: `ctrl+V` is `ctrl+v`, never shift too; alone, `V` types a capital.
pub fn combination(keys: &str) -> Result<Vec<u32>> {
    let keys = keys.trim();
    if keys.is_empty() {
        bail!("no key given");
    }
    // `+` alone, or ending a combination (`ctrl++`), is the key itself.
    let mut parts: Vec<&str> = keys.split('+').collect();
    if keys.ends_with('+') {
        parts.retain(|p| !p.is_empty());
        parts.push("+");
    }
    let several = parts.len() > 1;
    parts
        .iter()
        .map(|p| {
            let p = p.trim();
            match several && p.len() == 1 && p.bytes().all(|b| b.is_ascii_uppercase()) {
                true => named(&p.to_ascii_lowercase()),
                false => named(p),
            }
        })
        .collect()
}

/// The keysyms that type `text`, one a character, a Windows line end
/// (`\r\n`) one Return.
pub fn characters(text: &str) -> Vec<u32> {
    text.replace("\r\n", "\n").chars().map(character).collect()
}

/// Character `c`'s keysym: Latin-1 as itself, the rest of Unicode at
/// 0x01000000 beyond its code point, a line's end Return, a tab Tab.
pub fn character(c: char) -> u32 {
    match c {
        '\n' | '\r' => 0xff0d,
        '\t' => 0xff09,
        c if (' '..='~').contains(&c) || ('\u{a0}'..='\u{ff}').contains(&c) => c as u32,
        c => 0x0100_0000 + c as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_named_or_characters() {
        assert_eq!(combination("ctrl+c").unwrap(), [0xffe3, 'c' as u32]);
        assert_eq!(
            combination("Ctrl+Shift+T").unwrap(),
            [0xffe3, 0xffe1, 't' as u32]
        );
        // A capital among several keys is its key; alone, a capital.
        assert_eq!(combination("ctrl+V").unwrap(), [0xffe3, 'v' as u32]);
        assert_eq!(combination("V").unwrap(), ['V' as u32]);
        assert_eq!(combination("Return").unwrap(), [0xff0d]);
        assert_eq!(combination("F5").unwrap(), [0xffc2]);
        assert_eq!(combination("ctrl++").unwrap(), [0xffe3, '+' as u32]);
        // The names models send, as xdotool knows them.
        assert_eq!(combination("ctrl+minus").unwrap(), [0xffe3, '-' as u32]);
        assert_eq!(combination("KP_Enter").unwrap(), [0xff8d]);
        assert_eq!(combination("Control_L+Super_L").unwrap(), [0xffe3, 0xffeb]);
        assert_eq!(combination("F24").unwrap(), [0xffd5]);
        assert!(combination("F25").is_err());
        assert!(combination("hyper").is_err());
        assert!(combination("").is_err());
        assert_eq!(character('é'), 0xe9);
        assert_eq!(character('€'), 0x0100_20ac);
        assert_eq!(character('\n'), 0xff0d);
        assert_eq!(characters("a\r\nb\rc"), [0x61, 0xff0d, 0x62, 0xff0d, 0x63]);
    }
}
