//! Team moves compose ordinary agent streams; every member is paused
//! before any switch is sent and the team is activated only when whole.
use anyhow::ensure;
use link_agent::{
    cli, hosted, moved,
    settings::{self, Settings},
};
use link_agent_contract::moving;
use link_agent_contract::moving::Phase;
use link_agent_place as cloud;
use link_agent_place::Account;
use link_agent_place::Request;
use link_agent_place::teams::Action;
use link_agent_place::teams::Movement;
use link_format::WithinExt;
use link_format::{Done, Out, Report, Sentence};
use link_secretive::Duplicate;
use std::{
    io::{Read, Seek, Write},
    path::Path,
};

/// Where a team moves: a cloud account that hosts teams, or here.
fn to(args: &cli::TeamMoveArgs) -> anyhow::Result<String> {
    let slots = cli::slots(&args.on, &args.cloud, &args.zone, args.here, &args.place);
    match slots.target()? {
        Some(link_workplace::Target::Here) => Ok("here".into()),
        Some(link_workplace::Target::Cloud { cloud, zone: None }) => Ok(cloud),
        _ => {
            anyhow::bail!("a team moves to --cloud <cloud>, a cloud that hosts teams, or to --here")
        }
    }
}

pub(super) fn run(out: &mut Out, args: cli::TeamMoveArgs) -> Result<Done, Report> {
    let to = to(&args)?;
    if let Some(said) = resume_finish(out, &args)? {
        return Ok(Done::success(Sentence::adopt(&said)));
    }
    if to == "here" {
        return Ok(Done::success(Sentence::adopt(&fetch_here(out, args)?)));
    }
    Ok(Done::success(Sentence::adopt(&send(out, args)?)))
}

