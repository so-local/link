//! The agent's model: chosen on its first run, its fallback, and
//! whether it can be reached from here.

use link_agent::settings::{self, KEY_VARS_TEXT, Settings};
use link_format::{
    Address, Done, Exit, Name, Note, Out, Path, Phrase, Product, Question, Report, Sentence, Value,
    WithinExt, cmd, phrase, prose, sentence,
};
use link_harness_contract::harness::{Model, Runtime};
use link_model_contract::selection::{Provider, Route};
use link_secretive::Secret;

use crate::permissions::YES;
use crate::status::port_answers;
use crate::{absolute, expand};
mod validation;

/// Persist an exact provider ID, then apply it to a running agent.
pub(crate) async fn set(
    out: &mut Out,
    named: Option<&str>,
    args: &link_agent::cli::ModelSetArgs,
) -> Result<Done, Report> {
    let mut agent = Settings::resolve(named)?;
    if !agent.path().exists() {
        return Err(Report::error(sentence!("There is no agent yet."))
            .exit(Exit::Usage)
            .next(
                cmd!("lnk agent start --model.default <model>"),
                "set one up",
            ));
    }
    first_run(out, &mut agent, Some(args.model.trim().to_string()), None).await?;
    agent.save()?;
    crate::granting::restart_agent(out, &mut agent).await?;
    let route = agent
        .model
        .as_ref()
        .and_then(|model| Route::of_saved(&model.provider, model.name.as_deref()?));
    Ok(Done::success(match route {
        Some(route) => sentence!(
            "The agent's model is now {}, from {}.",
            Name::new("model", route.to_string()),
            Product::new(route.provider.title())
        ),
        None => sentence!(
            "The agent's model is now {}.",
            Name::new("model", args.model.trim())
        ),
    }))
}

/// Asks for what's missing: the model (one running here, or a key for a
/// hosted one), and the files folder.
pub(crate) async fn first_run(
    out: &mut Out,
    settings: &mut Settings,
    model: Option<String>,
    spec: Option<&link_agent_spec::Resolved>,
) -> Result<(), Report> {
    let selecting = model.is_some() || settings.model.is_none();
    match model {
        // A model's name says who serves it (`local/qwen3:8b`,
        // `openai/gpt-6.1-sol`): one here is looked for here, and a hosted
        // one takes its own provider's key, from its variable, the key
        // saved for that provider, or asked for.
        Some(wanted) => {
            let route = placed(routed(&wanted)?);
            settings.model = Some(match route.provider {
                Provider::Local => here(out, &route)?,
                provider => {
                    let key =
                        match settings::key_for_provider(provider.id(), settings.model.as_ref()) {
                            Some(key) => key,
                            None => ask_key(out, provider.id(), &route.to_string())?,
                        };
                    hosted_at(&route, &key)?
                }
            });
            said(out, &wanted, &route);
        }
        None => {
            match &mut settings.model {
                // Its provider's variable renews its key.
                Some(model) if !model.is_local() => {
                    if let Some(key) = settings::key_from_env(Some(model))? {
                        model.api_key = key;
                    }
                }
                // A local model, and a hosted provider's key set: never
                // ignored. In a terminal it's asked whether to switch; else
                // said, with how.
                Some(model) => {
                    if let Ok(Some(key)) = settings::key_from_env(None)
                        && let Some(provider) = settings::provider_of(key.expose())
                    {
                        let var = settings::key_var(provider).unwrap_or("its variable");
                        let now = model.name.as_deref().unwrap_or("a local model").to_string();
                        let to = hosted(&key, None)?;
                        let named = validation::name(&to, spec)?.unwrap_or_default();
                        let full = Route::of_saved(&to.provider, &named)
                            .map_or_else(|| named.clone(), |route| route.to_string());
                        let switch = may_ask()
                            && !YES.load(std::sync::atomic::Ordering::Relaxed)
                            && out.ask(Question::confirm(
                                sentence!(
                                    "{} is set. Think with {} from {} instead of {}?",
                                    var.to_string(),
                                    Name::new("model", &named),
                                    Product::new(provider_name(provider)),
                                    Name::new("model", &now)
                                ),
                                true,
                            ))?;
                        match switch {
                        true => *model = to,
                        false => out.block(Note(sentence!(
                            "{} is set, but your agent thinks with {} here, and {} switches it.",
                            var.to_string(),
                            Name::new("model", &now),
                            cmd!("lnk agent start --model.default {}", Name::new("model", &full))
                        ))),
                    }
                    }
                }
                None => {
                    settings.model = Some(match settings::key_from_env(None)? {
                        Some(key) => hosted(&key, None)?,
                        None => choose(out, &settings.name)?,
                    });
                }
            }
        }
    }
    if selecting && let Some(model) = &settings.model {
        validation::selected(model, spec).await?;
    }
    if settings.files.is_none() {
        let default = settings.files();
        let keep = Question::confirm(
            sentence!(
                "Your agent works in {}. Keep that folder?",
                Path::new(&default)
            ),
            true,
        )
        .why(prose!("The field flag --run.files picks another."))
        .unasked(sentence!(
            "Your agent works in {}, and --run.files picks another.",
            Path::new(&default)
        ));
        settings.files = Some(match out.ask(keep)? {
            true => default,
            false => {
                let dir = out.ask(
                    Question::text(sentence!("Which folder does your agent work in?"))
                        .flag("--run.files"),
                )?;
                absolute(expand(&dir))?
            }
        });
    }
    Ok(())
}

