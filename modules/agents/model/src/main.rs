//! `lnk-model`: the models module's commands. `lnk` runs it for `lnk
//! model ...`.

use link_actions::answer;
use link_format::clean;
use link_format::{
    Address, Done, Exit, Fields, Foreign, Name, Note, Out, Path, Product, Question, Refuser,
    Report, Sentence, Table, Text, Value, WithinExt, cmd, phrase, prose, sentence, states,
};
use link_model::cli::{self, Command};
use link_model::{added, constants, discover, install};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::saved::own(&[link_model::added::saved, link_model_contract::facts_saved]);
    link_core::describe_component!();
    link_settings::output::run(|out| {
        // Its runtime after it, so the guard took its secrets on one thread.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .within("starting its runtime")?;
        runtime.block_on(try_main(out))
    })
}

async fn try_main(out: &mut Out) -> Result<Done, Report> {
    let command = cli::parse().command.command;
    if matches!(
        &command,
        Command::ModelUsage(_)
            | Command::ModelContext(_)
            | Command::ModelChat(_)
            | Command::ModelEmbed(_)
            | Command::ModelTranscribe(_)
            | Command::ModelCount(_)
    ) && std::env::var("LNK_MODEL_URL")
        .ok()
        .is_none_or(|url| url.trim().is_empty())
    {
        return Err(link_model::cli_api::missing_context());
    }
    match command {
        // A chat's model commands are its harness's to run, never parsed
        // here: their surfaces leave out the command line.
        Command::ModelGet(_) | Command::ModelHelp(_) | Command::ModelSet(_) => {
            return Err(Report::error(sentence!(
                "An agent's harness runs this command in its chat."
            ))
            .exit(Exit::Usage)
            .next(cmd!("lnk agent model set <model>"), "set an agent's model"));
        }
        Command::ModelUsage(_) => {
            let answer = link_model::cli_api::read("usage").await?;
            answer::answer::<cli::Usage>(out, &answer)?;
        }
        Command::ModelContext(args) => {
            let answer = link_model::cli_api::context(args.chat).await?;
            answer::answer::<cli::Context>(out, &answer)?;
        }
        Command::ModelChat(args) => {
            let answer = link_model::cli_api::request(out, "chat", args.request.as_deref()).await?;
            match out.is_json() {
                // The last line of its stream, after its text's.
                true => answer::line::<cli::Stream>(out, &answer)?,
                // Its text came as it streamed; the rest of the answer
                // after it.
                false => {
                    let mut rest = answer["answer"].clone();
                    if let Some(members) = rest.as_object_mut() {
                        members.remove("text");
                    }
                    out.answer(&rest);
                }
            }
        }
        Command::ModelEmbed(args) => {
            let answer =
                link_model::cli_api::request(out, "embed", args.request.as_deref()).await?;
            answer::answer::<cli::Embedded>(out, &answer)?;
        }
        Command::ModelTranscribe(args) => {
            let answer =
                link_model::cli_api::request(out, "transcribe", args.request.as_deref()).await?;
            answer::answer::<cli::Heard>(out, &answer)?;
        }
        Command::ModelCount(args) => {
            let answer =
                link_model::cli_api::request(out, "count", args.request.as_deref()).await?;
            answer::answer::<cli::Counted>(out, &answer)?;
        }
        Command::ModelList(cli::ModelListArgs { fit: true, .. }) => return fit(out),
        Command::ModelList(args) => return list(out, args.start).await,
        Command::RuntimeList(args) => {
            if args.start {
                start_installed(out).await;
            }
            // Every runtime: each found, and each the owner added
            // (`lnk model runtime add`), answering or not, so a name two
            // serve goes to theirs.
            let found = discover::discover().await;
            let runtimes: Vec<_> = discover::inventory(found, &added::load())
                .into_iter()
                .map(|listed| cli::Runtime {
                    added: listed.added.then_some(true),
                    state: match (listed.answering, listed.runtime.models.is_empty()) {
                        (false, _) => cli::RuntimeState::NotAnswering,
                        (true, true) => cli::RuntimeState::NoModels,
                        (true, false) => cli::RuntimeState::Running,
                    },
                    name: listed.runtime.name,
                    base_url: listed.runtime.base_url,
                    models: listed.runtime.models,
                })
                .collect();
            answer::reply::<cli::Runtimes>(out, &runtimes)?;
            return models(out).await;
        }
        Command::RuntimeAdd(args) => {
            return add(out, &args.url, &args.name).await;
        }
        Command::RuntimeRemove(args) => return remove(&args.name).await,
        Command::Uninstall(given) => {
            let kept = install::kept(&link_files::home_dir());
            // Ollama's models go through its API before the files go.
            let delete = |picked: &[String]| {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(install::delete_parts(picked))
                })
            };
            link_core::uninstall::run(&given, &kept, || Ok(()), delete)
                .within("removing what the models module installed")?;
            return Ok(Done::shown());
        }
        Command::ModelServe(args) => return serve(out, args).await,
        Command::ModelInstall(args) => {
            return install_model(out, args.model, args.yes).await;
        }
        Command::ModelDelete(args) => {
            return delete(out, &args.model, args.yes).await;
        }
        Command::ModelShow(args) => {
            let model = args.model;
            if model
                .as_deref()
                .is_some_and(|name| !link_model_contract::show::plain_name(name))
            {
                return Err(Report::error(sentence!("Invalid model name."))
                    .exit(Exit::Usage)
                    .next(cmd!("lnk model list"), "find model names running here"));
            }
            if model.is_none() && std::env::var_os("LNK_MODEL_URL").is_none() {
                return Err(
                    Report::error(sentence!("Name a model to inspect its details."))
                        .exit(Exit::Usage)
                        .next(cmd!("lnk model list"), "find model names running here"),
                );
            }
            let shown = link_model::cli_api::show(model).await?;
            // Granted a runtime (in an agent), its answer is the route's
            // `Shown` with whether its endpoint is ready; asked by its
            // owner, the model's state as blocks, which `lnk issue create`
            // puts in an issue.
            if std::env::var_os("LNK_MODEL_URL").is_some() {
                answer::answer::<cli::Shown>(out, &shown)?;
            } else {
                return show(out, &shown);
            }
        }
        Command::ModelBridge(args) => {
            let started = link_sandbox_route::bridge::Started {
                socket: args.socket,
                keys: args.keys,
                runtime: args.runtime,
                settings: args.settings,
            };
            link_model::bridge::serve(&started).await?
        }
    }
    Ok(Done::shown())
}

