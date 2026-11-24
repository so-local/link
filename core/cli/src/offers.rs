//! Offering what goes with a module: asked once, said once, and
//! remembered.

use link_format::{List, Note, Question};

use super::*;

/// What this machine refused of what goes with a module, and what it was
/// told of without a terminal (`~/.lnk/safe/modules/offers.toml`): neither is
/// offered again but by `lnk module add`.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Offered {
    #[serde(default)]
    declined: Vec<String>,
    #[serde(default)]
    told: Vec<String>,
}

link_files::specification!(Offered, include_str!("offers.schema.toml"));

impl Offered {
    fn path() -> PathBuf {
        link_files::layout::Hidden::ModuleOffers.path()
    }

    fn load() -> Offered {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| link_files::saved::parse(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> anyhow::Result<()> {
        link_files::create_private_dir(&link_files::installation_dir())?;
        link_files::saved::write(&Self::path(), self)?;
        Ok(())
    }
}

/// What [`offer_goes_with`] does with what goes with a module and isn't
/// installed (`left`).
#[derive(Debug, PartialEq)]
enum Offering<'a> {
    /// Asks, in a terminal, about these.
    Ask(Vec<&'a str>),
    /// Says, once, what adds these.
    Say(Vec<&'a str>),
    Nothing,
}

/// Of `left`, what's asked about in a `terminal` (all but the `declined`),
/// else said (all not `told` already).
fn offering<'a>(left: &[&'a str], offered: &Offered, terminal: bool) -> Offering<'a> {
    let not_in = |list: &[String]| -> Vec<&'a str> {
        left.iter()
            .copied()
            .filter(|p| !list.iter().any(|l| l == p))
            .collect()
    };
    let refused = not_in(&offered.declined);
    match terminal {
        true if !refused.is_empty() => Offering::Ask(refused),
        false => {
            let unsaid: Vec<&str> = refused
                .into_iter()
                .filter(|p| !offered.told.iter().any(|s| s == p))
                .collect();
            match unsaid.is_empty() {
                true => Offering::Nothing,
                false => Offering::Say(unsaid),
            }
        }
        true => Offering::Nothing,
    }
}

/// Offers what goes with `module` ([`goes_with`]) and isn't installed,
/// the first time a command of it is typed, a line each with what it is:
/// one question in a terminal, a refusal remembered; without one, the
/// command that adds them, once, which isn't a refusal. Never for a
/// module's own `lnk` underneath.
pub(super) fn offer_goes_with(out: &mut Out, module: &str) -> Result<(), Report> {
    if std::env::var_os(link_core::LNK_EXE).is_some() {
        return Ok(());
    }
    let left: Vec<&str> = goes_with(module)
        .iter()
        .map(|(p, _)| *p)
        .filter(|p| !link_core::module_present(p))
        .collect();
    if left.is_empty() {
        return Ok(());
    }
    let mut offered = Offered::load();
    match offering(&left, &offered, out.can_ask()) {
        Offering::Ask(names) => {
            out.now(goes_with_section(module, &names));
            let yes = out.ask(Question::confirm(
                sentence!("Install {}?", it_or_them(names.len())),
                true,
            ))?;
            if yes {
                let done = wait(add(out, &names))?;
                absorb(out, done);
                return Ok(());
            }
            offered.declined.extend(names.iter().map(|n| n.to_string()));
            offered.save()?;
            out.block(Note(sentence!(
                "Not asked again: {} adds {} later.",
                module_add(&names),
                it_or_them(names.len())
            )));
        }
        Offering::Say(names) => {
            out.now(goes_with_section(module, &names));
            out.block(Note(sentence!(
                "{} adds {}.",
                module_add(&names),
                it_or_them(names.len())
            )));
            offered.told.extend(names.iter().map(|n| n.to_string()));
            offered.save()?;
        }
        Offering::Nothing => {}
    }
    Ok(())
}

/// Those of `names` that go with `module`, a bullet each with what it
/// is, under a heading naming `module`.
fn goes_with_section(module: &str, names: &[&str]) -> Section {
    let mut list = List::new(sentence!("Nothing goes with it."));
    for (p, about) in goes_with(module).iter().filter(|(p, _)| names.contains(p)) {
        list = list.item(prose!(
            "{}: {}.",
            module_name(p),
            Value::from(about.to_string())
        ));
    }
    Section::new(phrase!("What goes with the {} module", module_name(module))).with(list)
}

/// "it" for one module, "them" for more.
pub(super) fn it_or_them(n: usize) -> &'static str {
    match n {
        1 => "it",
        _ => "them",
    }
}

/// Whether this `lnk` was typed, by a person or their script, with no
/// terminal to ask in: what it needs is then installed unasked, as the
/// command named the feature. One a module runs (it has `LNK_EXE`) is
/// only said: a module installs what it needs itself
/// (`link_core::ensure`), and a check it makes installs nothing.
pub(super) fn typed_unasked() -> bool {
    let terminal = std::io::stdin().is_terminal() && link_format::streams::stderr_is_terminal();
    !terminal && std::env::var_os(link_core::LNK_EXE).is_none()
}

/// When `lnk <group>`'s module is missing: offers to install it, in a
/// terminal. Whether it was installed.
pub(super) fn offer(out: &mut Out, group: &str, module: &str) -> Result<bool, Report> {
    if !out.can_ask() {
        return Ok(false);
    }
    // The `lnk up` choice it's part of, with what goes with it.
    let (prompt, modules) = match up_choice(module) {
        Some((choice, modules)) => (
            sentence!(
                "{} is part of {}, which isn't installed. Install {}?",
                lnk_command([group]),
                lnk_command(["up", choice]),
                modules_value(&missing(&with_goes_with(modules))?)
            ),
            with_goes_with(modules),
        ),
        None => (
            sentence!(
                "{} is part of the {} module, which isn't installed. Install {}?",
                lnk_command([group]),
                module_name(module),
                modules_value(&missing(&[module])?)
            ),
            vec![module],
        ),
    };
    if !out.ask(Question::confirm(prompt, true))? {
        return Ok(false);
    }
    let done = wait(add(out, &modules))?;
    absorb(out, done);
    Ok(true)
}

#[cfg(test)]
mod tests {

    #[test]
    fn what_goes_with_a_module_is_asked_once_and_said_once() {
        use super::{Offered, Offering, offering};
        let left = ["storage", "sandbox"];
        let none = Offered::default();
        assert_eq!(
            offering(&left, &none, true),
            Offering::Ask(vec!["storage", "sandbox"])
        );
        assert_eq!(
            offering(&left, &none, false),
            Offering::Say(vec!["storage", "sandbox"])
        );
        // Refused: never asked again, nor said.
        let refused = Offered {
            declined: vec!["storage".into()],
            told: Vec::new(),
        };
        assert_eq!(
            offering(&left, &refused, true),
            Offering::Ask(vec!["sandbox"])
        );
        assert_eq!(
            offering(&left, &refused, false),
            Offering::Say(vec!["sandbox"])
        );
        // Said without a terminal: not said again, still asked in one.
        let said = Offered {
            declined: Vec::new(),
            told: vec!["storage".into(), "sandbox".into()],
        };
        assert_eq!(offering(&left, &said, false), Offering::Nothing);
        assert_eq!(
            offering(&left, &said, true),
            Offering::Ask(vec!["storage", "sandbox"])
        );
    }
}