/// The team moved to a cloud account that hosts teams: what to say.
fn send(out: &mut Out, args: cli::TeamMoveArgs) -> anyhow::Result<String> {
    let team = settings::team(&args.team)?;
    ensure!(
        team.members.values().all(|m| m.owner.is_none()),
        "remove outside members before moving a team to a machine"
    );
    let account =
        cloud::account(&to(&args)?).within("name a cloud account that hosts teams, or --here")?;
    let agents: Vec<Settings> = team
        .members
        .values()
        .map(|member| {
            let name = settings::names()?
                .into_iter()
                .find(|n| settings::key_of(n) == member.id)
                .within("every team member must be on this machine")?;
            let settings = Settings::load(&name)?;
            let grants = link_agent::sandbox_file::of(&name).spec.teams;
            for other in settings::teams_of(&member.id)? {
                let reaches = grants
                    .get(&other.name)
                    .is_some_and(|grant| !grant.write.is_empty() || !grant.read.is_empty());
                ensure!(
                    other.id == team.id || other.shared.is_none() || !reaches,
                    "{} also shares team {}: move that team together",
                    settings.name,
                    other.name
                );
            }
            Ok(settings)
        })
        .collect::<anyhow::Result<_>>()?;
    let file = std::fs::read_to_string(settings::team_path(&args.team))?;
    let hosted_agents = agents
        .iter()
        .map(|a| cloud::HostedAgent {
            id: a.agent_id().map(str::to_string).unwrap_or_default(),
            name: a.name.clone(),
            owner: a.owner.clone(),
            state: None,
        })
        .collect();
    let group = super::remote::call(
        &account,
        Action::BeginIn(link_agent_place::teams::TeamActionBeginIn {
            file: file.clone(),
            agents: hosted_agents,
            size: 0,
        }),
    )?
    .movement
    .within("the cloud named no team move")?;
    let mut have = moving::Have::new();
    let mut paused = Vec::new();
    let mut committed = false;
    let done = (|| {
        check(&group)?;
        ensure!(group.file == file, "the cloud returned another team's file");
        for phase in [Phase::Copy, Phase::Switch] {
            if phase == Phase::Switch {
                ensure!(
                    std::fs::read_to_string(settings::team_path(&args.team))? == file,
                    "the team changed during its move: try again"
                );
                for agent in &agents {
                    crate::moving::sending::pause(agent)?;
                    paused.push(agent);
                }
            }
            let mut archive = moving::temporary_file()?;
            let current = team
                .shared
                .as_ref()
                .map(|shared| moving::have_of(shared, "files/shared"))
                .transpose()?
                .unwrap_or_default();
            let removed: Vec<_> = have
                .keys()
                .filter(|p| p.starts_with("files/shared/") && !current.contains_key(*p))
                .cloned()
                .collect();
            let manifest = serde_json::json!({"format":moving::PROTOCOL.current,"phase":phase,"move":group.id,"team":file,"removed":removed});
            let mut tar = tar::Builder::new(&mut archive);
            moving::append_manifest(&mut tar, &serde_json::to_vec(&manifest)?)?;
            for member in &group.members {
                let agent = agents
                    .iter()
                    .find(|a| a.agent_id() == Some(&member.id))
                    .within("the cloud changed the team's members")?;
                let mut stream = moving::temporary_file()?;
                let mut sending = agent.duplicate();
                if phase == Phase::Switch {
                    let without = if member.manages_models() {
                        moved::models::keep(agent)?;
                        sending.model = None;
                        sending.fallback = None;
                        vec![
                            link_agent_identity::own::OwnSecret::Model.to_use()?,
                            link_agent_identity::own::OwnSecret::Fallback.to_use()?,
                        ]
                    } else {
                        moved::models::release(&member.id)?;
                        Vec::new()
                    };
                    link_secret_contract::copied_without(
                        agent.service_id()?,
                        link_secret_contract::Place::of_account(&account.name, None),
                        without,
                    )?;
                }
                crate::moving::sending::send_to(
                    stream.try_clone()?,
                    &sending,
                    phase,
                    Some(member.movement.clone()),
                    have.iter()
                        .filter_map(|(path, hash)| {
                            path.strip_prefix(&format!("files/members/{}/", member.id))
                                .map(|p| (p.to_string(), hash.clone()))
                        })
                        .collect(),
                    if phase == Phase::Switch {
                        Some(member.seal_to.as_ref().within("the hosting account supplied no per-move receiver for this team member")?)
                    } else {
                        None
                    },
                    // A hosting account takes the harness's state at the switch.
                    false,
                )?;
                stream.rewind()?;
                parts(&mut tar, &member.id, &mut stream)?;
            }
            if let Some(shared) = &team.shared {
                moving::append_folder(
                    &mut tar,
                    shared,
                    "files/shared",
                    &have,
                    None,
                    &link_files::layout::Hidden::TransferScratch.path(),
                )?;
            }
            moving::append_end(&mut tar)?;
            tar.finish()?;
            drop(tar);
            archive.rewind()?;
            let mut child = cloud::spawn(
                &account.kind,
                "agent-receive",
                &request(&account, &group, phase),
            )?;
            std::io::copy(
                &mut archive,
                child.stdin.as_mut().within("no stream input")?,
            )?;
            drop(child.stdin.take());
            let output = child.wait_with_output()?;
            ensure!(
                output.status.success(),
                "the cloud refused the team's {}",
                phase.name()
            );
            let reply: cloud::Reply = serde_json::from_slice(&output.stdout)?;
            if !reply.have.is_empty() {
                have = reply.have.clone();
            }
            if phase == Phase::Switch {
                ensure!(
                    reply.answers == Some(true),
                    "the cloud didn't confirm the team's switch"
                );
                committed = true;
            }
        }
        let finish = Finish {
            account: account.name.clone(),
            group: group.clone(),
            incoming: true,
        };
        finish.save()?;
        finish.complete(out)?;
        Ok(format!("Moved team {} to {}.", team.name, account.name))
    })();
    if done.is_err() && !committed {
        let abort = hosted::call(&account, "agent-abort", |r| {
            r.r#move = Some(group.id.clone())
        });
        // Resume only after the cloud confirms it has no active copy.
        abort.within("the cloud couldn't abort the team move; members remain paused")?;
        for agent in paused {
            crate::moving::sending::resume(out, agent);
        }
    }
    done
}

