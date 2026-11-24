//! Located terminal invocations for source inventories. Nothing here executes shell.

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Invocation {
    pub section: String,
    pub block: usize,
    pub line: usize,
    pub ordinal: usize,
    pub source: String,
    pub argv: Vec<String>,
    pub reject: bool,
}

#[derive(Default, Debug, Serialize)]
pub struct Discovery {
    pub invocations: Vec<Invocation>,
    pub problems: Vec<String>,
    pub sections: Vec<SourceSection>,
    pub exclusions: Vec<TerminalExclusion>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceSection {
    pub heading: String,
    /// Instructions, setup, example files, and terminal fences in source order.
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalExclusion {
    pub section: String,
    pub block: usize,
    pub line: usize,
    pub reason: ExclusionReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExclusionReason {
    Output,
    ShellIntrospection,
    CargoArtifactSelection,
}

/// Keep compact alternatives in text examples distinct from actual shell pipes.
/// Quoted and escaped pipes always remain literal arguments.
fn alternatives(text: &str) -> String {
    let chars: Vec<_> = text.chars().collect();
    let mut quote = None;
    let mut escaped = false;
    chars
        .iter()
        .enumerate()
        .map(|(at, &ch)| {
            if escaped {
                escaped = false;
            } else if ch == '\\' && quote != Some('\'') {
                escaped = true;
            } else if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() && matches!(ch, '\'' | '"') {
                quote = Some(ch);
            } else if quote.is_none()
                && ch == '|'
                && at > 0
                && chars
                    .get(at + 1)
                    .is_some_and(|next| next.is_ascii_alphanumeric())
                && chars[at - 1].is_ascii_alphanumeric()
            {
                return '\u{e000}';
            }
            ch
        })
        .collect()
}

fn expand(argv: Vec<String>) -> Vec<Vec<String>> {
    let mut branches = vec![Vec::new()];
    for word in argv {
        branches = branches
            .into_iter()
            .flat_map(|branch| {
                word.split('\u{e000}').map(move |value| {
                    let mut next = branch.clone();
                    next.push(value.to_owned());
                    next
                })
            })
            .collect();
    }
    branches
}

fn collect(
    found: &mut Discovery,
    section: &str,
    block: usize,
    start: usize,
    info: &str,
    body: &str,
) {
    let language = info.split_whitespace().next().unwrap_or("");
    if !matches!(
        language,
        "sh" | "bash" | "zsh" | "shell" | "console" | "text"
    ) {
        return;
    }
    if info.split_whitespace().any(|word| word == "output") {
        found.exclusions.push(TerminalExclusion {
            section: section.into(),
            block,
            line: start,
            reason: ExclusionReason::Output,
            command: None,
        });
        return;
    }
    let reject = info.split_whitespace().any(|word| word == "expect=error");
    let mut continued = String::new();
    let mut reviewed = String::new();
    let mut beginning = start;
    let mut command_ordinal = 0;
    for (offset, source) in body.lines().enumerate() {
        let initial = continued.is_empty();
        let source = if initial { source.trim_start() } else { source };
        if initial {
            beginning = start + offset;
        }
        let source = if initial {
            source
                .strip_prefix("$ ")
                .or_else(|| source.strip_prefix("% "))
                .unwrap_or(source)
        } else {
            source
        };
        if initial && (source.starts_with('#') || source.trim().is_empty()) {
            continue;
        }
        continued.push_str(source);
        reviewed.push_str(source.trim());
        let (quote, escaped) = super::shell_state(&continued);
        if escaped && quote != Some('\'') {
            continued.pop();
            reviewed.pop();
            reviewed.push(' ');
            continue;
        }
        if shlex::split(&continued).is_none() {
            continued.push('\n');
            reviewed.push('\n');
            continue;
        }
        let input = if language == "text" {
            alternatives(&continued)
        } else {
            continued.clone()
        };
        match super::parsed_line(&input, true, reject) {
            Ok(parsed) => {
                for (reason, command) in parsed.exclusions {
                    found.exclusions.push(TerminalExclusion {
                        section: section.into(),
                        block,
                        line: beginning,
                        reason,
                        command: Some(command),
                    });
                }
                for example in parsed.examples {
                    let ordinal = command_ordinal;
                    command_ordinal += 1;
                    for argv in expand(example.argv) {
                        found.invocations.push(Invocation {
                            section: section.to_owned(),
                            block,
                            line: beginning,
                            ordinal,
                            source: reviewed.clone(),
                            argv,
                            reject,
                        });
                    }
                }
            }
            Err(error) => found.problems.push(format!("line {beginning}: {error}")),
        }
        continued.clear();
        reviewed.clear();
    }
    if !continued.is_empty() && super::mentions_link(&continued) {
        found.problems.push(format!(
            "line {beginning}: unfinished terminal quote or continuation"
        ));
    }
}

/// Read complete terminal fences, retaining sections, source lines and alternatives.
/// Unclosed fences remain diagnostics rather than disappearing from the inventory.
pub fn terminal_commands(text: &str) -> Discovery {
    let mut found = Discovery::default();
    let mut headings: Vec<(usize, String)> = Vec::new();
    let mut opened: Option<(char, usize, String, usize)> = None;
    let mut blocks = std::collections::BTreeMap::<String, usize>::new();
    let mut block = 0;
    let mut section = SourceSection {
        heading: String::new(),
        source: String::new(),
    };
    let mut body = String::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let marker = trimmed.chars().next().unwrap_or(' ');
        let width = trimmed.chars().take_while(|ch| *ch == marker).count();
        if let Some((fence, length, info, start)) = &opened {
            if indent <= 3
                && marker == *fence
                && width >= *length
                && trimmed[width..].trim().is_empty()
            {
                let section = headings
                    .iter()
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ");
                collect(&mut found, &section, block, *start, info, &body);
                opened = None;
                body.clear();
            } else {
                body.push_str(line);
                body.push('\n');
            }
        } else if indent <= 3 && matches!(marker, '`' | '~') && width >= 3 {
            let count = blocks.entry(section.heading.clone()).or_default();
            *count += 1;
            block = *count;
            opened = Some((marker, width, trimmed[width..].trim().to_owned(), index + 2));
        } else if marker == '#' && width <= 6 && trimmed[width..].starts_with(' ') {
            headings.retain(|(level, _)| *level < width);
            headings.push((width, trimmed[width..].trim().to_owned()));
            found.sections.push(section);
            let heading = headings
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join(" / ");
            if found.sections.iter().any(|s| s.heading == heading) {
                found.problems.push(format!(
                    "line {}: duplicate heading path {heading}",
                    index + 1
                ));
            }
            section = SourceSection {
                heading,
                source: String::new(),
            };
        }
        section.source.push_str(line);
        section.source.push('\n');
    }
    found.sections.push(section);
    if let Some((_, _, _, start)) = opened {
        found
            .problems
            .push(format!("line {start}: unclosed fenced block"));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_strace_separator_preserves_only_the_inner_link_arguments() {
        let found = terminal_commands(
            "```sh\nstrace -f -qq -e trace=execve -e signal=none -o trace.txt -- lnk agent list\n```\n",
        );
        assert!(found.problems.is_empty(), "{found:?}");
        assert_eq!(found.invocations.len(), 1);
        assert_eq!(found.invocations[0].argv, ["lnk", "agent", "list"]);
        for command in [
            "strace -f lnk agent list",
            "sudo strace -f lnk agent list",
            "strace -f -- unrelated lnk agent list",
        ] {
            let found = terminal_commands(&format!("```sh\n{command}\n```\n"));
            assert!(!found.problems.is_empty(), "{command}: {found:?}");
            assert!(found.invocations.is_empty());
        }
    }

    #[test]
    fn cargo_artifact_names_are_excluded_without_hiding_a_following_link_invocation() {
        for operation in ["build", "check", "test", "clippy"] {
            let source = format!(
                "# Build\n```sh\ncargo {operation} --manifest-path ../../../link-public/core/cli/Cargo.toml --bin lnk --target-dir target && lnk agent list\n```\n"
            );
            let found = terminal_commands(&source);
            assert!(found.problems.is_empty(), "{found:?}");
            assert_eq!(found.invocations.len(), 1);
            assert_eq!(found.invocations[0].argv, ["lnk", "agent", "list"]);
            assert_eq!(found.exclusions.len(), 1);
            assert!(matches!(
                found.exclusions[0].reason,
                ExclusionReason::CargoArtifactSelection
            ));
            assert_eq!(found.exclusions[0].command.as_ref().unwrap()[1], operation);
        }
        let equal = terminal_commands("```sh\ncargo build --bin=lnk\n```\n");
        assert!(equal.problems.is_empty());
        assert_eq!(equal.exclusions.len(), 1);
        for command in [
            "cargo run --bin lnk",
            "cargo build --bin lnk lnk agent list",
            "cargo test --bin lnk -- lnk agent list",
            "cargo custom --bin lnk",
            "cargo build --unknown lnk --bin lnk",
            "cargo build --bin lnk --target-dir $(lnk agent list)",
        ] {
            let found = terminal_commands(&format!("```sh\n{command}\n```\n"));
            assert!(!found.problems.is_empty(), "{command}: {found:?}");
            assert!(found.exclusions.is_empty(), "{command}: {found:?}");
        }
    }

    #[test]
    fn bounded_shell_lookups_are_explicit_exclusions_and_other_commands_still_execute() {
        for command in [
            "type -a lnk",
            "type -- lnk",
            "which -a lnk",
            "command -v lnk",
            "command -p -V -- lnk",
        ] {
            let found =
                terminal_commands(&format!("# Setup\n```sh\n{command}; lnk agent list\n```\n"));
            assert!(found.problems.is_empty(), "{command}: {found:?}");
            assert_eq!(found.invocations.len(), 1);
            assert_eq!(found.invocations[0].argv, ["lnk", "agent", "list"]);
            assert_eq!(found.exclusions.len(), 1);
            assert!(matches!(
                found.exclusions[0].reason,
                ExclusionReason::ShellIntrospection
            ));
            assert!(
                found.exclusions[0]
                    .command
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|word| word == "lnk")
            );
            assert_eq!(found.exclusions[0].section, "Setup");
        }
        for command in ["type -x lnk", "which -x lnk", "command -v -x lnk"] {
            let found = terminal_commands(&format!("```sh\n{command}\n```\n"));
            assert!(!found.problems.is_empty(), "{command}: {found:?}");
            assert!(found.exclusions.is_empty());
        }
        let actual = terminal_commands("```sh\ncommand lnk agent list\n```\n");
        assert_eq!(actual.invocations.len(), 1);
        assert!(actual.exclusions.is_empty());
    }

    #[test]
    fn multiline_quoted_values_and_declared_output_keep_their_distinct_roles() {
        let source = "# Task\n```sh\nlnk thing go --text 'first line\n  second line'\n```\n```text output\nthis bot's messages mention lnk thing go\n```\n";
        let found = terminal_commands(source);
        assert!(found.problems.is_empty(), "{found:?}");
        assert_eq!(found.invocations.len(), 1);
        assert_eq!(
            found.invocations[0].argv,
            ["lnk", "thing", "go", "--text", "first line\n  second line"]
        );
        assert_eq!(found.invocations[0].line, 3);
        assert_eq!(found.exclusions.len(), 1);
        assert_eq!(found.exclusions[0].block, 2);
        assert!(super::super::read(source).is_ok());
    }

    #[test]
    fn unsupported_link_constructs_stay_visible_instead_of_empty_coverage() {
        for command in [
            "if true; then lnk thing go; fi",
            "for id in one; do lnk thing go; done",
            "result=$(lnk thing go)",
            "echo $(lnk thing go)",
            "result=`lnk thing go`",
            "result=`lnk`",
            "sh -c 'lnk thing go'",
            "bash -lc 'lnk thing go'",
            "eval 'lnk thing go'",
        ] {
            let found = terminal_commands(&format!("# Task\n```sh\n{command}\n```\n"));
            assert!(!found.problems.is_empty(), "{command}: {found:?}");
        }
        let outer = terminal_commands("```sh\nlnk thing go -- sh -c 'echo hello'\n```\n");
        assert!(outer.problems.is_empty(), "{outer:?}");
        assert_eq!(outer.invocations.len(), 1);
        for command in [
            "echo lnk",
            "printf 'lnk thing go'",
            "echo '$(lnk thing go)'",
            "echo ignored # $(lnk thing go)",
            "unrelated # lnk thing go",
        ] {
            let found = terminal_commands(&format!("```sh\n{command}\n```\n"));
            assert!(found.invocations.is_empty(), "{command}: {found:?}");
            assert!(found.problems.is_empty(), "{command}: {found:?}");
        }
    }

    #[test]
    fn command_ordinals_and_section_local_blocks_survive_unrelated_line_moves() {
        let task = "# Task\n```sh\nLANG=C lnk box list\nlnk cloud list | lnk storage list\necho lnk is mentioned here\n```\n";
        let found = terminal_commands(task);
        assert_eq!(
            found
                .invocations
                .iter()
                .map(|i| (i.block, i.ordinal))
                .collect::<Vec<_>>(),
            [(1, 0), (1, 1), (1, 2)]
        );
        let moved = terminal_commands(&format!("# Unrelated\n```sh\nlnk version\n```\n\n{task}"));
        assert_eq!(
            moved
                .invocations
                .iter()
                .filter(|i| i.section == "Task")
                .map(|i| (i.block, i.ordinal))
                .collect::<Vec<_>>(),
            [(1, 0), (1, 1), (1, 2)]
        );
        assert!(
            found
                .sections
                .iter()
                .any(|s| s.heading == "Task" && s.source.contains("echo lnk"))
        );
        assert!(
            !terminal_commands(
                "# Same\n```sh\nlnk box list\n```\n# Same\n```sh\nlnk cloud list\n```\n"
            )
            .problems
            .is_empty()
        );
    }

    #[test]
    fn alternatives_and_shell_pipes_have_different_meanings() {
        let source = "# Cloud\n```text\nlnk cloud connect aws|gcp\n```\n```sh\nlnk cloud list|lnk box list\nlnk box run -- echo 'aws|gcp'\n```\n";
        let found = terminal_commands(source);
        assert!(found.problems.is_empty());
        let argv: Vec<_> = found.invocations.iter().map(|c| c.argv.join(" ")).collect();
        assert_eq!(
            argv,
            [
                "lnk cloud connect aws",
                "lnk cloud connect gcp",
                "lnk cloud list",
                "lnk box list",
                "lnk box run -- echo aws|gcp"
            ]
        );
        assert_eq!(found.invocations[0].line, 3);
        assert_eq!(found.invocations[0].section, "Cloud");
    }

    #[test]
    fn split_steps_prompts_and_files_preserve_their_locations() {
        let prose = terminal_commands("```text\nplan/\n  week.md  this week's plan\n```\n");
        assert!(prose.problems.is_empty());
        assert!(prose.invocations.is_empty());
        let source = "# Task\n~~~console\n$ lnk box create \\\n --cloud aws\n~~~\n```toml\nname='web'\n```\n```zsh expect=error\n% lnk box ssh absent\n```\n";
        let found = terminal_commands(source);
        assert!(found.problems.is_empty());
        assert_eq!(found.invocations.len(), 2);
        assert_eq!(
            found.invocations[0].argv,
            ["lnk", "box", "create", "--cloud", "aws"]
        );
        assert_eq!(found.invocations[1].block, 3);
        assert!(found.invocations[1].reject);
        assert!(
            !terminal_commands("```sh\nlnk box create \\\n")
                .problems
                .is_empty()
        );
    }
}
