//! [`refresh`]: the evidence read again, from crates.io, npm's registry
//! and OSV.

use super::*;

/// What every request says it is, as crates.io asks.
const USER_AGENT: &str = "link-repository-check (https://github.com/so-local/link)";

/// The most an answer may be: a crate of many versions (web-sys) says
/// more than a megabyte.
const ANSWER_MAX: u64 = 16 << 20;

/// How many npm packages are read at once: its downloads API says to slow
/// down (429) to a few readers, and a reader that is told so waits minutes.
const NPM_AT_ONCE: usize = 2;

/// npm's count of a package's downloads in the last week, by its name. Its
/// search leaves some widely used packages out, which it would then count
/// as downloaded by no one.
const NPM_DOWNLOADS: &str = "https://api.npmjs.org/downloads/point/last-week";

/// How long each npm reader rests before it asks for a package's downloads.
const NPM_PAUSE: std::time::Duration = std::time::Duration::from_millis(500);

/// How many versions one OSV query asks about, its most.
const OSV_BATCH: usize = 1000;

/// Reads who publishes each package of `root`'s lock files, and OSV's
/// advisories against each version locked, and writes what they say to
/// `evidence`. crates.io is read once a second, as it asks; what was read
/// today already is kept, so a refresh that stopped partway goes on from
/// where it was. The advisories are read whole every time.
pub fn refresh(root: &Path, evidence: &Path, today: &str) {
    refresh_in(Locks::of(root), evidence, today)
}

/// [`refresh`] for lock files apart, as [`trust_in`] reads them.
pub fn refresh_in(locks: Locks, evidence: &Path, today: &str) {
    let client = client();
    let mut written = std::fs::read_to_string(evidence)
        .ok()
        .and_then(|text| toml::from_str::<Evidence>(&text).ok())
        .filter(|kept| kept.checked == today)
        .unwrap_or_else(|| Evidence {
            checked: today.to_string(),
            ..Evidence::default()
        });
    let mut unread = Vec::new();
    let mut locked: BTreeMap<String, (Registry, Locked)> = BTreeMap::new();
    for registry in Registry::ALL {
        let (served, _) = registry.locked(locks);
        // One version of each name to read its owners at.
        let mut names: BTreeMap<String, String> = BTreeMap::new();
        for package in &served {
            names
                .entry(package.name.clone())
                .or_insert_with(|| package.version.clone());
            locked.insert(affected(registry, package), (registry, package.clone()));
        }
        let published = match registry {
            Registry::Crates => &mut written.crates,
            Registry::Npm => &mut written.npm,
        };
        published.retain(|name, _| names.contains_key(name));
        let wanted: Vec<(String, String)> = names
            .into_iter()
            .filter(|(name, _)| !published.contains_key(name))
            .collect();
        let read = match registry {
            Registry::Crates => wanted
                .iter()
                .map(|(name, _)| (name.clone(), crate_published(&client, name)))
                .collect(),
            Registry::Npm => npm_published(&wanted),
        };
        for (name, read) in read {
            match read {
                Ok(seen) => {
                    published.insert(name, seen);
                }
                Err(e) => unread.push(e),
            }
        }
    }
    match advisories(&client, &locked) {
        Ok(advisories) => written.advisories = advisories,
        Err(e) => unread.push(e),
    }
    let text = toml::to_string(&written).expect("the evidence as TOML");
    std::fs::write(
        evidence,
        format!("# Who publishes each package of the lock files, and the advisories against\n# them, as crates.io, npm's registry and OSV said on `checked`. Written by\n# its package's refresh test, `cargo test -- --ignored refresh`; read by\n# every test run.\n{text}"),
    )
    .expect("writing the evidence");
    assert!(
        unread.is_empty(),
        "written without these, which weren't answered:\n{}",
        unread.join("\n")
    );
}

fn client() -> link_fetch::blocking::Client {
    link_fetch::blocking::Client::new().expect("an HTTP client")
}