fn parts<W: Write>(
    tar: &mut tar::Builder<W>,
    id: &str,
    file: &mut std::fs::File,
) -> anyhow::Result<()> {
    let mut chunk = 0;
    while file.stream_position()? < file.metadata()?.len() {
        let len = (file.metadata()?.len() - file.stream_position()?).min(64 << 20);
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o600);
        moving::append_file(
            tar,
            &mut header,
            Path::new(&format!("files/members/{id}/{chunk:08}.part")),
            (&mut *file).take(len),
            len,
            &Default::default(),
            &link_files::layout::Hidden::TransferScratch.path(),
        )?;
        chunk += 1;
    }
    Ok(())
}
fn request(account: &Account, group: &Movement, phase: Phase) -> Request {
    Request {
        account: account.duplicate(),
        r#move: Some(group.id.clone()),
        phase: Some(phase.name().into()),
        ..Default::default()
    }
}
fn check(group: &Movement) -> anyhow::Result<()> {
    moving::check_move_id(&group.id)?;
    let team = link_agent_spec::parse_team(&group.file)?;
    ensure!(
        group.members.len() == team.members.len() && !group.members.is_empty(),
        "the cloud omitted team members"
    );
    let mut ids = std::collections::BTreeSet::new();
    let mut movements = std::collections::BTreeSet::from([group.id.as_str()]);
    for member in &group.members {
        ensure!(
            ids.insert(&member.id) && movements.insert(&member.movement),
            "the cloud repeated a team member or move"
        );
        moving::check_move_id(&member.movement)?;
        link_machine::machine::check_name(&member.name)?;
        ensure!(
            team.members
                .values()
                .any(|m| m.id == member.id && m.owner.is_none()),
            "the cloud changed team members"
        );
    }
    Ok(())
}

fn fetch_here(out: &mut Out, args: cli::TeamMoveArgs) -> anyhow::Result<String> {
    let (account, listed) = super::remote::find(&args.team)?;
    ensure!(
        !settings::team_path(&listed.name).exists(),
        "a team of this name is here already"
    );
    let group = super::remote::call(
        &account,
        Action::BeginOut(link_agent_place::teams::TeamActionBeginOut { team: listed.id }),
    )?
    .movement
    .within("the cloud named no move")?;
    let root = link_files::layout::Hidden::TeamMoving
        .path()
        .join(&group.id);
    let recipient = link_secretive::Recipient::new();
    let mut have = moving::Have::new();
    let mut arrived = Vec::new();
    let mut committed = false;
    let result = (|| {
        check(&group)?;
        for member in &group.members {
            ensure!(
                !Settings::path_of(&member.name).exists(),
                "agent {} is here already",
                member.name
            );
        }
        link_files::create_private_dir(&root)?;
        for phase in [Phase::Copy, Phase::Switch] {
            let mut req = request(&account, &group, phase);
            req.seal_to = Some(recipient.seal_to().clone());
            req.have = have.clone();
            let mut child = cloud::spawn(&account.kind, "agent-send", &req)?;
            drop(child.stdin.take());
            let _ = std::fs::remove_dir_all(root.join("files/members"));
            moving::unpack(
                child.stdout.take().within("no stream from cloud")?,
                |manifest, head| {
                    ensure!(
                        head.move_id == group.id
                            && head.phase == phase
                            && manifest["team"] == group.file,
                        "another team's stream"
                    );
                    for removed in &head.removed {
                        ensure!(
                            removed.starts_with("files/shared/"),
                            "a team removes only shared files"
                        );
                        moving::remove_under(&root, removed)?;
                    }
                    Ok(root.clone())
                },
            )?;
            ensure!(child.wait()?.success(), "the team's stream stopped");
            if phase == Phase::Copy {
                for member in &group.members {
                    let file = member_stream(&root, &member.id)?;
                    let copied = link_agent::transfer::receive(file, None, |manifest| {
                        ensure!(
                            manifest.agent.id() == member.id && manifest.agent.name == member.name,
                            "a member stream names another agent"
                        );
                        Ok(())
                    })?;
                    let link_agent::transfer::Received::Copied { have: copied, .. } = copied else {
                        anyhow::bail!("the member's copy was another phase")
                    };
                    for (path, hash) in copied {
                        have.insert(format!("files/members/{}/{path}", member.id), hash);
                    }
                }
                let shared = root.join("files/shared");
                if shared.exists() {
                    have.extend(moving::have_of(&shared, "files/shared")?);
                }
                continue;
            }
            // The team here first, its shared folder in place: each member
            // arriving derives its grant of the folder from the team's file.
            let mut table: toml::Table = toml::from_str(&group.file)?;
            let team = link_agent_spec::parse_team(&group.file)?;
            // Its ID taken in its ledger here, refused once retired.
            settings::ids::TEAMS.arrive(&team.name, &team.id)?;
            let shared = settings::team_dir(&team.name).join("shared");
            if team.shared.is_some() {
                link_files::create_private_dir(&settings::team_dir(&team.name))?;
                if shared.exists() {
                    let backup =
                        settings::team_dir(&team.name).join(format!("before-{}", group.id));
                    std::fs::rename(&shared, backup)?;
                }
                let source = root.join("files/shared");
                if source.exists() {
                    std::fs::rename(source, &shared)?;
                } else {
                    std::fs::create_dir_all(&shared)?;
                }
                table.insert(
                    "files".into(),
                    toml::Value::Table(toml::Table::from_iter([(
                        "shared".into(),
                        toml::Value::String(shared.display().to_string()),
                    )])),
                );
            }
            super::write_team(&team.name, &table)?;
            // Each member's stream is validated by the ordinary receiver;
            // none starts while another is still being prepared.
            for member in &group.members {
                let file = member_stream(&root, &member.id)?;
                let (agent, _) = crate::moving::receiving::receive_team(
                    out, file, &recipient, args.yes, member,
                )?
                .within("no switched member")?;
                ensure!(
                    agent.agent_id() == Some(&member.id) && agent.name == member.name,
                    "a member stream names another agent"
                );
                arrived.push(agent);
            }
            let finish = Finish {
                account: account.name.clone(),
                group: group.clone(),
                incoming: false,
            };
            finish.save()?;
            committed = true;
            finish.complete(out)?;
        }
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        Ok(format!("Moved team {} here.", listed.name))
    })();
    if result.is_err() && !committed {
        for agent in &arrived {
            let _ = crate::moving::sending::pause(agent);
            let _ = crate::moving::sending::left(
                out,
                agent,
                moved::Place {
                    name: account.name.clone(),
                    cloud: true,
                    ..Default::default()
                },
            );
        }
        let _ = std::fs::remove_file(settings::team_path(&listed.name));
        hosted::call(&account, "agent-abort", |r| {
            r.r#move = Some(group.id.clone())
        })?;
    }
    result
}

