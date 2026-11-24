//! The sandbox's own routes, which `lnk-sandbox`'s bridge serves: tools
//! that run outside (`/_link/mcp/<name>`, MCP's HTTP transport) and a
//! page opened in your browser (`/_link/web/open`), each after asking you as
//! it must, both behind the proxy's URL secret.

use serde::{Deserialize, Serialize};

use crate::route::{Bridge, Route, url_of};

/// Where the tools are on a proxy, each by its name.
pub const TOOLS: &str = "/_link/mcp/";

/// The largest message a tool's MCP server and the sandbox's tools bridge
/// pass each other, each way, in bytes: a tool's answer past it ends its
/// session, so a tool keeps its answers under it.
pub const TOOL_MESSAGE_MAX: usize = 8 * 1024 * 1024;

/// Where a program asks to open a page.
pub const OPEN: &str = "/_link/web/open";

/// The variable holding where a program asks to open a page.
pub const OPEN_VARIABLE: &str = "LNK_SANDBOX_OPEN";

/// The bridge that serves the sandbox's own routes here: `lnk sandbox
/// bridge`.
pub fn bridge() -> anyhow::Result<Bridge> {
    Bridge::of("sandbox", None)
}

/// Whom a sandbox's questions are asked for, beside you on this
/// machine: the agent it runs, whose owner's chats are asked too, and how
/// long each question waits for an answer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asking {
    pub agent: String,
    pub wait_secs: u64,
}

/// What the tools route's bridge is given: the tools it may call, and
/// your desktop session's variables, where it asks you on the screen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tools {
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub desktop: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asking: Option<Asking>,
}

/// What the open route's bridge is given: the only hosts a page may be
/// on (`["*"]`, any), and your desktop session's variables, where it
/// asks you and opens your browser.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Open {
    pub hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub desktop: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asking: Option<Asking>,
}

/// The tools route of `tools`, served by `bridge`: each reached at
/// `$LNK_TOOL_<NAME>` ([`tool_variable`]).
pub fn tools(bridge: Bridge, given: Tools) -> anyhow::Result<Route> {
    for tool in &given.tools {
        check_tool_name(tool)?;
    }
    let env = given
        .tools
        .iter()
        .map(|tool| {
            (
                tool_variable(tool),
                format!("{}{tool}", url_of(TOOLS, true)),
            )
        })
        .collect();
    Ok(Route {
        name: "mcp".into(),
        prefix: TOOLS.into(),
        needs_url_secret: true,
        bridge,
        given: serde_json::to_string(&given)?.into(),
        env,
        ..Route::default()
    })
}

/// The open route, served by `bridge`: `$BROWSER` is the sandbox's own
/// program, which asks it, and `$LNK_SANDBOX_OPEN` where.
pub fn open(bridge: Bridge, given: Open) -> anyhow::Result<Route> {
    for host in given.hosts.iter().filter(|h| *h != "*") {
        crate::net::check_host(host)?;
    }
    let env = [
        ("BROWSER".to_string(), "{exe}".to_string()),
        (OPEN_VARIABLE.to_string(), url_of(OPEN, true)),
    ]
    .into();
    Ok(Route {
        name: "open".into(),
        prefix: OPEN.into(),
        needs_url_secret: true,
        bridge,
        given: serde_json::to_string(&given)?.into(),
        env,
        ..Route::default()
    })
}

/// Where a program behind a proxy reaches tool `name`, its proxy on
/// `port` whose URL secret is `secret`: an MCP server over HTTP.
pub fn tool_url(port: u16, name: &str, secret: &str) -> String {
    format!(
        "http://127.0.0.1:{port}{}{secret}/mcp/{name}",
        crate::route::SECRET_PREFIX
    )
}

/// The variable a program finds tool `name`'s URL in: `LNK_TOOL_GH`.
pub fn tool_variable(name: &str) -> String {
    format!("LNK_TOOL_{}", name.to_ascii_uppercase().replace('-', "_"))
}

/// A tool's name, or why it isn't one: lowercase letters, digits and
/// dashes, starting with a letter, at most 40 (`gh`, `notes`, `aws-prod`).
pub fn check_tool_name(name: &str) -> anyhow::Result<()> {
    let plain = (1..=40).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    anyhow::ensure!(
        plain,
        "{name:?} isn't a tool's name: lowercase letters, digits and dashes, such as gh or notes"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_tool_is_reached_behind_the_url_secret() {
        let secret = crate::route::url_secret().unwrap();
        assert_eq!(
            super::tool_url(40781, "gh", &secret),
            format!("http://127.0.0.1:40781/_link/url/{secret}/mcp/gh")
        );
    }
}