/// Whether anyone can be asked a question: a terminal in and out.
fn may_ask() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && link_format::streams::stderr_is_terminal()
}

/// Its fallback, as its spec's `[model] fallback_model` and `fallback`
/// say: a hosted model named as `model.default` names one, its key
/// [`FALLBACK_CREDENTIAL`], else as the model's is found. Refused where its
/// harness couldn't be handed it ([`Settings::fallback_unwired`]).
pub(crate) fn set_fallback(out: &mut Out, settings: &mut Settings) -> Result<(), Report> {
    let spec = settings.spec()?.spec;
    let model = spec.model.clone().unwrap_or_default();
    // `[model] fallback = false`: none, whatever it had.
    let falls_back = model.fallback != Some(false);
    if !falls_back {
        settings.fallback = None;
    }
    // An alias of `[model.aliases]` names the model it stands for.
    let wanted = model
        .fallback_model
        .filter(|_| falls_back)
        .map(|name| spec.model_named(&name));
    if let Some(wanted) = wanted.as_ref() {
        let route = placed(routed(wanted)?);
        if route.provider == Provider::Local {
            return Err(Report::error(sentence!(
                "{} runs on this machine.",
                Name::new("model", route.to_string())
            ))
            .why(prose!(
                "A fallback is a hosted model, for when this machine's can't answer."
            ))
            .exit(Exit::Usage));
        }
        let given = fallback_key(|var| Secret::from_env(var).map(Secret::into_exposed));
        let key = match given {
            Some(key) => key,
            None => {
                match settings::key_for_provider(route.provider.id(), settings.fallback.as_ref()) {
                    Some(key) => key,
                    None => ask_key(out, route.provider.id(), &route.to_string())?,
                }
            }
        };
        settings.fallback = Some(hosted_at(&route, &key)?);
        said(out, wanted, &route);
    }
    if let Some(why) = settings.fallback_unwired()? {
        return Err(Report::error(sentence!("The fallback can't be used."))
            .why(Sentence::adopt(&why))
            .exit(Exit::Usage));
    }
    Ok(())
}

/// The variable a fallback's key is read from: off the command line,
/// which other users can see.
pub(crate) const FALLBACK_CREDENTIAL: &str = "LNK_FALLBACK_CREDENTIAL";

/// The fallback's key as given: [`FALLBACK_CREDENTIAL`] (from `get`) when it
/// holds one.
pub(crate) fn fallback_key(get: impl Fn(&str) -> Option<String>) -> Option<Secret> {
    get(FALLBACK_CREDENTIAL)
        .map(|key| Secret::from(key.trim().to_string()))
        .filter(|key| !key.is_empty())
}

