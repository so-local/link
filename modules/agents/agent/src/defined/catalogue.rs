//! The current sandbox tool catalogue, shared by grants and read identity.

use link_format::WithinExt;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DefinedTool {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(rename = "ask")]
    _ask: DefinedAsk,
    #[serde(default)]
    pub allow: Vec<String>,
    /// The calls that ask by one boolean argument (`schedule.recurring`).
    #[serde(default, rename = "ask_if")]
    _ask_if: Vec<String>,
    /// One server for every sandbox naming it.
    #[serde(default, rename = "shared")]
    _shared: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum DefinedAsk {
    All,
    None,
}

pub(crate) fn list_tools() -> anyhow::Result<Vec<DefinedTool>> {
    let output = link_core::lnk()
        .args(["sandbox", "tool", "list", "--json"])
        .stderr(std::process::Stdio::inherit())
        .output()
        .within("running lnk sandbox tool list")?;
    anyhow::ensure!(output.status.success(), "lnk sandbox tool list failed");
    String::from_utf8(output.stdout)?
        .lines()
        .map(|line| serde_json::from_str(line).within("reading a defined tool's identity"))
        .collect()
}

#[cfg(test)]
mod tests {
    /// Every field `lnk sandbox tool list --json` writes is read, and the
    /// ones it leaves out when empty are optional: a scheduler's asking
    /// argument once made every listing unreadable.
    #[test]
    fn a_listed_tool_is_read_whatever_it_sets() {
        for line in [
            r#"{"name":"scheduler","argv":["/s"],"ask":"none","allow":["list"],"ask_if":["schedule.recurring"],"shared":true}"#,
            r#"{"name":"bare","argv":["/b"],"ask":"all"}"#,
        ] {
            let tool: super::DefinedTool = serde_json::from_str(line).unwrap();
            assert!(!tool.name.is_empty() && !tool.argv.is_empty());
        }
    }
}
