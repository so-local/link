//! Where the desktop's X server is looked for and how long it may take,
//! and its size, read once from `limits.toml`.

use serde::Deserialize;

/// What the desktop is and waits within.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Limits {
    /// The desktop's width, in pixels.
    pub width: u16,
    /// The desktop's height, in pixels.
    pub height: u16,
    /// The folders `Xvfb` is looked for in when no program is named.
    pub xvfb_folders: Vec<String>,
    /// The folders of the system's programs, every sandbox's already.
    pub system_folders: Vec<String>,
    /// How long one reply from the X server may take, in milliseconds.
    pub reply_ms: u64,
    /// How much of `Xvfb`'s errors is kept, in bytes.
    pub errors_kept: usize,
    /// How long the display may take to start, in milliseconds.
    pub start_ms: u64,
    /// How often a starting server is asked whether it serves.
    pub start_poll_ms: u64,
    /// How long a stopping server may take before it's killed.
    pub stop_ms: u64,
    /// How often a stopping server is asked whether it ended.
    pub stop_poll_ms: u64,
    /// The folder every X server's socket is in.
    pub sockets: String,
    /// The display's lock, `{display}` its number.
    pub lock: String,
    /// The steps a drag moves in.
    pub drag_steps: u16,
    /// The pause between a drag's steps, in milliseconds.
    pub drag_step_ms: u64,
    /// How long programs are given to read a changed key mapping.
    pub key_mapping_ms: u64,
    /// The wheel's clicks a scroll turns when it names none.
    pub scroll_clicks: u32,
    /// How many parents are followed to the process this tool started.
    pub parents_max: usize,
}

impl Limits {
    /// Display `display`'s lock.
    pub fn lock(&self, display: u32) -> std::path::PathBuf {
        self.lock.replace("{display}", &display.to_string()).into()
    }
}

/// The desktop's width and height, in pixels, as `limits.toml` says.
pub fn size() -> (u16, u16) {
    (limits().width, limits().height)
}

/// The limits, from `limits.toml`.
pub fn limits() -> &'static Limits {
    static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("limits.toml")).expect("limits.toml is the desktop's limits")
    });
    &LIMITS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limits_read() {
        let limits = limits();
        assert_eq!(size(), (1280, 800));
        let lock = limits.lock(120);
        assert!(
            lock.is_absolute() && lock.ends_with(".X120-lock"),
            "{lock:?}"
        );
        assert!(std::path::Path::new(&limits.sockets).is_absolute());
        assert!(limits.drag_steps > 0 && limits.parents_max > 0);
        assert!(limits.xvfb_folders.iter().all(|f| f.starts_with('/')));
        assert!(limits.reply_ms > 0);
    }
}