/// A hosted provider's model, from its key.
pub(crate) fn hosted(key: &Secret, name: Option<String>) -> anyhow::Result<Model> {
    let key = key.expose_map(|k| k.trim().to_string());
    if key.is_empty() {
        anyhow::bail!("no key given: your agent needs a model (set {KEY_VARS_TEXT})");
    }
    let Some(provider) = settings::provider_of(key.expose()) else {
        anyhow::bail!("that doesn't look like an Anthropic, OpenAI or OpenRouter key (sk-...)");
    };
    Ok(Model {
        actions_url: None,
        provider: provider.into(),
        api_key: key,
        name,
        base_url: None,
    })
}

/// `wanted` placed: who serves it, as its name says, or why it can't be.
fn routed(wanted: &str) -> Result<Route, Report> {
    Route::parse(wanted).map_err(|why| {
        Report::error(sentence!(
            "Can't tell where {} runs.",
            Name::typed("model", wanted)
        ))
        .why(Sentence::adopt(&why))
        .exit(Exit::Usage)
    })
}

/// `route` where it goes: a bare name to the model of that name running
/// here, when one runs, else its owner or OpenRouter. A bare name with an
/// owner takes only a model here of exactly its name (or that name with a
/// tag), never one it merely starts, so `gpt-5` never finds `gpt-5-x`.
fn placed(route: Route) -> Route {
    if !route.here_first {
        return route;
    }
    let runs = local_runtimes().ok().is_some_and(|runtimes| {
        find_local(&runtimes, &route.id).is_some_and(|found| {
            !route.owned()
                || found.name.as_deref().is_some_and(|name| {
                    name == route.id
                        || name
                            .split_once(':')
                            .is_some_and(|(base, _)| base == route.id)
                })
        })
    });
    match runs {
        true => route.here(),
        false => route,
    }
}

/// Hosted model `route`, with `key`: refused when the key is another
/// provider's, so the name always says where the model is sent.
fn hosted_at(route: &Route, key: &Secret) -> Result<Model, Report> {
    if let Some(declared) = link_model_contract::providers::declared(route.provider.id()) {
        return declared_at(route, declared, key);
    }
    let model = hosted(key, Some(route.id.clone()))?;
    if model.provider != route.provider.id() {
        let theirs =
            Provider::of_saved(&model.provider).map_or("another provider", Provider::title);
        return Err(Report::error(sentence!(
            "{} is {}'s, and that key is {}'s.",
            Name::new("model", route.to_string()),
            Product::new(route.provider.title()),
            Product::new(theirs)
        ))
        .why(prose!(
            "A model goes only where its name says. Set {}, or name the model with {}'s prefix.",
            settings::key_var(route.provider.id())
                .unwrap_or("its key")
                .to_string(),
            Product::new(theirs)
        ))
        .exit(Exit::Usage));
    }
    Ok(model)
}

/// Model `route` of a provider at an address of its own (`link`): the
/// provider is the route's, never told by the key, and a key of another
/// kind is refused, so another provider's never goes to its address.
fn declared_at(
    route: &Route,
    declared: &link_model_contract::providers::Declared,
    key: &Secret,
) -> Result<Model, Report> {
    let key = key.expose_map(|k| k.trim().to_string());
    if !key.expose().starts_with(&declared.key_prefix) {
        return Err(Report::error(sentence!(
            "That isn't a {} key.",
            Product::new(declared.title.as_str())
        ))
        .why(prose!(
            "{} is {}'s, whose keys start with {}.",
            Name::new("model", route.to_string()),
            Product::new(declared.title.as_str()),
            declared.key_prefix.clone()
        ))
        .exit(Exit::Usage));
    }
    Ok(Model {
        actions_url: None,
        provider: route.provider.id().into(),
        api_key: key,
        name: Some(route.id.clone()),
        base_url: None,
    })
}

