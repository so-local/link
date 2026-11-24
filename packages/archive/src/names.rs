//! The name each imported thing takes here: its own, or one you give,
//! never one already taken. A terminal is asked; without one, a name
//! that's taken refuses the import.

use anyhow::{bail, ensure};

/// How import asks you, in a terminal.
pub trait Ask {
    /// A yes or no question, no when unanswered.
    fn confirm(&mut self, question: &str) -> anyhow::Result<bool>;
    /// A line answered to `prompt`.
    fn line(&mut self, prompt: &str) -> anyhow::Result<String>;
    /// Something to tell before asking again.
    fn say(&mut self, text: &str);
}

/// The name thing `name` of kind `kind` takes here: `given` when the
/// command named one; else asked, when there's a terminal; else its
/// own, unless that's taken.
pub fn choose(
    kind: &str,
    name: &str,
    given: Option<&str>,
    taken: &dyn Fn(&str) -> bool,
    ask: Option<&mut dyn Ask>,
) -> anyhow::Result<String> {
    if let Some(given) = given {
        link_machine::machine::check_name(given)?;
        ensure!(
            !taken(given),
            "a {kind} here is named {given} already: name it otherwise in --select"
        );
        return Ok(given.into());
    }
    let Some(ask) = ask else {
        ensure!(
            !taken(name),
            "this {kind} is named {name}, which one of yours already is: \
             --select {kind}.{name}={kind}.<another> imports it under another name"
        );
        return Ok(name.into());
    };
    if taken(name) {
        ask.say(&format!(
            "This {kind} is named {name}, which one of yours already is."
        ));
    } else if !ask.confirm(&format!("This {kind} is named {name}. Rename it?"))? {
        return Ok(name.into());
    }
    loop {
        let answer = ask.line("Name: ")?;
        let answer = answer.trim();
        if answer.is_empty() {
            bail!("no name was given, so nothing was imported");
        }
        match link_machine::machine::check_name(answer) {
            Err(err) => ask.say(&format!("{err:#}.")),
            Ok(()) if taken(answer) => {
                ask.say(&format!("A {kind} here is named {answer} already."))
            }
            Ok(()) => return Ok(answer.into()),
        }
    }
}
