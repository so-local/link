//! A trusted host may hold an exact conditional object write without changing baseline policy.
use super::*;
use link_interactions::{
    approval::EffectOutcome,
    sources::{Application, ReviewedEffect},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedWrite {
    pub owner: String,
    pub agent: String,
    pub team: Option<TeamRef>,
    pub path: String,
    pub data: Vec<u8>,
    pub expect: Expect,
}

impl PreparedWrite {
    pub fn payload(&self) -> anyhow::Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }
    pub fn claim(&self, reviewed: ReviewedEffect) -> anyhow::Result<Application> {
        ensure!(
            self.expect != Expect::Any,
            "a reviewed write needs an exact expected object version"
        );
        reviewed.claim(&self.payload()?, true)
    }
}

pub(super) fn freeze(
    store: &dyn Store,
    who: &Identity,
    team: Option<&TeamRef>,
    path: &str,
    data: &[u8],
    expect: &Expect,
) -> anyhow::Result<PreparedWrite> {
    let expect = if *expect == Expect::Any {
        match store.get(&format!("{HOME}{path}"))? {
            Some((_, version)) => Expect::Version(version),
            None => Expect::Absent,
        }
    } else {
        expect.clone()
    };
    Ok(PreparedWrite {
        owner: who.owner.clone(),
        agent: who.agent.clone(),
        team: team.cloned(),
        path: path.into(),
        data: data.to_vec(),
        expect,
    })
}

pub(super) fn report(
    application: Option<Application>,
    result: &anyhow::Result<Version>,
) -> anyhow::Result<()> {
    if let Some(application) = application {
        application.report(match result {
            Ok(version) => EffectOutcome::Applied {
                detail: format!("Committed the exact reviewed bytes at version {version}"),
            },
            Err(err) if is_conflict(err) => EffectOutcome::Failed {
                reason: "object changed during review; no reviewed write was committed".into(),
            },
            Err(err) => EffectOutcome::Unknown {
                reason: format!("object write did not confirm its outcome: {err:#}"),
            },
        })?;
    }
    Ok(())
}