/// Local model `route`, found running here.
fn here(out: &mut Out, route: &Route) -> Result<Model, Report> {
    let runtimes = local_runtimes().map_err(|why| {
        Report::error(sentence!(
            "Can't look for {} on this machine.",
            Name::typed("model", route.to_string())
        ))
        .why(Sentence::adopt(&why))
    })?;
    if let Some(local) = find_local(&runtimes, &route.id) {
        chosen_among(out, &runtimes, &local)?;
        return Ok(local);
    }
    let found: Vec<_> = runtimes.iter().flat_map(|r| r.models.clone()).collect();
    let found = match found.is_empty() {
        true => "No models run here".to_string(),
        false => format!("Running here: {}", found.join(", ")),
    };
    Err(Report::error(sentence!(
        "No model named {} runs on this machine.",
        Name::typed("model", &route.id)
    ))
    .why(Sentence::adopt(&format!("{found}.")))
    .exit(Exit::Usage))
}

/// Says where `wanted` goes when its name didn't say in so many words: a
/// bare `claude-...`, or another vendor's model, through OpenRouter.
fn said(out: &mut Out, wanted: &str, route: &Route) {
    if !Route::said(wanted) {
        out.block(Note(sentence!(
            "{} is {}, sent to {}.",
            Name::new("model", wanted.trim()),
            Name::new("model", route.to_string()),
            Product::new(route.provider.title())
        )));
    }
}

/// Asks for provider `provider`'s key for model `wanted`, in a terminal;
/// without one, its variable answers.
pub(crate) fn ask_key(out: &mut Out, provider: &str, wanted: &str) -> Result<Secret, Report> {
    let who = provider_name(provider);
    let mut ask =
        Question::hidden(sentence!("What is your {} API key?", Product::new(who))).why(prose!(
            "{} is {}'s, so your agent needs a key for it, kept with its settings.",
            Name::new("model", wanted),
            Product::new(who)
        ));
    if let Some(var) = settings::key_var(provider) {
        ask = ask.variable(var);
    }
    out.ask(ask)
}

/// The model runtimes on this machine, from the models module (`lnk model list
/// --json`), or why it couldn't say (not installed, ...).
pub(crate) fn local_runtimes() -> Result<Vec<Runtime>, String> {
    let output = link_core::lnk()
        // An installed Ollama that's stopped starts, so its models are
        // offered rather than a download of one.
        .args(["model", "runtime", "list", "--json", "--start"])
        .output()
        .map_err(|err| format!("running lnk model list: {err}"))?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr);
        return Err(link_format::clean(why.trim().trim_start_matches("Error: ")));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("lnk model runtime list --json: {err}"))
}

/// `wanted` among the local models: its exact id, its name without a tag
/// (`qwen3` finds `qwen3:8b`), or else the only id starting with it
/// (`qwen` finds `qwen3.5:9b`). An id two runtimes serve is the one on a
/// runtime the owner added, if one is ([`served_by`] says which serve it).
pub(crate) fn find_local(runtimes: &[Runtime], wanted: &str) -> Option<Model> {
    let all = || {
        let (added, found): (Vec<_>, Vec<_>) = runtimes.iter().partition(|r| r.added);
        added
            .into_iter()
            .chain(found)
            .flat_map(|r| r.models.iter().map(move |m| (r, m)))
    };
    let prefixed: Vec<_> = all().filter(|(_, m)| m.starts_with(wanted)).collect();
    let (runtime, id) = all()
        .find(|(_, m)| *m == wanted)
        .or_else(|| all().find(|(_, m)| m.split_once(':').is_some_and(|(base, _)| base == wanted)))
        .or(match prefixed.as_slice() {
            [only] => Some(*only),
            _ => None,
        })?;
    Some(local_model(runtime, id))
}

/// The runtimes here that serve `id`.
pub(crate) fn served_by<'a>(runtimes: &'a [Runtime], id: &str) -> Vec<&'a Runtime> {
    runtimes
        .iter()
        .filter(|r| r.models.iter().any(|m| m == id))
        .collect()
}

