//! What the tool admits from its caller and the pages it reads, and the
//! search APIs it calls, read once from `limits.toml`.

use std::time::Duration;

use serde::Deserialize;

/// The limits it reads its caller's messages and pages within.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Limits {
    /// The longest message line, in bytes.
    pub line_max_bytes: usize,
    /// How much of a page's start a `<meta>` charset is looked for in.
    pub meta_scan_bytes: usize,
    /// How much of a body of no type is looked at for HTML.
    pub sniff_bytes: usize,
    /// How much of a page is read at a time.
    pub read_chunk_bytes: usize,
    /// The longest character reference's name read.
    pub entity_name_max: usize,
    /// How much of a page a fetch returns at once when the spec doesn't
    /// say: Link's spec's.
    pub fetch_bytes: usize,
    /// The least and the most a fetch returns at once, whatever the spec
    /// says.
    pub fetch_least_bytes: usize,
    pub fetch_max_bytes: usize,
    /// The most of a page it reads.
    pub page_max_bytes: u64,
    /// The most redirects followed for one page.
    pub redirects: usize,
    timeout_seconds: u64,
    connect_seconds: u64,
    /// How many results a search gives when not asked.
    pub results: usize,
    /// How many pages are kept for their next parts.
    pub pages: usize,
    page_kept_seconds: u64,
    /// The most calls answered at once.
    pub calls: usize,
    /// How many pages are made markdown at once.
    pub conversions: usize,
    /// The deepest a page's elements may nest for its markdown.
    pub depth_max: usize,
    /// The most hosts a pages client keeps pinned.
    pub pinned_max: usize,
    /// The search APIs' addresses.
    pub addresses: Addresses,
}

/// The search APIs' addresses.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Addresses {
    pub brave: String,
    pub tavily: String,
}

impl Limits {
    /// How long a page or a search may take, its body included.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_seconds)
    }

    /// How long connecting may take.
    pub fn connect(&self) -> Duration {
        Duration::from_secs(self.connect_seconds)
    }

    /// How long a page is kept for its next parts.
    pub fn page_kept(&self) -> Duration {
        Duration::from_secs(self.page_kept_seconds)
    }
}

/// The limits, from `limits.toml`.
pub fn limits() -> &'static Limits {
    static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("limits.toml")).expect("limits.toml is web's limits")
    });
    &LIMITS
}

/// The most results a search gives: `search`'s declared `count` maximum
/// (`actions.toml`), its one home.
pub fn results_max() -> usize {
    static MOST: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| {
        let template = link_actions::tools::Template::parse(link_web_tool_contract::TOOLS)
            .expect("web's declared calls read");
        template
            .tools
            .iter()
            .find(|tool| tool.name == "search")
            .and_then(|search| search.input_schema["properties"]["count"]["maximum"].as_u64())
            .expect("search declares its count's maximum") as usize
    });
    *MOST
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_limits_read() {
        let limits = super::limits();
        assert!(limits.line_max_bytes >= 64 << 10);
        assert!(limits.meta_scan_bytes > 0 && limits.sniff_bytes > 0);
        assert!(limits.read_chunk_bytes > 0 && limits.entity_name_max > 0);
        assert!(limits.fetch_least_bytes <= limits.fetch_bytes);
        assert!(limits.fetch_bytes <= limits.fetch_max_bytes);
        assert!(limits.fetch_max_bytes as u64 <= limits.page_max_bytes);
        assert!(limits.connect() <= limits.timeout());
        assert!(limits.redirects > 0 && limits.pages > 0 && limits.calls > 0);
        assert!(limits.conversions > 0 && limits.depth_max > 0 && limits.pinned_max > 0);
        assert!(!limits.page_kept().is_zero());
        for address in [&limits.addresses.brave, &limits.addresses.tavily] {
            assert!(address.starts_with("https://"), "{address}");
        }
        assert!((1..=super::results_max()).contains(&limits.results));
    }

    /// A fetch's default part is Link's spec's `[tool.web] fetch`, and
    /// its bounds the spec's own check's: one value, said twice, held
    /// together here.
    #[test]
    fn the_fetch_sizes_are_the_specs() {
        let limits = super::limits();
        let link = link_agent_spec::link().tool("web").fetch.unwrap();
        assert_eq!(
            link_agent_spec::size(&link).unwrap(),
            limits.fetch_bytes as u64
        );
        let spec = |bytes: usize| {
            link_agent_spec::parse(&format!("schema = 1\n[tool.web]\nfetch = \"{bytes} B\"\n"))
        };
        for bytes in [limits.fetch_least_bytes, limits.fetch_max_bytes] {
            assert!(spec(bytes).is_ok(), "{bytes}");
        }
        for bytes in [limits.fetch_least_bytes - 1, limits.fetch_max_bytes + 1] {
            assert!(spec(bytes).is_err(), "{bytes}");
        }
    }
}
