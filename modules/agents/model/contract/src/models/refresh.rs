//! The release's refresh of Link's models file: OpenRouter's figures
//! taken into it within what was reviewed, and the file printed back as
//! Link includes it.

use anyhow::ensure;
use link_format::WithinExt;

use super::{Facts, File, Price, Thinks, check, parse, plain, retired, selection};

/// `current` (Link's file) with OpenRouter's figures (`models`, the body
/// of `GET https://openrouter.ai/api/v1/models`): each model it lists,
/// its window, longest answer and prices, matched to an entry by its
/// [`plain`] name or added by its own, except a [`retired`] one; how a
/// model thinks stays as `current` says, and a new one's is
/// OpenRouter's (reasoning at an effort, Anthropic's adaptive). An error for an answer with no models,
/// so a release never ships an emptied file. A figure of an entry
/// `current` has that moves past [`plausible_change`] is refused: the
/// entry keeps `current`'s, and the change is said, one line each, so
/// what's released stays near what was reviewed.
pub fn refresh(current: &File, models: &serde_json::Value) -> anyhow::Result<(File, Vec<String>)> {
    let listed = models["data"]
        .as_array()
        .within("OpenRouter's answer has no models")?;
    ensure!(
        listed.len() >= 10,
        "OpenRouter listed {} models: too few to trust",
        listed.len()
    );
    let mut file = current.clone();
    let mut refused = Vec::new();
    let ids: Vec<&str> = listed.iter().filter_map(|m| m["id"].as_str()).collect();
    let mut ambiguous = std::collections::BTreeSet::new();
    let per_million = |value: &serde_json::Value| -> Option<f64> {
        let per_token: f64 = match value {
            serde_json::Value::String(text) => text.parse().ok()?,
            other => other.as_f64()?,
        };
        (per_token.is_finite() && per_token >= 0.0).then(|| (per_token * 1e6 * 1e6).round() / 1e6)
    };
    for model in listed {
        let Some(id) = model["id"]
            .as_str()
            .filter(|id| check(id, &Facts::default()).is_ok())
        else {
            continue;
        };
        // A retired model stays out, under any router's name for it.
        if retired().contains_key(&plain(id)) {
            continue;
        }
        let window = model["context_length"]
            .as_u64()
            .filter(|w| *w >= 1024)
            .map(|w| w.min(u32::MAX as u64) as u32);
        let Some(window) = window else {
            continue;
        };
        let answer = model["top_provider"]["max_completion_tokens"]
            .as_u64()
            .filter(|a| *a >= 256)
            .map(|a| a.min(u32::MAX as u64) as u32);
        let pricing = &model["pricing"];
        let price = match (
            per_million(&pricing["prompt"]),
            per_million(&pricing["completion"]),
        ) {
            (Some(input), Some(output)) => Some(Price {
                input,
                output,
                cached: per_million(&pricing["input_cache_read"]),
            }),
            _ => None,
        };
        let reasons = model["supported_parameters"]
            .as_array()
            .is_some_and(|p| p.iter().any(|p| p == "reasoning"));
        // Matched to an entry of `current`'s, never one this answer
        // changed or added: each is bound by what was reviewed.
        let key = current
            .model
            .keys()
            .find(|known| *known == id || plain(known) == plain(id))
            .cloned();
        match key {
            // Its own id is listed too: that one alone is its figures.
            Some(key) if key != id && ids.contains(&key.as_str()) => {}
            // Several ids, none its own, would each set it: none does.
            Some(key)
                if key != id && ids.iter().filter(|i| plain(i) == plain(&key)).count() > 1 =>
            {
                if ambiguous.insert(key.clone()) {
                    refused.push(format!(
                        "{}: more than one model listed is it; kept as it was",
                        link_format::clean(&key)
                    ));
                }
            }
            Some(key) => {
                let old = current.model.get(&key).cloned().unwrap_or_default();
                let new = Facts {
                    window: Some(window),
                    answer: answer.or(old.answer),
                    thinking: old.thinking,
                    price: price.or(old.price),
                    above: old.above.clone(),
                    cache: old.cache.clone(),
                };
                match plausible_change(&old, &new) {
                    Ok(()) => {
                        file.model.insert(key, new);
                    }
                    Err(why) => refused.push(format!(
                        "{}: {why} (from {id}); kept as it was",
                        link_format::clean(&key)
                    )),
                }
            }
            // A first party's model enters only as a newer generation of
            // a line Link describes.
            None if !current_generation(current, id) => {}
            None => {
                let thinking = match (reasons, id.starts_with("anthropic/")) {
                    (false, _) => Thinks::No,
                    (true, true) => Thinks::Adaptive,
                    (true, false) => Thinks::Effort,
                };
                file.model.insert(
                    id.to_string(),
                    Facts {
                        window: Some(window),
                        answer,
                        thinking: Some(thinking),
                        price,
                        above: Vec::new(),
                        cache: Vec::new(),
                    },
                );
            }
        }
    }
    for (name, facts) in &file.model {
        check(name, facts)?;
    }
    Ok((file, refused))
}