/// `request`'s answer as JSON, tried four times, waiting longer before
/// each: a registry that says to slow down (429) is given a minute.
#[expect(
    clippy::disallowed_methods,
    reason = "backs off before asking the registry again"
)]
fn answer<'c>(
    url: &str,
    request: impl Fn() -> link_fetch::blocking::Request<'c>,
) -> Result<serde_json::Value, String> {
    let mut last = String::new();
    for wait in [5, 20, 60, 0] {
        match request()
            .header("User-Agent", USER_AGENT)
            .limit(ANSWER_MAX)
            .text()
        {
            Ok(text) => return serde_json::from_str(&text).map_err(|e| format!("{url}: {e}")),
            Err(e) => last = format!("{url}: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_secs(wait));
    }
    Err(last)
}

/// A crate's owners and downloads, from crates.io, a second apart.
#[expect(
    clippy::disallowed_methods,
    reason = "keeps to crates.io's rate of one request a second"
)]
fn crate_published(client: &link_fetch::blocking::Client, name: &str) -> Result<Published, String> {
    let get = |url: String| {
        std::thread::sleep(std::time::Duration::from_secs(1));
        answer(&url, || client.get(&url))
    };
    let about = get(format!("https://crates.io/api/v1/crates/{name}"))?;
    let owners = get(format!("https://crates.io/api/v1/crates/{name}/owners"))?;
    Ok(Published {
        owners: owners["users"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|u| u["login"].as_str())
            .map(owner)
            .collect(),
        downloads: about["crate"]["downloads"].as_u64().unwrap_or(0),
        unofficial: unofficial(about["crate"]["description"].as_str().unwrap_or_default()),
    })
}

/// Each npm package `wanted` names, at its version: its maintainers and
/// scope, and its downloads in the last week, as npm's downloads API
/// counts them; [`NPM_AT_ONCE`] at a time.
fn npm_published(wanted: &[(String, String)]) -> Vec<(String, Result<Published, String>)> {
    let size = wanted.len().div_ceil(NPM_AT_ONCE).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = wanted
            .chunks(size)
            .map(|chunk| {
                scope.spawn(move || {
                    let client = client();
                    chunk
                        .iter()
                        .map(|(name, version)| (name.clone(), npm_package(&client, name, version)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("reading npm"))
            .collect()
    })
}

#[expect(
    clippy::disallowed_methods,
    reason = "keeps to npm's downloads API rate, which refuses a faster reader"
)]
fn npm_package(
    client: &link_fetch::blocking::Client,
    name: &str,
    version: &str,
) -> Result<Published, String> {
    let url = format!("https://registry.npmjs.org/{name}/{version}");
    let about = answer(&url, || client.get(&url))?;
    std::thread::sleep(NPM_PAUSE);
    let counted = format!("{NPM_DOWNLOADS}/{name}");
    let downloads = answer(&counted, || client.get(&counted))?["downloads"]
        .as_u64()
        .ok_or_else(|| format!("{counted}: no weekly downloads in its answer"))?;
    let scope = name
        .strip_prefix('@')
        .and_then(|scoped| scoped.split_once('/'))
        .map(|(scope, _)| format!("@{scope}"));
    Ok(Published {
        owners: scope
            .into_iter()
            .chain(
                about["maintainers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m["name"].as_str())
                    .map(str::to_string),
            )
            .collect(),
        downloads,
        unofficial: unofficial(about["description"].as_str().unwrap_or_default()),
    })
}

/// OSV's advisories against each version `locked` names, withdrawn ones
/// aside, by id.
fn advisories(
    client: &link_fetch::blocking::Client,
    locked: &BTreeMap<String, (Registry, Locked)>,
) -> Result<BTreeMap<String, Advisory>, String> {
    let all: Vec<(&String, &(Registry, Locked))> = locked.iter().collect();
    let mut against: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let batch = "https://api.osv.dev/v1/querybatch";
    for chunk in all.chunks(OSV_BATCH) {
        let queries: Vec<serde_json::Value> = chunk
            .iter()
            .map(|(_, (registry, package))| {
                serde_json::json!({
                    "package": {"ecosystem": registry.ecosystem(), "name": package.name},
                    "version": package.version,
                })
            })
            .collect();
        let body = serde_json::json!({ "queries": queries });
        let found = answer(batch, || client.post(batch).json(&body))?;
        let results = found["results"]
            .as_array()
            .filter(|results| results.len() == chunk.len())
            .ok_or_else(|| format!("{batch}: not one result for each query"))?;
        for ((named, _), result) in chunk.iter().zip(results) {
            for vuln in result["vulns"].as_array().into_iter().flatten() {
                if let Some(id) = vuln["id"].as_str() {
                    against
                        .entry(id.to_string())
                        .or_default()
                        .insert((*named).clone());
                }
            }
        }
    }
    let mut advisories = BTreeMap::new();
    for (id, affects) in against {
        let url = format!("https://api.osv.dev/v1/vulns/{id}");
        let about = answer(&url, || client.get(&url))?;
        if about.get("withdrawn").is_some_and(|w| !w.is_null()) {
            continue;
        }
        let summary = about["summary"]
            .as_str()
            .or_else(|| about["details"].as_str().and_then(|d| d.lines().next()))
            .unwrap_or_default()
            .trim()
            .to_string();
        advisories.insert(
            id,
            Advisory {
                summary,
                affects: affects.into_iter().collect(),
            },
        );
    }
    Ok(advisories)
}