/// `lnk model list`: the names of the models running here.
async fn list(out: &mut Out, start: bool) -> Result<Done, Report> {
    let listed: Vec<String> = if let Ok(url) = std::env::var("LNK_MODEL_URL") {
        link_model_contract::chat::client::canonical::get(
            &link_model_contract::chat::client::canonical::client()?,
            &url,
            "list",
        )
        .await?
    } else {
        if start {
            start_installed(out).await;
        }
        let mut names: Vec<String> = discover::discover()
            .await
            .into_iter()
            .flat_map(|runtime| runtime.models)
            .filter(|name| link_model_contract::show::plain_name(name))
            .collect();
        names.sort();
        names.dedup();
        names
    };
    answer::reply::<cli::Names>(out, &listed)?;
    let mut table = Table::new(["Model"], Sentence::adopt(&discover::nothing_running()));
    for name in &listed {
        table.row([Name::new("model", clean(name))]);
    }
    out.block(table);
    Ok(match listed.first() {
        Some(first) => Done::shown().next(
            cmd!(
                "lnk model serve --model {}",
                Name::new("model", clean(first))
            ),
            "serve it here",
        ),
        None => Done::shown()
            .next(cmd!("lnk model install"), "install a model with Ollama")
            .next(
                cmd!("lnk model runtime add <url>"),
                "add a runtime of yours",
            ),
    })
}

states! {
    /// A model runtime as `lnk model list` finds it.
    enum RuntimeState {
        Running("running") => Good,
        NoModels("no models") => Quiet,
        NotAnswering("not answering") => Bad,
    }
}

