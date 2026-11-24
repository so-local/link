use super::*;
use crate::agent_spec::{Known, parse, resolve};
use crate::harness::Contract;

fn resolved(own: &str) -> Resolved {
    let known = Known {
        name: "main".into(),
    };
    resolve(
        Some(&parse(&format!("schema = 1\n{own}")).unwrap()),
        &[],
        &known,
    )
    .unwrap()
}

fn set(features: &[Feature]) -> BTreeSet<Feature> {
    features.iter().copied().collect()
}

#[test]
fn a_whole_prompt_source_turns_off_assembled_instruction_features() {
    for source in ["instructions/instruction.md", "none"] {
        let own = resolved(&format!("[instructions]\nsource = {source:?}\n"));
        let on = Feature::of_spec(&own.spec);
        for feature in [Feature::RepoInstructions, Feature::Persona, Feature::Skills] {
            assert!(!feature.on(&own.spec));
            assert!(!on.contains(&feature));
            assert!(!Feature::chosen(&own).contains(&feature));
        }
        assert!(on.contains(&Feature::Commands));
    }
}

#[test]
fn each_feature_has_one_id_read_back() {
    let ids: BTreeSet<&str> = Feature::ALL.iter().map(|f| f.id()).collect();
    assert_eq!(ids.len(), Feature::ALL.len());
    for feature in Feature::ALL {
        let json = serde_json::to_string(&feature).unwrap();
        assert_eq!(json, format!("\"{}\"", feature.id()));
        assert_eq!(serde_json::from_str::<Feature>(&json).unwrap(), feature);
        // Each key it's switched by is one Link's spec sets.
        let link = toml::Value::try_from(crate::agent_spec::link()).unwrap();
        for key in feature.switches().iter().flat_map(|k| k.iter()) {
            assert!(paths(&link, key).next().is_some(), "{key}");
        }
    }
    assert!(serde_json::from_str::<Feature>("\"teleport\"").is_err());
    assert_eq!(Feature::CodeSearch.table(), "[tool.file] enabled");
    assert_eq!(Feature::Images.table(), "[connections] images");
}

/// An agent with no `extends` has every feature but those of the tools
/// Link's spec leaves off until added (file is on, so asked to, it
/// changes its files, instructions and skills); with them added, every
/// one.
#[test]
fn links_spec_turns_every_feature_on() {
    use Feature::*;
    let link = resolved("");
    let tools = [Subagents, Schedules, Proactive, Shell, Lsp];
    let untooled: Vec<Feature> = Feature::ALL
        .into_iter()
        .filter(|f| !tools.contains(f))
        .collect();
    assert_eq!(Feature::of_spec(&link.spec), set(&untooled));
    assert!(Feature::chosen(&link).is_empty());
    let added = resolved(
        "[tool.scheduler]\nenabled = true\n[tool.run]\nenabled = true\n\
         [tool.file]\nenabled = true\n[tool.code]\nenabled = true\n",
    );
    assert_eq!(Feature::of_spec(&added.spec), set(&Feature::ALL));
}

#[test]
fn a_feature_is_off_where_a_key_says_none_never_or_false() {
    let off = resolved(
        "[instructions]\nrepository = \"none\"\npersona = \"none\"\n\
         [tool.file]\nenabled = true\nfind = false\n[connections]\ngroups = \"never\"\n",
    );
    let on = Feature::of_spec(&off.spec);
    use Feature::*;
    for feature in [RepoInstructions, CodeSearch, Groups] {
        assert!(!on.contains(&feature), "{feature:?}");
    }
    // The owner's section still holds who its owner is.
    assert!(on.contains(&Persona) && on.contains(&FileRead) && on.contains(&Diffs));
    // Left out, off: a spec that extends none sets what it has.
    let mut commands = false;
    let none: Vec<&str> = crate::agent_spec::LINK_SPEC
        .lines()
        .filter(|l| {
            if l.starts_with('[') {
                commands = l.starts_with("[commands]");
            }
            // Its schema line is the one `resolved` writes.
            !commands && !l.starts_with("repository =") && !l.starts_with("schema =")
        })
        .collect();
    let none = format!("extends = \"none\"\n{}", none.join("\n"));
    let none = Feature::of_spec(&resolved(&none).spec);
    assert!(!none.contains(&RepoInstructions) && !none.contains(&Commands));
    assert!(none.contains(&Persona));
}

/// Only what the agent's own spec turns on is its choice: what Link's
/// spec turns on isn't.
#[test]
fn the_chosen_features_are_the_agents_own() {
    assert!(Feature::chosen(&resolved("extends = \"link\"\n")).is_empty());
    let groups = resolved("[connection.slack]\ngroups = \"mention\"\n");
    assert_eq!(Feature::chosen(&groups), set(&[Feature::Groups]));
    let own = resolved("[commands]\ncreate = true\n[tool.file]\nenabled = true\nfind = true\n");
    use Feature::*;
    assert_eq!(
        Feature::chosen(&own),
        set(&[Commands, FileRead, FileEdit, CodeSearch, Diffs])
    );
    let off = resolved("[commands]\ncreate = false\n");
    assert!(Feature::chosen(&off).is_empty());
}

/// A stand-in harness honoring only some features: a spec turning on
/// another is refused, naming it; one leaving it to Link's spec isn't.
#[test]
fn a_spec_asking_a_harness_for_what_it_doesnt_honor_is_refused() {
    let stand_in = Contract {
        features: Some(Honors::from([(Feature::Persona, Honor::Native)])),
        ..Contract::new(true)
    };
    let asks = resolved("[commands]\ncreate = true\n[instructions]\npersona = \"me.md\"\n");
    assert_eq!(stand_in.refused(&asks), Some(Feature::Commands));
    assert_eq!(stand_in.refused(&resolved("")), None);
    let honors = Contract {
        features: Some(Honors::from([
            (Feature::Persona, Honor::Native),
            (Feature::Commands, Honor::Spec),
        ])),
        ..Contract::new(true)
    };
    assert_eq!(honors.refused(&asks), None);
}

/// An adapter before the list honors what its flags imply; a list
/// naming what this Link doesn't know keeps the rest.
#[test]
fn an_old_contract_keeps_its_meaning() {
    let old: Contract =
        serde_json::from_str(r#"{"version":1,"proxy":true,"instructions":true}"#).unwrap();
    assert_eq!(old.features, None);
    let honors = old.honors();
    use Feature::*;
    for feature in [RepoInstructions, Persona, Skills, Groups, Streaming] {
        assert_eq!(honors.get(&feature), Some(&Honor::Spec), "{feature:?}");
    }
    for feature in [Commands, ModelFallback, Images, Shell, Subagents] {
        assert!(!honors.contains_key(&feature), "{feature:?}");
    }
    // Today's [instructions] stay its to take, and groups Link's.
    assert_eq!(
        old.refused(&resolved("[instructions]\npersona = \"me.md\"\n")),
        None
    );
    assert_eq!(
        old.refused(&resolved("[commands]\ncreate = true\n")),
        Some(Commands)
    );
    let newer: Contract = serde_json::from_str(
        r#"{"version":1,"proxy":true,"features":{"lsp":"spec","teleport":"spec","persona":"somehow"}}"#,
    )
    .unwrap();
    assert_eq!(newer.honors(), Honors::from([(Lsp, Honor::Spec)]));
    let back: Contract = serde_json::from_str(&serde_json::to_string(&newer).unwrap()).unwrap();
    assert_eq!(back, newer);
}