/// Whether `model`, found among `runtimes` for `wanted`, is one of
/// several runtimes' of that id: taken from the one the owner added, and
/// said; refused, naming each, where none or several of them were.
fn chosen_among(out: &mut Out, runtimes: &[Runtime], model: &Model) -> Result<(), Report> {
    let id = model.name.as_deref().unwrap_or_default();
    let serving = served_by(runtimes, id);
    if serving.len() < 2 {
        return Ok(());
    }
    let each = Value::List(
        serving
            .iter()
            .map(|r| Value::from(format!("{} at {}", r.name, r.base_url)))
            .collect(),
    );
    match serving.iter().filter(|r| r.added).count() {
        1 => {
            out.block(Note(sentence!(
                "{} runs on {} here, so Link takes the one you added, at {}.",
                Name::new("model", id),
                each,
                Address::new(model.base_url.as_deref().unwrap_or_default())
            )));
            Ok(())
        }
        _ => Err(Report::error(sentence!(
            "{} runs on {} here, and Link can't tell which you mean.",
            Name::new("model", id),
            each
        ))
        .next(cmd!("lnk model runtime add <url>"), "add the one you want")
        .exit(Exit::Usage)),
    }
}

pub(crate) fn local_model(runtime: &Runtime, id: &str) -> Model {
    Model {
        actions_url: None,
        provider: if runtime.name == "Ollama" {
            "ollama"
        } else {
            "local"
        }
        .into(),
        api_key: Secret::default(),
        name: Some(id.to_string()),
        base_url: Some(runtime.base_url.clone()),
    }
}

/// Offers the models running here, else asks for a key.
fn choose(out: &mut Out, named: &str) -> Result<Model, Report> {
    let runtimes = local_runtimes().unwrap_or_else(|why| {
        let note = match why.contains("(lnk-model), which isn't installed") {
            true => sentence!("There are no local models yet, and lnk up models adds them."),
            false => sentence!("Link isn't looking for models on this machine."),
        };
        out.block(Note(note));
        if !why.contains("(lnk-model), which isn't installed") {
            out.block(Note(Sentence::adopt(&why)));
        }
        Vec::new()
    });
    let local: Vec<_> = runtimes
        .iter()
        .flat_map(|r| r.models.iter().map(move |m| (r, m)))
        .collect();
    let key = || -> (Value, Phrase) {
        (
            Value::from("an API key"),
            phrase!("from Anthropic, OpenAI or OpenRouter"),
        )
    };
    let ask_for_key = |out: &mut Out| -> Result<Model, Report> {
        let key = out.ask(
            Question::hidden(sentence!("What is your API key?"))
                .why(prose!("Paste one from Anthropic, OpenAI or OpenRouter."))
                .variable("ANTHROPIC_API_KEY"),
        )?;
        Ok(hosted(&key, None)?)
    };
    if local.is_empty() {
        if let Some(pick) = recommended() {
            let options = vec![
                (
                    Value::from(pick.clone()),
                    phrase!("free and private on this Mac, installed with Ollama"),
                ),
                key(),
            ];
            let picked = ask_model(out, named, options)?;
            return match picked {
                0 => install_local(out, &pick),
                _ => ask_for_key(out),
            };
        }
        return ask_for_key(out);
    }
    let mut options: Vec<(Value, Phrase)> = local
        .iter()
        .map(|(runtime, model)| {
            (
                Value::from(model.to_string()),
                phrase!("on {}, free and private", Product::new(&runtime.name)),
            )
        })
        .collect();
    options.push(key());
    let picked = ask_model(out, named, options)?;
    match local.get(picked) {
        Some((runtime, id)) => Ok(local_model(runtime, id)),
        None => ask_for_key(out),
    }
}

/// Model selection also runs inside team lifecycle commands, which have
/// no model flag. Recovery always names the individual agent's command.
fn ask_model(out: &mut Out, named: &str, options: Vec<(Value, Phrase)>) -> Result<usize, Report> {
    out.ask(
        Question::choose(
            sentence!("Which model should your agent think with?"),
            options,
        )
        .default(0),
    )
    .map_err(|report| {
        report.next(
            cmd!(
                "lnk agent model set <model> --agent {}",
                Name::new("agent", named)
            ),
            "choose this agent's model",
        )
    })
}