fn member_stream(root: &Path, id: &str) -> anyhow::Result<std::fs::File> {
    let folder = root.join("files/members").join(id);
    let mut chunks = std::fs::read_dir(&folder)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    chunks.sort();
    let mut file = moving::temporary_file()?;
    for (n, path) in chunks.into_iter().enumerate() {
        ensure!(
            path.file_name().and_then(|p| p.to_str()) == Some(format!("{n:08}.part").as_str()),
            "member chunks are ordered"
        );
        std::io::copy(
            &mut moving::open_under(root, path.strip_prefix(root)?)?,
            &mut file,
        )?;
    }
    file.rewind()?;
    Ok(file)
}

/// A confirmed destination is kept until the source's cleanup is
/// acknowledged. Repeating the command finishes it without a second copy.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Finish {
    account: String,
    group: Movement,
    incoming: bool,
}

link_files::specification!(Finish, include_str!("finish.schema.toml"));
impl Finish {
    fn root(&self) -> std::path::PathBuf {
        link_files::layout::Hidden::TeamMoving
            .path()
            .join(&self.group.id)
    }
    fn save(&self) -> anyhow::Result<()> {
        check(&self.group)?;
        link_files::create_private_dir(&self.root())?;
        link_files::write_private(
            &self.root().join("finish.json"),
            &link_files::saved::print_json(self)?,
        )?;
        Ok(())
    }
    fn complete(&self, out: &mut Out) -> anyhow::Result<()> {
        check(&self.group)?;
        let team = link_agent_spec::parse_team(&self.group.file)?;
        if self.incoming {
            for member in &self.group.members {
                if Settings::path_of(&member.name).exists() {
                    let agent = Settings::load(&member.name)?;
                    ensure!(
                        agent.agent_id() == Some(&member.id),
                        "another agent has this name here"
                    );
                    crate::moving::sending::left(
                        out,
                        &agent,
                        moved::Place {
                            name: self.account.clone(),
                            cloud: true,
                            ..Default::default()
                        },
                    )?;
                }
            }
            let path = settings::team_path(&team.name);
            if path.exists() {
                ensure!(
                    settings::team(&team.name)?.id == team.id,
                    "another team has this name here"
                );
                std::fs::remove_file(path)?;
            }
        } else {
            let account =
                cloud::account(&self.account).within("the move's cloud account is missing")?;
            ensure!(
                settings::team(&team.name)?.id == team.id,
                "another team has this name here"
            );
            for member in &self.group.members {
                let agent = Settings::load(&member.name)?;
                ensure!(
                    agent.agent_id() == Some(&member.id),
                    "another agent has this name here"
                );
                crate::moving::receiving::settle_and_start(
                    out,
                    &member.name,
                    &member.id,
                    &account.name,
                    &None,
                )?;
            }
            let reply = hosted::call(&account, "agent-left", |r| {
                r.r#move = Some(self.group.id.clone())
            })?;
            for member in &self.group.members {
                if let Some(messages) = reply.team_held.get(&member.id) {
                    crate::moving::sending::deliver_at(out, &None, &member.name, messages);
                }
                moved::models::release(&member.id)?;
            }
        }
        std::fs::remove_dir_all(self.root())?;
        Ok(())
    }
}
/// Whether agent `name` is a member of a team move that switched and is
/// finishing on this machine: leaving it, or landing on it and not yet
/// answering. Until the move is done nothing here answers for it, so a
/// message waits for the move rather than reaching a half-gone agent or
/// a harness still starting.
pub(crate) fn finishing(name: &str) -> anyhow::Result<bool> {
    finishing_in(&link_files::layout::Hidden::TeamMoving.path(), name)
}