/// A model's line and generation, by its [`plain`] name: the name
/// without its numbers, and the numbers in order (`claude-sonnet-4-5`
/// and `claude-3-5-sonnet` are both `claude-sonnet`, generation 4.5 and
/// 3.5; `gpt-6.1-sol` is `gpt-sol`, 6.1; `gpt-4o-mini` is `gpt-o-mini`, 4).
pub fn generation(name: &str) -> (String, Vec<u32>) {
    let mut line = Vec::new();
    let mut numbers = Vec::new();
    let plain = plain(name);
    for part in plain.split('-') {
        let digits = part.bytes().take_while(u8::is_ascii_digit).count();
        if let Ok(number) = part[..digits].parse() {
            numbers.push(number);
        }
        if digits < part.len() {
            line.push(&part[digits..]);
        }
    }
    (line.join("-"), numbers)
}

/// Whether a refresh may add `id`, a model `current` doesn't describe:
/// another vendor's always; a model of a first party Link speaks to
/// (`claude-...`, `gpt-...`) only when `current` describes its line and
/// it's a newer generation than any `current` has of it. So an older
/// model a router still lists never enters, and a first party's new line
/// is added by hand, with how it thinks and its cache.
pub fn current_generation(current: &File, id: &str) -> bool {
    let own = id.rsplit('/').next().unwrap_or(id);
    if selection::owner(own).is_none() {
        return true;
    }
    let (line, of) = generation(id);
    let newest = current
        .model
        .keys()
        .map(|known| generation(known))
        .filter(|(known, _)| *known == line)
        .map(|(_, numbers)| numbers)
        .max();
    newest.is_some_and(|newest| of > newest)
}

/// Whether `new` figures for a model are a change its provider could
/// make to `old`, or why not: a window or longest answer more than
/// halved, or a price moved more than tenfold (a price of nothing or of
/// anything from nothing included). Facts a release takes from outside
/// are bound by what was reviewed, so a wrong answer can't shrink every
/// agent's window or zero its costs.
pub fn plausible_change(old: &Facts, new: &Facts) -> anyhow::Result<()> {
    if let (Some(old), Some(new)) = (old.window, new.window) {
        ensure!(
            new >= old / 2,
            "its window would go from {old} to {new} tokens"
        );
    }
    if let (Some(old), Some(new)) = (old.answer, new.answer) {
        ensure!(
            new >= old / 2,
            "its longest answer would go from {old} to {new} tokens"
        );
    }
    if let (Some(old), Some(new)) = (&old.price, &new.price) {
        for (key, old, new) in [
            ("input", Some(old.input), Some(new.input)),
            ("output", Some(old.output), Some(new.output)),
            ("cached", old.cached, new.cached),
        ] {
            if let (Some(old), Some(new)) = (old, new) {
                ensure!(
                    new <= old * 10.0 && new * 10.0 >= old && (old > 0.0) == (new > 0.0),
                    "its {key} price would go from ${old} to ${new} a million tokens"
                );
            }
        }
    }
    Ok(())
}

/// `file` as Link includes it: `head` (its comments) and each model, one
/// table each, in order.
pub fn print(head: &str, file: &File) -> anyhow::Result<String> {
    let mut out = String::new();
    out.push_str(head.trim_end());
    out.push_str("\n\n");
    let schema = <File as link_files::schema::Specification>::format().current;
    out.push_str(&format!("schema = {schema}\n"));
    if let Some(version) = file.version {
        out.push_str(&format!("version = {version}\n"));
    }
    for (name, facts) in &file.model {
        out.push_str(&format!(
            "\n[model.{}]\n",
            toml::Value::String(name.clone())
        ));
        if let Some(window) = facts.window {
            out.push_str(&format!("window = {window}\n"));
        }
        if let Some(answer) = facts.answer {
            out.push_str(&format!("answer = {answer}\n"));
        }
        if let Some(thinking) = facts.thinking {
            out.push_str(&format!(
                "thinking = {}\n",
                toml::Value::try_from(thinking)?
            ));
        }
        if let Some(price) = &facts.price {
            out.push_str(&format!("price = {}\n", inline(price)));
        }
        if !facts.above.is_empty() {
            let jumps = facts
                .above
                .iter()
                .map(|jump| {
                    format!(
                        "{{ tokens = {}, price = {} }}",
                        jump.tokens,
                        inline(&jump.price)
                    )
                })
                .collect::<Vec<_>>();
            out.push_str(&format!("above = [{}]\n", jumps.join(", ")));
        }
        if !facts.cache.is_empty() {
            let offers = facts
                .cache
                .iter()
                .map(|offer| {
                    format!(
                        "{{ lasts = {}, write = {} }}",
                        toml::Value::String(offer.lasts.clone()),
                        toml::Value::Float(offer.write)
                    )
                })
                .collect::<Vec<_>>();
            out.push_str(&format!("cache = [{}]\n", offers.join(", ")));
        }
    }
    // What it prints, Link reads back the same.
    ensure!(
        parse(&out)? == *file,
        "the models file didn't read back as written"
    );
    Ok(out)
}

/// `price` as an inline table: `{ input = 3.0, output = 15.0 }`.
fn inline(price: &Price) -> String {
    let mut parts = vec![
        format!("input = {}", toml::Value::Float(price.input)),
        format!("output = {}", toml::Value::Float(price.output)),
    ];
    if let Some(cached) = price.cached {
        parts.push(format!("cached = {}", toml::Value::Float(cached)));
    }
    format!("{{ {} }}", parts.join(", "))
}

/// The comments at the top of a models file: its lines before the first
/// that isn't one.
pub fn head(text: &str) -> String {
    text.lines()
        .take_while(|line| line.starts_with('#') || line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
