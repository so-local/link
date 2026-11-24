//! Successful account sign-in activates only locally declared components.

use link_core::modules::AccountLogin;
use link_format::{Done, Out, Report};

pub(super) async fn run(out: &mut Out) -> Result<Done, Report> {
    let wanted: Vec<_> = link_core::owner::ACCOUNT
        .ask(&())
        .into_iter()
        .filter_map(|answer| answer.reply.ok())
        .flat_map(|login| selected(&crate::registry::REGISTRY, &login))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if !wanted.is_empty() && !crate::installing::missing(&wanted)?.is_empty() {
        return crate::installing::add(out, &wanted).await;
    }
    Ok(Done::shown())
}

fn selected<'a>(
    registry: &'a std::collections::BTreeMap<String, link_component::Resolved>,
    login: &AccountLogin,
) -> Vec<&'a str> {
    let Some(service) = login.account_service() else {
        return vec![];
    };
    registry
        .values()
        .filter(|component| component.after_sign_in(service))
        .map(|component| component.component.id.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_out_and_unrelated_logins_activate_nothing() {
        let mut registry = link_cli::registry().clone();
        let component = registry.values_mut().next().unwrap();
        component.account_services = vec!["https://accounts.example".into()];
        let id = component.component.id.clone();
        assert!(selected(&registry, &AccountLogin::default()).is_empty());
        let mut login = AccountLogin {
            username: Some("alice".into()),
            owner: Some("acct-owner".into()),
            accounts: Some("https://accounts.example".into()),
            ..Default::default()
        };
        assert_eq!(selected(&registry, &login), [id]);
        for origin in [
            "https://another.example",
            "https://accounts.example.evil",
            "https://accounts.example/path",
            "http://accounts.example",
        ] {
            login.accounts = Some(origin.into());
            assert!(selected(&registry, &login).is_empty(), "{origin}");
        }
        login.accounts = Some("https://accounts.example".into());
        login.owner = None;
        assert!(selected(&registry, &login).is_empty());
    }
}
