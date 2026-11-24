//! Searching: the APIs it calls with your key, what a search asks, and
//! their results as hits.

use super::*;

/// A search API it calls, with your key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// Brave Search's API, `BRAVE_API_KEY`.
    Brave,
    /// Tavily's API, `TAVILY_API_KEY`.
    Tavily,
}

impl Provider {
    /// Its name, as `LNK_WEB_SEARCH` takes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Brave => "brave",
            Self::Tavily => "tavily",
        }
    }

    /// The variable its key is in.
    pub fn key_env(self) -> &'static str {
        match self {
            Self::Brave => "BRAVE_API_KEY",
            Self::Tavily => "TAVILY_API_KEY",
        }
    }

    /// Its API's address (`limits.toml`'s `[addresses]`).
    fn url(self) -> &'static str {
        let addresses = &crate::limits().addresses;
        match self {
            Self::Brave => &addresses.brave,
            Self::Tavily => &addresses.tavily,
        }
    }
}

/// The search it offers: an API and your key.
#[derive(Duplicate, Debug)]
pub struct Search {
    pub provider: Provider,
    #[duplicate]
    key: link_secretive::Secret,
    url: String,
}

impl Search {
    /// The search the environment gives: `LNK_WEB_SEARCH` names the API
    /// when it's set, else the first whose key is set, Brave's then
    /// Tavily's. None without a key: there's no search to offer.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        // A key, read as a Secret: out of the environment once the program
        // guards itself, and kept.
        let key_of = |provider: &Provider| {
            link_secretive::Secret::from_env(provider.key_env())
                .map(|k| k.expose_map(|k| k.trim().to_string()))
                .filter(|k| !k.is_empty())
        };
        let provider = match var(SEARCH_ENV).as_deref() {
            Some("brave") => Provider::Brave,
            Some("tavily") => Provider::Tavily,
            Some(other) => bail!(
                "{SEARCH_ENV} is brave or tavily, not {:?}",
                link_format::clean(other)
            ),
            None => match [Provider::Brave, Provider::Tavily]
                .into_iter()
                .find(|p| key_of(p).is_some())
            {
                Some(provider) => provider,
                None => return Ok(None),
            },
        };
        let key = key_of(&provider).with_context(|| {
            format!(
                "{SEARCH_ENV} is {} but {} isn't set",
                provider.name(),
                provider.key_env()
            )
        })?;
        let url = match var(SEARCH_URL_ENV).filter(|_| cfg!(debug_assertions)) {
            Some(url) => test_url(&url)?,
            None => provider.url().to_string(),
        };
        Ok(Some(Self { provider, key, url }))
    }
}

/// The search API's address a test gives (`LNK_WEB_SEARCH_URL`, read only
/// by a debug build): https, or http only to this machine's loopback,
/// where the key never crosses a network.
fn test_url(url: &str) -> anyhow::Result<String> {
    let parsed = Url::parse(url).with_context(|| format!("{SEARCH_URL_ENV} isn't a URL"))?;
    let host = parsed.host_str().unwrap_or_default();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let loopback = match bare.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => bare.eq_ignore_ascii_case("localhost"),
    };
    ensure!(
        parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback),
        "{SEARCH_URL_ENV} is https, or http to this machine's loopback, not {}",
        link_format::clean(url)
    );
    Ok(url.to_string())
}

/// How recent a search's results are: `search`'s declared `freshness`.
pub use link_web_tool_contract::WebToolSearchArgsFreshness as Freshness;

/// A search's result.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// When it was published, as the API says it (`2 days ago`, a date).
    pub age: Option<String>,
}

