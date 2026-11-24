//! The browser's limits and lists (`browser.toml`), read once.

/// What `browser.toml` holds.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    /// How much of a page's text a read returns at once, in bytes.
    pub read_bytes: usize,
    call_ms: u64,
    start_ms: u64,
    load_ms: u64,
    settle_ms: u64,
    load_poll_ms: u64,
    download_ms: u64,
    /// How far a scroll moves the page, as a share of the window's height.
    pub scroll_share: f64,
    scroll_ms: u64,
    close_ms: u64,
    end_ms: u64,
    end_poll_ms: u64,
    /// The window's size in pixels, which a screenshot is.
    pub width: u32,
    pub height: u32,
    /// The oldest Chromium major version started.
    pub minimum_major: u32,
    /// The most bytes of the page's words in one note.
    pub note_bytes: usize,
    /// The most bytes of notes in one answer.
    pub notes_bytes: usize,
    /// The most bytes of DevTools events kept unread for one page.
    pub events_bytes: usize,
    /// The most bytes one DevTools message may hold.
    pub message_bytes: usize,
    /// The DevTools events the browser reads.
    pub events: Vec<String>,
    /// The most conversations' pages kept open at once.
    pub pages_max: usize,
    idle_ms: u64,
    /// Your own browsers' profiles, under your home.
    pub own_profiles: Vec<String>,
    /// What a browser's line naming why it stopped holds.
    pub causes: Vec<String>,
    /// The keys `key` presses, by name.
    pub keys: std::collections::BTreeMap<String, Key>,
    /// The modifiers a `key` combination holds, by name.
    pub modifiers: std::collections::BTreeMap<String, Modifier>,
}

impl Settings {
    /// How long a call to the browser may take.
    pub fn call(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.call_ms)
    }

    /// How long the browser may take to start.
    pub fn start(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.start_ms)
    }

    /// How long a page may take to load before its answer says so.
    pub fn load(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.load_ms)
    }

    /// How long an action waits to see whether it starts loading a page.
    pub fn settle(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.settle_ms)
    }

    /// How long a conversation's page stays open unused.
    pub fn idle(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.idle_ms)
    }

    /// How often a loading page is looked at for its dialogs.
    pub fn load_poll(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.load_poll_ms)
    }

    /// How long a page that stopped without loading waits to tell of a
    /// download.
    pub fn download(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.download_ms)
    }

    /// How long a scroll waits for the page to draw.
    pub fn scroll(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.scroll_ms)
    }

    /// How long the browser is given to close.
    pub fn close(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.close_ms)
    }

    /// How long a closing browser is given to end.
    pub fn end(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.end_ms)
    }

    /// How often a closing browser is looked at.
    pub fn end_poll(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.end_poll_ms)
    }
}

/// A key `key` presses, as DevTools' `Input.dispatchKeyEvent` takes it.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Key {
    pub code: String,
    #[serde(rename = "virtual")]
    pub virtual_key: u32,
    #[serde(default)]
    pub text: String,
}

/// A modifier a `key` combination holds, as DevTools takes it.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Modifier {
    pub code: String,
    #[serde(rename = "virtual")]
    pub virtual_key: u32,
    /// Its bit in `Input.dispatchKeyEvent`'s `modifiers`.
    pub bit: u32,
}

/// `browser.toml`, read once.
pub(crate) fn get() -> &'static Settings {
    static READ: std::sync::OnceLock<Settings> = std::sync::OnceLock::new();
    READ.get_or_init(|| toml::from_str(include_str!("browser.toml")).expect("browser.toml reads"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn browser_toml_reads() {
        let settings = super::get();
        assert!(settings.minimum_major > 0);
        assert!(settings.note_bytes < settings.notes_bytes);
        assert!(settings.notes_bytes < settings.read_bytes);
        assert!(settings.call() <= settings.start());
        assert!(settings.settle() < settings.load());
        assert!(settings.load_poll() < settings.load() && settings.download() < settings.call());
        assert!(settings.scroll_share > 0.0 && settings.scroll_share <= 1.0);
        assert!(settings.scroll() < settings.call());
        assert!(settings.end_poll() < settings.end() && settings.close() <= settings.call());
        let bits: Vec<u32> = settings.modifiers.values().map(|m| m.bit).collect();
        assert!(bits.iter().all(|b| b.is_power_of_two()), "{bits:?}");
        assert!(settings.events_bytes < settings.message_bytes);
        assert!(settings.pages_max > 0 && !settings.events.is_empty());
        assert!(settings.width > 0 && settings.height > 0);
        assert!(!settings.own_profiles.is_empty());
    }

    /// `key`'s declaration lists each key named here, and each modifier,
    /// so the model reads what it may press.
    #[test]
    fn keys_declaration_names_each_key() {
        let actions: toml::Table = toml::from_str(include_str!("../actions.toml")).unwrap();
        let said = actions["actions"]["browser-tool.browser-tool.key"]["description"]
            .as_str()
            .unwrap();
        let settings = super::get();
        for name in settings.keys.keys().chain(settings.modifiers.keys()) {
            assert!(
                said.contains(name.as_str()),
                "{name} isn't in key's description"
            );
        }
    }
}
