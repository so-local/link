//! A successful capability answer is typed and version-checked before use.
use super::*;

use link_harness_contract::harness::Contract;
use std::sync::Arc;

pub(super) type Answer = Result<Contract, CachedError>;

#[derive(Clone, Debug)]
pub(super) struct CachedError(Arc<anyhow::Error>);
impl std::fmt::Display for CachedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for CachedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

fn checked(name: &str, output: &std::process::Output) -> anyhow::Result<Contract> {
    use anyhow::Context;
    // An adapter without this optional command answers no capabilities.
    // Successful output, however, promises the typed current record.
    if !output.status.success() {
        return Ok(Contract::default());
    }
    let contract: Contract = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("reading lnk-{name} contract"))?;
    link_harness_contract::harness::PROTOCOL.accept(&format!("lnk-{name}"), contract.version)?;
    Ok(contract)
}

impl Adapter {
    /// The adapter's capabilities. Absence of the optional command gives
    /// version zero with no capabilities. A successful malformed or
    /// unsupported answer is an error, including when read from cache.
    pub fn contract(&self) -> anyhow::Result<Contract> {
        use anyhow::Context;
        let answers = Answers::of(&self.program);
        answers
            .contract
            .get_or_init(|| {
                (|| {
                    let output = link_secretive::process::command(&self.program)
                        .arg("contract")
                        .stdin(Stdio::null())
                        .stderr(Stdio::null())
                        .output()
                        .with_context(|| format!("running lnk-{} contract", self.name))?;
                    checked(&self.name, &output)
                })()
                .map_err(|error| CachedError(Arc::new(error)))
            })
            .clone()
            .map_err(anyhow::Error::new)
    }
}

#[cfg(test)]
mod tests;
