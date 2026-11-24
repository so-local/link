//! Where Chromium is found: its names and a Mac's places, and distribution
//! launchers with the engines they start. A launcher reads
//! host preferences that are outside the browser's sandbox.
pub(crate) const PACKAGED_CHROMIUMS: &[(&str, &str)] = &[
    ("/usr/bin/chromium", "/usr/lib/chromium/chromium"),
    (
        "/usr/bin/chromium-browser",
        "/usr/lib/chromium-browser/chromium-browser",
    ),
    ("/usr/bin/google-chrome", "/opt/google/chrome/chrome"),
    ("/usr/bin/google-chrome-stable", "/opt/google/chrome/chrome"),
];

/// The names Chromium goes by on a PATH, the commonest first.
pub(crate) const CHROMIUMS: &[&str] = &[
    "chromium",
    "chromium-browser",
    "google-chrome",
    "google-chrome-stable",
    "microsoft-edge",
    "brave-browser",
];

/// Where a Mac keeps them.
pub(crate) const MAC_CHROMIUMS: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
];

/// The folders of the system's programs, on its PATH: a browser's
/// helpers are never allowed by its folder when it is one of these.
pub(crate) const PROGRAM_FOLDERS: &[&str] = &[
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/snap/bin",
];