/// The local model that suits this machine, the one the models module
/// installs (`lnk model list --fit --json`, a list of it alone), if any.
pub(crate) fn recommended() -> Option<String> {
    let output = link_core::lnk()
        .args(["model", "list", "--fit", "--json"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice::<Vec<String>>(&output.stdout)
        .ok()?
        .into_iter()
        .next()
}

/// Installs `model` with the models module (and Ollama with it), then
/// finds it running.
pub(crate) fn install_local(out: &mut Out, model: &str) -> Result<Model, Report> {
    let shown = Name::new("model", model);
    out.hand_over();
    let status = link_core::lnk()
        .args(["model", "install", model, "--yes"])
        .status()
        .within("running lnk model install")?;
    if !status.success() {
        return Err(
            Report::error(sentence!("Installing {} didn't finish.", &shown))
                .next(cmd!("lnk model install {}", &shown), "try again"),
        );
    }
    let runtimes = local_runtimes().map_err(|why| {
        Report::error(sentence!("Can't look for models.")).why(Sentence::adopt(&why))
    })?;
    find_local(&runtimes, model).ok_or_else(|| {
        Report::error(sentence!(
            "{} is installed, but Ollama doesn't list it.",
            &shown
        ))
    })
}

/// Whether `model` runs here with fewer than 30 billion parameters, as
/// its name says (`qwen3:8b`, `llama3.2-3b-instruct`): too few, as a
/// rule, to call tools reliably. A name with no size isn't judged.
pub(crate) fn small_model(model: &Model) -> bool {
    let Some(name) = model.name.as_deref().filter(|_| model.is_local()) else {
        return false;
    };
    name.to_ascii_lowercase()
        .split([':', '-', '_', '/'])
        .filter_map(|part| part.strip_suffix('b'))
        .filter_map(|size| size.parse::<f64>().ok())
        .any(|billions| billions < 30.0)
}

/// Why `model` can't be used from here, if it can't: a local one whose
/// runtime doesn't answer.
pub(crate) fn model_ready(model: &Model) -> Result<(), Report> {
    if model_answers(model) {
        return Ok(());
    }
    let (runtime, start) = match model.provider.as_str() {
        "ollama" => ("Ollama", Some(cmd!("ollama serve"))),
        _ => ("its runtime", None),
    };
    let mut report = Report::error(sentence!(
        "{} is on {} at {}, which isn't answering.",
        Name::new("model", model.name.as_deref().unwrap_or("the model")),
        Product::new(runtime),
        Address::new(model.base_url.as_deref().unwrap_or_default())
    ));
    if let Some(start) = start {
        report = report.next(start, "start Ollama, then run this again");
    }
    Err(report.next(
        cmd!("lnk agent start --model.default <another>"),
        "or use another model",
    ))
}

/// Whether `model` can be reached from here: a hosted one always is, a
/// local one when its runtime answers.
pub(crate) fn model_answers(model: &Model) -> bool {
    if !model.is_local() {
        return true;
    }
    model
        .base_url
        .as_deref()
        .and_then(settings::local_port)
        .is_some_and(port_answers)
}

pub(crate) fn provider_name(provider: &str) -> &str {
    match Provider::of_saved(provider).filter(|known| *known != Provider::Local) {
        Some(known) => known.title(),
        None => provider,
    }
}

/// A provider a module adds, declared for this test process as a fixture
/// under the name only a module provides (`link`), the module's answer
/// never asked.
#[cfg(test)]
pub(crate) fn example_models() -> &'static link_model_contract::providers::Declared {
    use link_model_contract::providers::{Declared, Providers, declare, declared};
    declare(Providers {
        provider: std::collections::BTreeMap::from([(
            "link".into(),
            Declared {
                title: "Example Models".into(),
                base_url: "https://models.gw.example/v1".into(),
                speaks: "openai".into(),
                key_prefix: "ex_key_".into(),
            },
        )]),
    });
    declared("link").expect("the example provider is declared")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model of a provider a module adds is the route's provider's, its
    /// key checked against the declared prefix and never read for its
    /// provider.
    #[test]
    fn a_declared_providers_key_is_the_routes_providers() {
        example_models();
        let route = Route::parse("link/auto").unwrap();
        let model = hosted_at(&route, &Secret::from(" ex_key_abc ".to_string())).unwrap();
        assert_eq!(model.provider, "link");
        assert_eq!(model.name.as_deref(), Some("auto"));
        assert_eq!(model.api_key.expose(), "ex_key_abc");
        assert_eq!(model.base_url, None);
        // Another provider's key never goes to its address.
        assert!(hosted_at(&route, &Secret::from("sk-proj-abc".to_string())).is_err());
        // Nor its key to OpenAI.
        let openai = Route::parse("openai/gpt-6-luna").unwrap();
        assert!(hosted_at(&openai, &Secret::from("ex_key_abc".to_string())).is_err());
    }

    #[test]
    fn model_selection_recovery_names_the_agent_and_a_supported_command() {
        use clap::Parser;
        let actions: toml::Value = crate::navigation::navigation();
        for (action, typed) in [
            ("agent start", "lnk agent start --agent writer"),
            ("agent create", "lnk agent create writer"),
            ("agent restart", "lnk agent restart --agent writer"),
            (
                "agent harness set",
                "lnk agent harness set link --agent writer",
            ),
            ("agent team start", "lnk agent team start research"),
        ] {
            let words: Vec<_> = typed.split_whitespace().collect();
            let (mut out, _) = Out::capture(false, &words);
            let refusal = ask_model(
                &mut out,
                "writer",
                vec![(Value::from("qa-standin"), phrase!("on this machine"))],
            )
            .unwrap_err();
            assert_eq!(refusal.next.0.len(), 1);
            let recovery = &refusal.next.0[0].command;
            assert_eq!(
                recovery.text(),
                "lnk agent model set <model> --agent writer"
            );
            let fail: Vec<_> = actions[action]["fail"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect();
            assert!(
                link_format::nexts::declares(&fail, recovery, &out.typed()),
                "{action}"
            );
        }
        // The name the recovery names is looked up as the command line
        // looks one up, here a stand-in finding every name.
        link_actions::cli::names(link_agent::settings::ids::KIND, |_| {
            Ok("agent:k7m2p9q4r8s3".into())
        });
        assert!(
            link_agent::cli::Cli::try_parse_from([
                "lnk-agent",
                "model",
                "set",
                "local/qa-standin",
                "--agent",
                "writer"
            ])
            .is_ok()
        );
        assert!(
            link_agent::cli::Cli::try_parse_from([
                "lnk-agent",
                "team",
                "start",
                "research",
                "--model",
                "local/qa-standin"
            ])
            .is_err()
        );
    }

    /// A fallback's key comes from its variable, off the command line.
    #[test]
    fn a_fallback_key_from_its_variable() {
        let set = |var: &str| (var == FALLBACK_CREDENTIAL).then(|| " sk-or-1 ".to_string());
        let unset = |_: &str| None;
        let empty = |_: &str| Some("  ".to_string());
        assert_eq!(fallback_key(set).unwrap().expose(), "sk-or-1");
        assert!(fallback_key(unset).is_none());
        assert!(fallback_key(empty).is_none());
    }

    /// A model's name on two runtimes goes to the one you added, said;
    /// with neither or both yours, it's refused, naming each.
    #[test]
    fn a_name_two_runtimes_serve_is_yours_or_refused() {
        let runtime = |name: &str, url: &str, added: bool| Runtime {
            name: name.into(),
            base_url: url.into(),
            models: vec!["qa-standin".into(), "qwen3:8b".into()],
            added,
        };
        let ollama = runtime("Ollama", "http://localhost:11434", false);
        let yours = runtime("qa", "http://localhost:5000", true);
        let both = [ollama.clone(), yours.clone()];
        let found = find_local(&both, "qa-standin").unwrap();
        assert_eq!(found.base_url.as_deref(), Some("http://localhost:5000"));
        assert_eq!(found.provider, "local");
        let (mut out, said) = Out::capture(false, &["lnk-agent", "start"]);
        assert!(chosen_among(&mut out, &both, &found).is_ok());
        assert!(
            said.stderr().contains("takes the one you added"),
            "{}",
            said.stderr()
        );
        let found = [
            ollama.clone(),
            runtime("LM Studio", "http://localhost:1234", false),
        ];
        let picked = find_local(&found, "qa-standin").unwrap();
        let refusal = chosen_among(&mut out, &found, &picked).unwrap_err();
        let refused = refusal.headline.to_string();
        assert!(
            refused.contains("Ollama at http://localhost:11434 and LM Studio at"),
            "{refused}"
        );
        // Both yours: refused too.
        let added = [
            runtime("Ollama", "http://localhost:11434", true),
            yours.clone(),
        ];
        let picked = find_local(&added, "qa-standin").unwrap();
        assert!(chosen_among(&mut out, &added, &picked).is_err());
        let alone = [ollama];
        assert!(chosen_among(&mut out, &alone, &find_local(&alone, "qa-standin").unwrap()).is_ok());
        let actions: toml::Value = crate::navigation::navigation();
        for (action, typed) in [
            (
                "agent start",
                cmd!("lnk agent start --model.default local/qa-standin"),
            ),
            (
                "agent create",
                cmd!("lnk agent create another --model.default local/qa-standin"),
            ),
            (
                "agent restart",
                cmd!("lnk agent start --model.default local/qa-standin"),
            ),
            (
                "agent model set",
                cmd!("lnk agent model set local/qa-standin"),
            ),
        ] {
            let fail: Vec<String> = actions[action]["fail"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect();
            for suggestion in &refusal.next.0 {
                assert!(
                    link_format::nexts::declares(&fail, &suggestion.command, &typed),
                    "{action} must declare its ambiguous-model refusal hint: {}",
                    suggestion.command.text()
                );
            }
        }
    }

    #[test]
    fn a_small_local_model_is_one_named_under_30_billion() {
        let model = |provider: &str, name: &str| Model {
            actions_url: None,
            provider: provider.into(),
            api_key: Secret::default(),
            name: Some(name.into()),
            base_url: None,
        };
        assert!(small_model(&model("ollama", "qwen3:8b")));
        assert!(small_model(&model("ollama", "llama3.2:3b")));
        assert!(small_model(&model("local", "qwen2.5-7b-instruct")));
        assert!(!small_model(&model("ollama", "qwen3:32b")));
        assert!(!small_model(&model("ollama", "qwen3:30b-a3b")));
        assert!(!small_model(&model("ollama", "qwen3")));
        // A hosted model isn't judged by its name.
        assert!(!small_model(&model("openrouter", "meta/llama-3-8b")));
    }

    #[test]
    fn an_agent_whose_local_model_is_down_doesnt_start() {
        // A port that refuses: bound, and never listened on, held for the
        // test so no other test here takes it.
        let refusing = tokio::net::TcpSocket::new_v4().unwrap();
        refusing.bind(([127, 0, 0, 1], 0).into()).unwrap();
        let port = refusing.local_addr().unwrap().port();
        let local = Model {
            provider: "ollama".into(),
            name: Some("qwen3:8b".into()),
            base_url: Some(format!("http://localhost:{port}")),
            ..Default::default()
        };
        let err = model_ready(&local).unwrap_err();
        assert!(
            err.headline.to_string().contains("qwen3:8b is on Ollama")
                && err
                    .next
                    .0
                    .iter()
                    .any(|n| n.why.to_string() == "start Ollama, then run this again"),
            "{err:?}"
        );
        let hosted = Model {
            provider: "openai".into(),
            ..Default::default()
        };
        assert!(model_ready(&hosted).is_ok());
    }
}