/// [`finishing`], of the team moves in `root`.
fn finishing_in(root: &Path, name: &str) -> anyhow::Result<bool> {
    let folders = match std::fs::read_dir(root) {
        Ok(folders) => folders,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    for folder in folders {
        // The move's own cleanup removes its folder as it ends.
        let finish = match std::fs::read(folder?.path().join("finish.json")) {
            Ok(finish) => finish,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let finish: Finish = link_files::saved::parse_json(&finish)?;
        if finish.group.members.iter().any(|m| m.name == name) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn resume_finish(out: &mut Out, args: &cli::TeamMoveArgs) -> anyhow::Result<Option<String>> {
    let root = link_files::layout::Hidden::TeamMoving.path();
    let folders = match std::fs::read_dir(root) {
        Ok(folders) => folders,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let (name, account) = args
        .team
        .rsplit_once('@')
        .map_or((args.team.as_str(), None), |(n, a)| (n, Some(a)));
    for folder in folders {
        let folder = folder?;
        if !folder.file_type()?.is_dir() {
            continue;
        }
        let path = folder.path().join("finish.json");
        if !path.exists() {
            continue;
        }
        let finish: Finish = link_files::saved::parse_json(&std::fs::read(path)?)?;
        check(&finish.group)?;
        let team = link_agent_spec::parse_team(&finish.group.file)?;
        if (name == team.name || name == team.id)
            && account.is_none_or(|a| a == finish.account)
            && ((finish.incoming && to(args)? == finish.account)
                || (!finish.incoming && to(args)? == "here"))
        {
            finish.complete(out)?;
            return Ok(Some(format!(
                "Finished moving team {} {}.",
                team.name,
                if finish.incoming {
                    "to its cloud"
                } else {
                    "here"
                }
            )));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A member of a switched team move is finishing until the move's
    /// folder goes; a move not yet switched, with no finish of its own,
    /// and an agent of no move aren't.
    #[test]
    fn a_switched_moves_members_are_finishing_until_its_folder_goes() {
        let fixture = link_test_kit::TempDir::new("team-finishing");
        let root = fixture.canonicalize().unwrap();
        assert!(!finishing_in(&root.join("absent"), "scout").unwrap());
        std::fs::create_dir_all(root.join("copying")).unwrap();
        assert!(!finishing_in(&root, "scout").unwrap());
        let finish: Finish = serde_json::from_value(serde_json::json!({
            "account": "link",
            "incoming": false,
            "group": {
                "id": "switched",
                "file": "",
                "folder": "",
                "members": [{"id": "a1b2c3d4e5f6", "name": "scout", "movement": "scout-move"}],
            },
        }))
        .unwrap();
        let switched = root.join("switched");
        std::fs::create_dir_all(&switched).unwrap();
        std::fs::write(
            switched.join("finish.json"),
            link_files::saved::print_json(&finish).unwrap(),
        )
        .unwrap();
        assert!(finishing_in(&root, "scout").unwrap());
        assert!(!finishing_in(&root, "writer").unwrap());
        std::fs::remove_dir_all(&switched).unwrap();
        assert!(!finishing_in(&root, "scout").unwrap());
    }
}
