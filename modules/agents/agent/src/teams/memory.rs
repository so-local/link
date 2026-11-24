//! `lnk agent team-memory start` and `stop`: the team's memory over its
//! shared folder turned on or off, as its spec's `[settings.memory]
//! enabled` keeps it. Off by default, like an agent's own memory.
use super::*;

/// `lnk agent team-memory start|stop <team>`: its memory turned `on` or
/// off, its file changed, and the members it's handed to restarted, each
/// once, so each takes it at a start of its own.
pub(super) async fn run(out: &mut Out, team: &str, on: bool) -> Result<Done, Report> {
    let before = team_now(team)?;
    let shown = Name::new("team", team);
    let Some(shared) = before.shared.clone() else {
        return Err(Report::error(sentence!(
            "Team {} has no shared folder for a memory to keep.",
            &shown
        ))
        .exit(Exit::Usage));
    };
    if before.settings.memory.enabled == on {
        out.block(Note(match on {
            true => sentence!("Team {}'s memory is on already.", &shown),
            false => sentence!("Team {}'s memory is off already.", &shown),
        }));
        return Ok(Done::shown());
    }
    let was = std::fs::read_to_string(settings::team_path(team)).within("reading the team")?;
    let mut table = read_team(team)?;
    set_enabled(&mut table, on)?;
    write_team(team, &table)?;
    let now = team_now(team)?;
    match on {
        // An engine that won't serve: the file put back, as it was.
        true => {
            if let Err(err) = crate::memory::define_team(out, &now) {
                link_files::write_private(&settings::team_path(team), was.as_bytes())
                    .within("putting the team back")?;
                return Err(err);
            }
        }
        false => crate::memory::forget_team(&now),
    }
    // Only a member granted all of the folder is handed its memory.
    let ids = by_id();
    let handed: Vec<String> = names_of(&now, &ids)
        .into_iter()
        .filter(|agent| {
            sandbox_file::of(agent)
                .spec
                .teams
                .get(team)
                .is_some_and(TeamFolders::whole)
        })
        .collect();
    restart(out, &handed).await?;
    let mut done = Done::success(match on {
        true => sentence!(
            "Team {}'s memory is on, over {}.",
            &shown,
            Where::new(&shared)
        ),
        false => sentence!("Team {}'s memory is off.", &shown),
    })
    .changed(
        "Kept in the team's file",
        Where::new(settings::team_path(team)),
    );
    if on && handed.is_empty() {
        done = done.next(
            cmd!("lnk agent team add {} --member <member> --read .", &shown),
            "hand it to a member, which takes all of the folder",
        );
    }
    Ok(done)
}

/// `table`, a team's file, with `[settings.memory] enabled` set `on`, and
/// left out when off, its default.
fn set_enabled(table: &mut toml::Table, on: bool) -> anyhow::Result<()> {
    let settings = table
        .entry("settings")
        .or_insert_with(|| toml::Table::new().into())
        .as_table_mut()
        .within("its [settings] isn't a table")?;
    let memory = settings
        .entry("memory")
        .or_insert_with(|| toml::Table::new().into())
        .as_table_mut()
        .within("its [settings.memory] isn't a table")?;
    match on {
        true => memory.insert("enabled".into(), true.into()),
        false => memory.remove("enabled"),
    };
    if memory.is_empty() {
        settings.remove("memory");
    }
    if settings.is_empty() {
        table.remove("settings");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::set_enabled;

    /// On, the setting is written; off, it's left out with any table it
    /// leaves empty, and the rest of `[settings.memory]` stays.
    #[test]
    fn the_setting_is_written_on_and_left_out_off() {
        let mut table: toml::Table = toml::from_str("name = \"research\"\n").unwrap();
        set_enabled(&mut table, true).unwrap();
        assert_eq!(table["settings"]["memory"]["enabled"].as_bool(), Some(true));
        set_enabled(&mut table, false).unwrap();
        assert!(!table.contains_key("settings"), "{table}");

        let mut table: toml::Table =
            toml::from_str("[settings.memory]\nsemantic = false\nenabled = true\n").unwrap();
        set_enabled(&mut table, false).unwrap();
        assert_eq!(
            table["settings"]["memory"].as_table().unwrap().len(),
            1,
            "{table}"
        );
    }
}
