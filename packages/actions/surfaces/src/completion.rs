//! Shell completion for `lnk`: each command a person types, the words
//! after each one, and the flags each takes with their help, written from
//! the catalog for bash and zsh. A command's runtime flags come from its
//! declared arguments and its field flags from its bound spec, as its
//! help lists them (`link_spec::fields`), so what completes is what the
//! command line takes.

use std::{collections::BTreeMap, fmt::Write as _};

use link_actions::{Action, Arg, Catalog, Role, Surface, schema::Node};

use crate::{Files, config::SETTINGS};

/// A word a person may type next: its help's first line, and the values
/// its flag takes, where they're a closed set.
#[derive(Clone, Debug, Default, PartialEq)]
struct Word {
    help: String,
    values: Vec<String>,
}

/// Every command line's next words, by the words typed before them,
/// `lnk` first, joined by a space: `lnk` for the first word.
#[derive(Debug, Default)]
struct Tree {
    next: BTreeMap<String, BTreeMap<String, Word>>,
}

impl Tree {
    fn add(&mut self, at: &str, word: &str, help: Word) {
        let words = self.next.entry(at.to_string()).or_default();
        // The first help said stays: a group's word is said by its first
        // command, in ID order.
        words.entry(word.to_string()).or_insert(help);
    }
}

/// The completion scripts for every action `catalog` offers on the
/// command line, by their paths from `root`, the workspace: none where
/// `lnk`'s own module, core, is in another workspace, which writes them.
pub fn generate(catalog: &Catalog, root: &std::path::Path) -> anyhow::Result<Files> {
    let mut files = Files::new();
    let core = catalog.module(&link_actions::core().module);
    if !core.is_some_and(|core| core.directory.starts_with(root)) {
        return Ok(files);
    }
    let tree = tree(catalog)?;
    files.insert(&SETTINGS.completion.bash, bash(&tree))?;
    files.insert(&SETTINGS.completion.zsh, zsh(&tree))?;
    Ok(files)
}

fn tree(catalog: &Catalog) -> anyhow::Result<Tree> {
    let mut tree = Tree::default();
    let actions = crate::offered(catalog, Surface::Cli).filter(|a| a.role != Role::Plumbing);
    for action in actions {
        let program = link_actions::core().program.as_str();
        let typed: Vec<&str> = std::iter::once(program).chain(action.id.typed()).collect();
        for (depth, word) in typed.iter().enumerate().skip(1) {
            let at = typed[..depth].join(" ");
            let help = match depth + 1 == typed.len() {
                true => first_line(action.summary()),
                false => String::new(),
            };
            tree.add(
                &at,
                word,
                Word {
                    help,
                    values: Vec::new(),
                },
            );
        }
        let at = typed.join(" ");
        for (name, arg) in flags(action) {
            tree.add(&at, &format!("--{name}"), runtime(catalog, arg)?);
            if let Some(short) = arg.cli.short {
                tree.add(&at, &format!("-{short}"), runtime(catalog, arg)?);
            }
        }
        if let Some(spec) = &action.spec {
            if !action.args.contains_key(&link_actions::core().spec) {
                let help = SETTINGS.completion.spec_help.clone();
                tree.add(
                    &at,
                    "--spec",
                    Word {
                        help,
                        values: Vec::new(),
                    },
                );
            }
            let schema = catalog.json_schema(spec)?;
            for field in link_spec::fields::fields(&schema) {
                // A free key's flag names a key the person chooses.
                if field.path.iter().any(|key| key == link_spec::FREE_KEY) {
                    continue;
                }
                let word = Word {
                    help: first_line(field.summary.as_deref().unwrap_or_default()),
                    values: choices(&field.hint),
                };
                tree.add(&at, &format!("--{}", field.flag), word);
            }
        }
    }
    // A group's word with no help of its own says which commands are in it.
    let groups: Vec<(String, String)> = tree
        .next
        .iter()
        .flat_map(|(at, words)| {
            words
                .iter()
                .filter(|(_, word)| word.help.is_empty())
                .map(move |(word, _)| (at.clone(), word.clone()))
        })
        .collect();
    for (at, word) in groups {
        let path = format!("{at} {word}");
        let under: Vec<&str> = tree
            .next
            .get(&path)
            .map(|w| {
                w.keys()
                    .filter(|w| !w.starts_with('-'))
                    .map(String::as_str)
                    .collect()
            })
            .unwrap_or_default();
        let help = format!("{} {}", SETTINGS.completion.group_help, under.join(", "));
        if let Some(found) = tree.next.get_mut(&at).and_then(|w| w.get_mut(&word)) {
            found.help = help;
        }
    }
    // An alias completes as its group does: the same words after it, and
    // its own word beside the group's, its help the group's.
    let program = link_actions::core().program.as_str();
    for module in catalog.modules() {
        for (group, alias) in &module.aliases {
            let full = format!("{program} {group}");
            let (parent, _) = full.rsplit_once(' ').unwrap_or((program, ""));
            let short = format!("{parent} {alias}");
            let below: Vec<(String, BTreeMap<String, Word>)> = tree
                .next
                .iter()
                .filter(|(at, _)| **at == full || at.starts_with(&format!("{full} ")))
                .map(|(at, words)| (format!("{short}{}", &at[full.len()..]), words.clone()))
                .collect();
            tree.next.extend(below);
            let word = full[parent.len() + 1..].to_string();
            if let Some(help) = tree.next.get(parent).and_then(|w| w.get(&word)).cloned() {
                tree.add(parent, alias, help);
            }
        }
    }
    Ok(tree)
}

