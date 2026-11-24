//! `lnk-aws connect`: an AWS account, from your aws login (a profile) or
//! an access key, checked with AWS. Questions go to stderr: stdout is the
//! account, as JSON.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::bail;
use link_aws::flags::AwsConnectArgs as Connect;
use link_cloud_contract::Account;
use link_format::{Exit, Note, Out, Question, Report, WithinExt, cmd, prose, sentence};

pub fn connect(out: &mut Out, options: Connect) -> Result<Account, Report> {
    let home = link_files::try_home_dir()?;
    let login = options.key_id.is_none() && has_aws_login(&home, options.profile.as_deref());
    let profile = login.then(|| options.profile.clone().unwrap_or_else(|| "default".into()));
    let region = options
        .region
        .clone()
        .or_else(|| std::env::var("AWS_REGION").ok())
        .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
        .or_else(|| aws_region(&home, profile.as_deref()))
        .unwrap_or_else(|| "us-east-1".into());
    // Held as secrets from the start: the access key among them.
    let mut env: BTreeMap<String, link_secretive::Secret> =
        BTreeMap::from([("AWS_REGION".to_string(), region.clone().into())]);
    let login_text = match &profile {
        Some(profile) => {
            env.insert("AWS_PROFILE".into(), profile.clone().into());
            format!("your aws login (profile {profile})")
        }
        None => {
            out.block(Note(sentence!(
                "There is no aws login here, so Link needs an access key for this account, \
                 from the AWS console under IAM."
            )));
            let id = match &options.key_id {
                Some(id) => id.clone(),
                None => out
                    .ask(
                        Question::text(sentence!("AWS access key id?"))
                            .flag("--key-id")
                            .why(prose!("Or sign in with aws configure first.")),
                    )?
                    .trim()
                    .to_string(),
            };
            let secret = out.ask(
                Question::hidden(sentence!("AWS secret access key?"))
                    .variable("AWS_SECRET_ACCESS_KEY")
                    .why(prose!(
                        "Or sign in with aws configure first, and Link uses that login."
                    )),
            )?;
            if id.is_empty() || secret.is_empty() {
                return Err(
                    Report::error(sentence!("A key id and a secret are needed."))
                        .next(cmd!("lnk cloud connect aws"), "try again")
                        .exit(Exit::Usage),
                );
            }
            let shown = format!("access key {}", masked(&id));
            env.insert("AWS_ACCESS_KEY_ID".into(), id.into());
            env.insert("AWS_SECRET_ACCESS_KEY".into(), secret);
            shown
        }
    };
    let mut account = Account {
        name: options.name,
        kind: "aws".into(),
        place: region.clone(),
        identity: String::new(),
        login: login_text,
        region: Some(region),
        profile,
        project: None,
        settings: BTreeMap::new(),
        env,
        machine: None,
    };
    account.split_settings();
    let aws = match crate::cli::find("aws") {
        Some(aws) => Some(aws),
        None => match crate::install::offer(out) {
            Ok(installed) => installed,
            Err(err) => {
                out.block(link_cloud_contract::warned(
                    sentence!("The AWS CLI didn't install."),
                    err,
                ));
                None
            }
        },
    };
    match aws {
        Some(_) => {
            let cli = crate::cli::Cli::new("aws", account.child_environment())?;
            let who = cli
                .json(&["sts", "get-caller-identity", "--output", "json"])
                .within("checking the sign-in with AWS")?;
            account.identity = format!(
                "AWS account {} ({})",
                who["Account"].as_str().unwrap_or("?"),
                who["Arn"].as_str().unwrap_or("?")
            );
        }
        None => out.block(Note(sentence!(
            "Boxes need the AWS CLI, which isn't installed: lnk cloud connect aws in a \
             terminal installs it, or https://aws.amazon.com/cli (or brew install awscli). \
             Buckets don't."
        ))),
    }
    Ok(account)
}

/// `storage`: the account's own sign-in for its buckets, as rclone's
/// settings for them alone (never saved): its aws login's profile, which
/// the SDK signs in with as the aws CLI does (SSO included), or its access
/// key.
pub fn storage(account: &Account) -> anyhow::Result<link_cloud_contract::Reply> {
    let env = &account.env;
    let mut settings: BTreeMap<String, link_secretive::Secret> = BTreeMap::from([
        ("type".to_string(), "s3".to_string().into()),
        ("provider".to_string(), "AWS".to_string().into()),
    ]);
    match (
        account.setting("AWS_PROFILE"),
        env.get("AWS_ACCESS_KEY_ID"),
        env.get("AWS_SECRET_ACCESS_KEY"),
    ) {
        (Some(profile), ..) => {
            settings.insert("env_auth".into(), "true".to_string().into());
            settings.insert(
                "profile".into(),
                link_secretive::Secret::new(profile.to_string()),
            );
        }
        (None, Some(id), Some(secret)) => {
            settings.insert("access_key_id".into(), id.duplicate());
            settings.insert("secret_access_key".into(), secret.duplicate());
            if let Some(token) = env.get("AWS_SESSION_TOKEN") {
                settings.insert("session_token".into(), token.duplicate());
            }
        }
        _ => bail!(
            "{} has no aws login or access key: lnk cloud connect aws --name {} again",
            link_format::clean(&account.name),
            link_format::clean(&account.name)
        ),
    }
    Ok(link_cloud_contract::Reply {
        credential: settings,
        ..Default::default()
    })
}

/// `AKIA…WXYZ`: enough to recognize a key, not to use it.
fn masked(id: &str) -> String {
    match id.len() {
        0..=8 => "…".into(),
        n => format!("{}…{}", &id[..4], &id[n - 4..]),
    }
}

fn has_aws_login(home: &Path, profile: Option<&str>) -> bool {
    profile.is_some()
        || std::env::var_os("AWS_PROFILE").is_some()
        || home.join(".aws/credentials").is_file()
        || home.join(".aws/config").is_file()
}

/// The region of `profile` (else `default`) in `~/.aws/config`.
fn aws_region(home: &Path, profile: Option<&str>) -> Option<String> {
    let text = std::fs::read_to_string(home.join(".aws/config")).ok()?;
    let header = match profile.unwrap_or("default") {
        "default" => "[default]".to_string(),
        other => format!("[profile {other}]"),
    };
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == header;
        } else if inside
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == "region"
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_come_from_the_profiles_section() {
        let dir = link_test_kit::TempDir::new("aws");
        std::fs::create_dir_all(dir.join(".aws")).unwrap();
        std::fs::write(
            dir.join(".aws/config"),
            "[default]\nregion = us-east-1\n[profile work]\nregion=eu-west-2\n",
        )
        .unwrap();
        assert_eq!(aws_region(&dir, Some("work")).as_deref(), Some("eu-west-2"));
        assert_eq!(aws_region(&dir, None).as_deref(), Some("us-east-1"));
        assert_eq!(aws_region(&dir, Some("other")), None);
        assert_eq!(masked("AKIAABCDEFGHWXYZ"), "AKIA…WXYZ");
    }
}