/// `lnk model serve`: a model's API on this machine, until stopped.
async fn serve(out: &mut Out, args: cli::ModelServeArgs) -> Result<Done, Report> {
    let cli::ModelServeArgs {
        upstream,
        model,
        port,
        json: _,
    } = args;
    // The runtimes asked once, and again only after starting Ollama.
    let mut found = None;
    if upstream.is_none() {
        let runtimes = discover::discover().await;
        // Never with LNK_SERVICES=off, which tests set.
        let ollama = install::ollama_installed().filter(|_| link_service::managed());
        match (runtimes.is_empty(), ollama) {
            // Installed but not running: start it rather than fail.
            (true, Some(ollama)) => {
                let _wait = out.activity(phrase!("Starting Ollama"));
                install::start_ollama(&ollama).await;
            }
            _ => found = Some(runtimes),
        }
    }
    let resolved = match (&upstream, found) {
        (None, Some(runtimes)) => link_model::choose(&runtimes, model.as_deref()),
        _ => link_model::resolve(upstream.as_deref(), model.as_deref()).await,
    };
    let target = match resolved {
        Ok(target) => target,
        Err(err) => {
            let missing = upstream.is_none()
                && model.as_deref().is_some_and(|model| {
                    err.to_string()
                        .starts_with(&format!("no model named {model:?}"))
                });
            let report = Report::error(Sentence::adopt(&err.to_string()));
            return Err(match (missing, &model) {
                // The code `lnk model serve` offers to install on.
                (true, Some(model)) => report
                    .next(cmd!("lnk model list"), "see what runs here")
                    .next(
                        cmd!("lnk model install {}", Name::new("model", clean(model))),
                        "install it with Ollama",
                    )
                    .exit(Exit::Usage),
                _ => report.next(cmd!("lnk model list"), "see what runs here"),
            });
        }
    };
    let runtime = target.runtime.map(|r| format!("{r}, ")).unwrap_or_default();
    let model = target
        .model
        .map_or("all models".into(), |m| format!("model {m}"));
    let description = format!("{}/v1 ({runtime}{model})", target.upstream);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .within("opening its port")?;
    let url = format!(
        "http://{}",
        listener.local_addr().within("reading its port")?
    );
    // Kept while the server runs: `--json` ends it by closing stdin.
    let closed = if out.is_json() {
        answer::line::<cli::Serving>(
            out,
            &cli::Serving {
                url: url.clone(),
                description: description.clone(),
            },
        )?;
        Some(tokio::spawn(async {
            let _ = tokio::io::copy(&mut tokio::io::stdin(), &mut tokio::io::sink()).await;
        }))
    } else {
        None
    };
    out.now(
        Fields::new()
            .key("Serving", Address::new(format!("{url}/v1")))
            .row("Using", Foreign::new(&description)),
    );
    match closed {
        // Its owner gone, the port closes at once, and what it already
        // took is answered for a while.
        Some(closed) => {
            let (gone, owner_gone) = tokio::sync::oneshot::channel();
            let serving = link_model::serve(listener, target.service, async move {
                let _ = closed.await;
                let _ = gone.send(());
            });
            tokio::pin!(serving);
            tokio::select! {
                served = &mut serving => served.within("serving")?,
                Ok(()) = owner_gone => {
                    if let Ok(served) =
                        tokio::time::timeout(constants::ANSWERING_AFTER_OWNER, serving).await
                    {
                        served.within("serving")?;
                    }
                }
            }
        }
        None => link_model::serve(listener, target.service, std::future::pending())
            .await
            .within("serving")?,
    }
    Ok(Done::shown())
}

/// `lnk model list --start`: an installed Ollama that isn't running,
/// started.
async fn start_installed(out: &mut Out) {
    if install::ollama_running().await {
        return;
    }
    if let Some(ollama) = install::ollama_installed() {
        let _wait = out.activity(phrase!("Starting Ollama"));
        install::start_ollama(&ollama).await;
    }
}