impl Web {
    /// Searches the web for `query`: at most `count` results, as recent
    /// as `freshness` says.
    pub fn search(
        &self,
        query: &str,
        count: usize,
        freshness: Option<Freshness>,
    ) -> anyhow::Result<Vec<Hit>> {
        let Some(search) = &self.search else {
            bail!("no search API: its key isn't in the tool's environment");
        };
        let query = query.trim();
        ensure!(!query.is_empty(), "search needs a 'query'");
        let count = count.clamp(1, crate::limits::results_max());
        let client = self.clients.search()?;
        let request = match search.provider {
            Provider::Brave => {
                let mut query = vec![("q", query.to_string()), ("count", count.to_string())];
                if let Some(freshness) = freshness {
                    let code = match freshness {
                        Freshness::Day => "pd",
                        Freshness::Week => "pw",
                        Freshness::Month => "pm",
                        Freshness::Year => "py",
                    };
                    query.push(("freshness", code.into()));
                }
                client
                    .get(&search.url)
                    .query(&query)
                    .header("X-Subscription-Token", search.key.expose())
                    .header(reqwest::header::ACCEPT, "application/json")
            }
            Provider::Tavily => {
                let mut body = json!({"query": query, "max_results": count});
                if let Some(freshness) = freshness {
                    body["time_range"] = json!(match freshness {
                        Freshness::Day => "day",
                        Freshness::Week => "week",
                        Freshness::Month => "month",
                        Freshness::Year => "year",
                    });
                }
                client
                    .post(&search.url)
                    .bearer_auth(search.key.expose())
                    .json(&body)
            }
        };
        let name = search.provider.name();
        let response = request
            .send()
            .map_err(|err| anyhow::anyhow!("{}", without_url(&err)))
            .with_context(|| format!("asking {name}'s search API"))?;
        let status = response.status();
        if matches!(status.as_u16(), 401 | 403) {
            bail!(
                "{name}'s search API refused the key ({status}): check {} in the tool's environment",
                search.provider.key_env()
            );
        }
        if status.as_u16() == 429 {
            bail!("{name}'s search API says too many searches for now ({status}): try again later");
        }
        ensure!(status.is_success(), "{name}'s search API answered {status}");
        let mut body = String::new();
        response
            .take(crate::limits().page_max_bytes)
            .read_to_string(&mut body)
            .with_context(|| format!("reading {name}'s answer"))?;
        let answer: Value = serde_json::from_str(&body)
            .with_context(|| format!("{name}'s search API answered something other than JSON"))?;
        let mut hits = hits(search.provider, &answer);
        hits.truncate(count);
        Ok(hits)
    }
}

/// A search API's results, in our words.
fn hits(provider: Provider, answer: &Value) -> Vec<Hit> {
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    let results = match provider {
        Provider::Brave => answer.pointer("/web/results"),
        Provider::Tavily => answer.get("results"),
    };
    results
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let url = text(r, "url")?;
            Some(Hit {
                title: plain(&text(r, "title").unwrap_or_default()),
                url,
                snippet: plain(
                    &text(r, "description")
                        .or_else(|| text(r, "content"))
                        .unwrap_or_default(),
                ),
                age: text(r, "age")
                    .or_else(|| text(r, "published_date"))
                    .filter(|a| !a.is_empty()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test's search address carries the key only over https, or to
    /// this machine's loopback.
    #[test]
    fn a_tests_search_address_is_https_or_loopback() {
        for url in [
            "https://search.example/api",
            "http://127.0.0.1:9/brave",
            "http://[::1]:9/x",
            "http://localhost:9/x",
        ] {
            assert_eq!(test_url(url).unwrap(), url);
        }
        for url in [
            "http://search.example/api",
            "ftp://127.0.0.1/x",
            "not a url",
        ] {
            assert!(test_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn each_api_answers_as_hits() {
        let brave = json!({"web": {"results": [
            {"title": "A <b>story</b>", "url": "https://a.example/1",
             "description": "What <strong>happened</strong>", "age": "2 hours ago"},
            {"title": "No URL"},
        ]}});
        assert_eq!(
            hits(Provider::Brave, &brave),
            vec![Hit {
                title: "A story".into(),
                url: "https://a.example/1".into(),
                snippet: "What happened".into(),
                age: Some("2 hours ago".into()),
            }]
        );
        let tavily = json!({"results": [
            {"title": "B", "url": "https://b.example", "content": "Body", "published_date": ""},
        ]});
        assert_eq!(hits(Provider::Tavily, &tavily)[0].snippet, "Body");
        assert_eq!(hits(Provider::Tavily, &tavily)[0].age, None);
        assert!(hits(Provider::Brave, &json!({})).is_empty());
    }
}
