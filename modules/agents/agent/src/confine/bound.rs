//! The agent bound for one `lnk agent harness run`, while no start binds
//! it.

use link_agent::settings::Settings;
use link_format::WithinExt;

/// The agent's binding for one harness run, held until it ends: its
/// proxy's calls are proved as the agent, its service named, so each is
/// granted what the agent is granted at that call. A running agent's
/// binding is used as it is. A stopped agent is bound for its runs, every
/// run meanwhile sharing that binding, and the last to end unbinds it,
/// unless a start has bound it since ([`link_sandbox_binding::Keyring::hold`]).
pub(crate) struct RunBinding {
    _held: link_sandbox_binding::Held,
}

impl RunBinding {
    /// Agent `settings` bound in `keyring` for this run. A bind that fails
    /// fails the run: its calls are never proved as a person's run.
    pub(crate) fn of(
        keyring: &link_sandbox_binding::Keyring,
        settings: &Settings,
    ) -> anyhow::Result<Self> {
        let key = link_secretive::Secret::new(link_files::random_token()?);
        let held = keyring
            .hold(
                &settings.identity(),
                &key,
                link_secret_contract::keeper::RouteRevoker,
            )
            .within("binding its key for this run")?;
        Ok(Self { _held: held })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stopped(dir: &link_test_kit::TempDir) -> (link_sandbox_binding::Keyring, Settings) {
        let keyring = link_sandbox_binding::Keyring::at(&dir.path().join("keys"));
        let mut settings = Settings::default();
        settings.name = "ada".into();
        settings.owner = Some("someone@here".into());
        settings.service = Some("s1s2s3s4s5s6".into());
        (keyring, settings)
    }

    /// A stopped agent's harness run is proved as the agent, its service
    /// named, never as a person's run with none; once it ends, nothing is
    /// left bound.
    #[test]
    fn a_stopped_agent_s_run_is_proved_as_the_agent_until_it_ends() {
        let _config = crate::tests::own_config();
        let dir = link_test_kit::TempDir::new("run-binding");
        let (keyring, settings) = stopped(&dir);
        let run = RunBinding::of(&keyring, &settings).unwrap();
        let caller = keyring.bound(&settings.key()).expect("bound for the run");
        assert_eq!(caller.identity.service, "s1s2s3s4s5s6");
        assert_eq!(caller.identity.agent, settings.key());
        drop(run);
        assert!(keyring.bound(&settings.key()).is_none());
    }

    /// A start's binding is the run's caller, and stays when the run
    /// ends, whether it was there before the run or made during it.
    #[test]
    fn a_start_s_binding_outlives_the_run() {
        let _config = crate::tests::own_config();
        let dir = link_test_kit::TempDir::new("run-binding");
        let (keyring, settings) = stopped(&dir);
        let started = link_secretive::Secret::new("s".repeat(32));
        keyring.bind(&settings.identity(), &started).unwrap();
        drop(RunBinding::of(&keyring, &settings).unwrap());
        assert!(keyring.bound(&settings.key()).unwrap().key == started);
        keyring.unbind(&settings.key(), &link_secret_contract::keeper::RouteRevoker);
        let run = RunBinding::of(&keyring, &settings).unwrap();
        keyring.bind(&settings.identity(), &started).unwrap();
        drop(run);
        assert!(keyring.bound(&settings.key()).unwrap().key == started);
    }

    /// Two runs of a stopped agent at once share one binding: the second
    /// is still the agent once the first ends, and the last to end unbinds
    /// it. A start's binding, made while they run, outlives them both.
    #[test]
    fn overlapping_runs_share_a_binding_the_last_unbinds() {
        let _config = crate::tests::own_config();
        let dir = link_test_kit::TempDir::new("run-binding");
        let (keyring, settings) = stopped(&dir);
        let first = RunBinding::of(&keyring, &settings).unwrap();
        let bound = keyring.bound(&settings.key()).expect("bound for the runs");
        let second = RunBinding::of(&keyring, &settings).unwrap();
        drop(first);
        let still = keyring
            .bound(&settings.key())
            .expect("the second run is still the agent");
        assert!(still.key == bound.key);
        assert_eq!(still.identity.service, "s1s2s3s4s5s6");
        drop(second);
        assert!(keyring.bound(&settings.key()).is_none());
        // A start during two runs wins, and outlives them both.
        let first = RunBinding::of(&keyring, &settings).unwrap();
        let second = RunBinding::of(&keyring, &settings).unwrap();
        let started = link_secretive::Secret::new("s".repeat(32));
        keyring.bind(&settings.identity(), &started).unwrap();
        let third = RunBinding::of(&keyring, &settings).unwrap();
        drop(first);
        drop(third);
        drop(second);
        assert!(keyring.bound(&settings.key()).unwrap().key == started);
    }

    /// An agent that can't be bound doesn't run: it's never proved as a
    /// person's run instead.
    #[test]
    fn an_agent_that_cant_be_bound_doesnt_run() {
        let _config = crate::tests::own_config();
        let dir = link_test_kit::TempDir::new("run-binding");
        let (keyring, mut settings) = stopped(&dir);
        settings.owner = Some("not an owner!".into());
        let refused = RunBinding::of(&keyring, &settings)
            .err()
            .expect("it was bound");
        assert!(
            format!("{refused:#}").contains("binding its key"),
            "{refused:#}"
        );
        assert!(keyring.bound(&settings.key()).is_none());
    }
}