async fn models(out: &mut Out) -> Result<Done, Report> {
    let listed = discover::inventory(discover::discover().await, &added::load());
    // Added addresses list even while nothing answers there.
    let (runtimes, silent): (Vec<_>, Vec<_>) = listed.into_iter().partition(|l| l.answering);
    let runtimes: Vec<_> = runtimes.into_iter().map(|l| l.runtime).collect();
    let silent: Vec<_> = silent.into_iter().map(|l| l.runtime).collect();
    let stopped = !install::ollama_running().await && install::ollama_installed().is_some();
    let mut table = Table::new(
        ["Model", "Runtime", "Address", "State"],
        Sentence::adopt(&discover::nothing_running()),
    );
    for runtime in &runtimes {
        let at = || {
            (
                Product::new(runtime.name.as_str()),
                Address::new(runtime.base_url.as_str()),
            )
        };
        if runtime.models.is_empty() {
            let (name, address) = at();
            table.row([
                Value::from(String::new()),
                name.into(),
                address.into(),
                RuntimeState::NoModels.into(),
            ]);
        }
        for model in &runtime.models {
            let (name, address) = at();
            table.row([
                Value::from(Name::new("model", clean(model))),
                name.into(),
                address.into(),
                RuntimeState::Running.into(),
            ]);
        }
    }
    for runtime in &silent {
        table.row([
            Value::from(String::new()),
            Product::new(runtime.name.as_str()).into(),
            Address::new(runtime.base_url.as_str()).into(),
            RuntimeState::NotAnswering.into(),
        ]);
    }
    let empty = table.is_empty();
    out.block(table);
    let first = runtimes.iter().flat_map(|r| r.models.first()).next();
    Ok(match (first, empty, stopped) {
        (Some(model), _, _) => Done::shown()
            .next(
                cmd!(
                    "lnk model serve --model {}",
                    Name::new("model", clean(model))
                ),
                "serve it here",
            )
            .next(cmd!("lnk model serve"), "serve a runtime here"),
        (None, true, true) => Done::shown().next(
            cmd!("lnk model list --start"),
            "start Ollama, which is installed",
        ),
        (None, _, _) => Done::shown()
            .next(cmd!("lnk model install"), "install a model with Ollama")
            .next(
                cmd!("lnk model runtime add <url>"),
                "add a runtime of yours",
            ),
    })
}

/// `lnk model runtime add`.
async fn add(out: &mut Out, url: &str, name: &str) -> Result<Done, Report> {
    let (base_url, outcome) = added::add(url, name).map_err(|why| {
        Report::error(Sentence::adopt(&why.to_string()))
            .next(
                cmd!("lnk model runtime add <url>"),
                "give an address like http://localhost:5000",
            )
            .exit(Exit::Usage)
    })?;
    let name = clean(name.trim());
    let address = Address::new(base_url.as_str());
    let done = match outcome {
        added::Outcome::Known(known) => sentence!(
            "Added {}, the default port of {}, which counts as yours.",
            address,
            Product::new(known)
        ),
        added::Outcome::Added => sentence!("Added {} at {}.", Product::new(name.as_str()), address),
        added::Outcome::Renamed => sentence!(
            "{} was added already; it's {} now.",
            address,
            Product::new(name.as_str())
        ),
    };
    let mut next = Done::success(done).changed("Kept in", Path::new(added::path()));
    match discover::probe_one(&name, &base_url).await {
        Some(runtime) if runtime.models.is_empty() => {
            out.block(Note(sentence!("It answers, and serves no models yet.")));
            next = next.next(cmd!("lnk model install"), "install a model with Ollama");
        }
        Some(runtime) => {
            let models: Vec<_> = runtime.models.iter().map(|m| clean(m)).collect();
            out.block(Note(sentence!(
                "It serves {}.",
                Foreign::new(&models.join(", "))
            )));
            next = next.next(
                cmd!(
                    "lnk agent start --model.default {}",
                    Name::new("model", format!("local/{}", models[0]))
                ),
                "your agent can use it",
            );
        }
        None => {
            out.block(Note(sentence!(
                "Nothing answers {} now.",
                Address::new(format!("{base_url}/v1/models"))
            )));
            next = next.next(cmd!("lnk model list"), "list its models while it runs");
        }
    }
    Ok(next)
}

/// `lnk model runtime remove`: a runtime `lnk model runtime add` added, removed.
async fn remove(name: &str) -> Result<Done, Report> {
    let gone = added::forget(name).map_err(|why| {
        Report::error(Sentence::adopt(&why.to_string()))
            .next(cmd!("lnk model runtime list"), "list the runtimes it knows")
    })?;
    let names: Vec<String> = gone
        .iter()
        .map(|g| format!("{} at {}", clean(&g.name), g.base_url))
        .collect();
    Ok(
        Done::success(sentence!("Removed {}.", Foreign::new(&names.join(", ")))).next(
            cmd!("lnk model runtime add <url>"),
            "add a runtime of yours",
        ),
    )
}