/// An action's runtime flags, as its command line takes them: every
/// argument but its positionals.
fn flags(action: &Action) -> impl Iterator<Item = (&String, &Arg)> {
    action
        .args
        .iter()
        .filter(|(_, arg)| arg.cli.position.is_none() && !arg.cli.rest)
}

/// A runtime flag's help and the values it takes, a named enum's among
/// them, read through its name.
fn runtime(catalog: &Catalog, arg: &Arg) -> anyhow::Result<Word> {
    let help = arg.schema.meta.description.as_deref().unwrap_or_default();
    let values = match &catalog.resolve(&arg.schema)?.node {
        Node::Enum(values) => values
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    Ok(Word {
        help: first_line(help),
        values,
    })
}

/// The values a field flag's hint names, `<off|low|high>`, where they're
/// a closed set of plain words.
fn choices(hint: &str) -> Vec<String> {
    let Some(inner) = hint.strip_prefix('<').and_then(|h| h.strip_suffix('>')) else {
        return Vec::new();
    };
    let words: Vec<String> = inner.split('|').map(str::to_string).collect();
    let plain = words.len() > 1
        && words.iter().all(|w| {
            !w.is_empty()
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c))
        });
    if plain { words } else { Vec::new() }
}

fn first_line(text: &str) -> String {
    let line = text.trim().lines().next().unwrap_or_default();
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` in single quotes, as both shells read it.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn header(comment: &str) -> String {
    format!(
        "# {}\n# {comment}\n# shellcheck disable=SC2034\n\n",
        crate::marker()
    )
}

fn bash(tree: &Tree) -> String {
    let mut out = header(&SETTINGS.completion.bash_comment);
    out.push_str("declare -gA _lnk_next=(\n");
    for (at, words) in &tree.next {
        let joined: Vec<&str> = words.keys().map(String::as_str).collect();
        let _ = writeln!(out, "  [{}]={}", quoted(at), quoted(&joined.join(" ")));
    }
    out.push_str(")\ndeclare -gA _lnk_values=(\n");
    for (at, words) in &tree.next {
        for (word, help) in words.iter().filter(|(_, w)| !w.values.is_empty()) {
            let key = format!("{at} {word}");
            let _ = writeln!(
                out,
                "  [{}]={}",
                quoted(&key),
                quoted(&help.values.join(" "))
            );
        }
    }
    out.push_str(BASH_BODY);
    out
}

fn zsh(tree: &Tree) -> String {
    let mut out = header(&SETTINGS.completion.zsh_comment);
    out.push_str("typeset -gA _lnk_next _lnk_values\n_lnk_next=(\n");
    for (at, words) in &tree.next {
        let entries: Vec<String> = words
            .iter()
            .map(|(word, help)| match help.help.is_empty() {
                true => word.clone(),
                false => format!("{word}:{}", help.help),
            })
            .collect();
        let _ = writeln!(out, "  {} {}", quoted(at), quoted(&entries.join("\n")));
    }
    out.push_str(")\n_lnk_values=(\n");
    for (at, words) in &tree.next {
        for (word, help) in words.iter().filter(|(_, w)| !w.values.is_empty()) {
            let key = format!("{at} {word}");
            let _ = writeln!(out, "  {} {}", quoted(&key), quoted(&help.values.join(" ")));
        }
    }
    out.push_str(ZSH_BODY);
    out
}

/// What reads the tables in bash: the words typed so far, past flags,
/// down the tree, then the next words or the typed flag's values.
const BASH_BODY: &str = r#")

_lnk() {
  local cur="${COMP_WORDS[COMP_CWORD]}" prev="" at="${COMP_WORDS[0]##*/}" word i
  ((COMP_CWORD > 1)) && prev="${COMP_WORDS[COMP_CWORD - 1]}"
  for ((i = 1; i < COMP_CWORD; i++)); do
    word="${COMP_WORDS[i]}"
    [[ $word == -* ]] && continue
    if [[ -n ${_lnk_next["$at $word"]+set} ]]; then
      at="$at $word"
    fi
  done
  if [[ $prev == -* && -n ${_lnk_values["$at $prev"]+set} ]]; then
    mapfile -t COMPREPLY < <(compgen -W "${_lnk_values["$at $prev"]}" -- "$cur")
    return
  fi
  mapfile -t COMPREPLY < <(compgen -W "${_lnk_next["$at"]}" -- "$cur")
}
complete -o default -F _lnk lnk
"#;

/// What reads the tables in zsh: as bash's, each word with its help.
const ZSH_BODY: &str = r#")

_lnk() {
  local at="${words[1]:t}" word prev="${words[CURRENT-1]}" i
  local -a next
  for ((i = 2; i < CURRENT; i++)); do
    word="${words[i]}"
    [[ $word == -* ]] && continue
    (( ${+_lnk_next[$at $word]} )) && at="$at $word"
  done
  if [[ $prev == -* ]] && (( ${+_lnk_values[$at $prev]} )); then
    local -a values=(${=_lnk_values[$at $prev]})
    compadd -a values
    return
  fi
  next=("${(@f)_lnk_next[$at]}")
  _describe -t commands 'lnk' next
}
compdef _lnk lnk
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hint_names_values_only_as_a_closed_set_of_plain_words() {
        assert_eq!(choices("<off|low|high>"), ["off", "low", "high"]);
        assert!(choices("<duration>").is_empty());
        assert!(choices("'{ … }'...").is_empty());
        assert!(choices("<A|b>").is_empty());
    }

    #[test]
    fn a_quote_is_closed_escaped_and_reopened() {
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }
}
