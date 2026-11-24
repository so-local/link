//! An agent's key for this start, bound where its proxy's routes are
//! proved.

use link_format::{Report, WithinExt};

use crate::settings::Settings;

/// Binds `settings`' agent with a key of this start's own, which its
/// proxy's routes prove its calls with when it has no bridge. A bind that
/// fails fails the start: an agent's routes are never called as a
/// person's run, whose tools and pages are fixed when its proxy starts,
/// so a grant taken back would keep holding.
pub(super) fn bind_routes(
    keys: &link_sandbox_binding::Keyring,
    settings: &Settings,
) -> Result<(), Report> {
    let key = link_secretive::Secret::new(link_files::random_token().within("making its key")?);
    if let Err(err) = keys.bind(&settings.identity(), &key) {
        keys.unbind(&settings.key(), &link_secret_contract::keeper::RouteRevoker);
        Err(err).within("binding its key for this start")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An agent that can't be bound isn't started: its routes would
    /// otherwise be called as a person's run, by what its proxy was
    /// started with rather than what it's granted at each call.
    #[test]
    fn an_agent_that_cant_be_bound_isnt_started() {
        let _config = crate::tests::own_config();
        let dir = link_test_kit::TempDir::new("unbound");
        let keys = link_sandbox_binding::Keyring::at(&dir.path().join("keys"));
        let mut settings = Settings::default();
        settings.name = "unbound".into();
        settings.owner = Some("not an owner!".into());
        let refused = bind_routes(&keys, &settings).expect_err("it was bound");
        assert!(
            refused.headline.to_string().contains("binding its key"),
            "{}",
            refused.headline
        );
        assert!(keys.bound(&settings.key()).is_none());
        settings.owner = Some("someone@here".into());
        bind_routes(&keys, &settings).unwrap();
        assert!(keys.bound(&settings.key()).is_some());
    }
}