/// `lnk model install`.
async fn install_model(out: &mut Out, model: Option<String>, yes: bool) -> Result<Done, Report> {
    let model = match model {
        Some(model) => model,
        None => {
            let pick = install::recommend().map_err(|why| {
                Report::error(Sentence::adopt(&why.to_string())).next(
                    cmd!("lnk model install <model>"),
                    "name a model to install one anyway",
                )
            })?;
            out.block(Note(Sentence::adopt(&format!(
                "Installing {} (about {}): {}",
                pick.model, pick.size, pick.why
            ))));
            pick.model
        }
    };
    install::ensure_ollama(out, yes).await?;
    let ollama = discover::probe_one("Ollama", install::OLLAMA)
        .await
        .within("Ollama stopped answering")?;
    let shown = Name::new("model", clean(&model));
    let done = if let Ok((_, id)) = discover::find_model(std::slice::from_ref(&ollama), &model) {
        sentence!("{} is installed already.", Name::new("model", clean(&id)))
    } else {
        install::pull(out, install::OLLAMA, &model).await?;
        sentence!("Installed {}.", &shown)
    };
    Ok(Done::success(done)
        .next(
            cmd!(
                "lnk agent start --model.default {}",
                Name::new("model", format!("local/{}", clean(&model)))
            ),
            "an agent thinks with it",
        )
        .next(cmd!("lnk model serve --model {}", &shown), "serve it here"))
}

/// `lnk model delete`: a model deleted from Ollama.
async fn delete(out: &mut Out, model: &str, yes: bool) -> Result<Done, Report> {
    let Some(ollama) = discover::probe_one("Ollama", install::OLLAMA).await else {
        return Err(Report::error(sentence!(
            "Ollama isn't running here, and Link removes only Ollama's models."
        ))
        .next(
            cmd!("lnk model list --start"),
            "start Ollama, if it is installed",
        ));
    };
    let (_, id) = discover::find_model(std::slice::from_ref(&ollama), model).map_err(|why| {
        Report::error(Sentence::adopt(&why.to_string()))
            .next(cmd!("lnk model list"), "see the models that run here")
    })?;
    let id = Name::new("model", clean(&id));
    let sure =
        out.ask(Question::confirm(sentence!("Delete {} from Ollama?", &id), true).yes(yes))?;
    if !sure {
        return Err(Report::refusal(
            Refuser::Person,
            sentence!("Not deleting {}.", &id),
        ));
    }
    install::delete(install::OLLAMA, id.as_str()).await?;
    Ok(
        Done::success(sentence!("Deleted {}.", &id))
            .next(cmd!("lnk model list"), "see what's left"),
    )
}

/// `lnk model list --fit`: the model that suits this machine, and why.
fn fit(out: &mut Out) -> Result<Done, Report> {
    let pick = install::recommend();
    // Nothing fits: no name.
    let names: Vec<String> = pick.iter().map(|pick| pick.model.clone()).collect();
    answer::reply::<cli::Names>(out, &names)?;
    Ok(match pick {
        Ok(pick) => {
            let model = Name::new("model", clean(&pick.model));
            out.block(Text(prose!(
                "{} (about {}): {}.",
                &model,
                Foreign::new(&pick.size),
                Foreign::new(pick.why.trim_end_matches('.'))
            )));
            Done::shown().next(cmd!("lnk model install"), "install it")
        }
        Err(why) => {
            out.block(Text(prose!(
                "None: {}.",
                Foreign::new(why.trim_end_matches('.'))
            )));
            Done::shown().next(
                cmd!("lnk model install <model>"),
                "name a model to install one anyway",
            )
        }
    })
}

/// `lnk model show <model>`: what Link knows of it, and what its runtime
/// says of it.
fn show(out: &mut Out, shown: &link_model_contract::show::Shown) -> Result<Done, Report> {
    answer::reply::<cli::Shown>(out, shown)?;
    let tokens = |n: Option<u32>| n.map_or("?".to_string(), |n| n.to_string());
    let mut fields = Fields::new().key("Model", Name::new("model", clean(&shown.name)));
    match &shown.facts {
        Some(facts) => {
            let thinks = facts
                .thinking
                .map(|t| serde_json::to_value(t).unwrap_or_default())
                .and_then(|t| t.as_str().map(str::to_string))
                .unwrap_or_else(|| "?".into());
            let price = facts
                .price
                .map_or("?".to_string(), |p| format!("{}, {}", p.input, p.output));
            fields = fields
                .row("Window", Foreign::new(&tokens(facts.window)))
                .row("Answer", Foreign::new(&tokens(facts.answer)))
                .row("Thinks", Foreign::new(&thinks))
                .row("$/M in, out", Foreign::new(&price));
        }
        None => fields = fields.row("Facts", Foreign::new("none known")),
    }
    out.block(fields);
    Ok(Done::shown().next(cmd!("lnk model list"), "see the models that run here"))
}
