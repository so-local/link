use clap::Parser;

use super::*;

/// A command line taking the slots as every generated parser does: the
/// cloud positional and the four flags, read into [`Slots`].
#[derive(Parser)]
struct Line {
    place: Option<String>,
    #[arg(long)]
    on: Option<String>,
    #[arg(long)]
    cloud: Option<String>,
    #[arg(long)]
    zone: Option<String>,
    #[arg(long)]
    here: bool,
}

fn parse(args: &[&str]) -> Result<Option<Target>, Refusal> {
    let Line {
        place,
        on,
        cloud,
        zone,
        here,
    } = Line::try_parse_from(std::iter::once("lnk").chain(args.iter().copied())).unwrap();
    Slots {
        place,
        on,
        cloud,
        zone,
        here,
    }
    .target()
}

fn refused(args: &[&str]) -> String {
    parse(args).unwrap_err().to_string()
}

#[test]
fn the_cloud_positional_parses_to_its_strict_target() {
    assert_eq!(parse(&["aws"]), parse(&["--cloud", "aws"]));
    assert_eq!(parse(&["lab"]), parse(&["--cloud", "lab"]));
    assert_eq!(parse(&["here"]), parse(&["--here"]));
    assert_eq!(
        parse(&["gcp", "--zone", "eu"]),
        parse(&["--cloud", "gcp", "--zone", "eu"])
    );
    assert_eq!(
        refused(&["here", "--zone", "eu"]),
        refused(&["--here", "--zone", "eu"])
    );
    assert!(refused(&["aws", "--cloud", "gcp"]).starts_with("<place> and --cloud"));
    assert!(refused(&["aws", "--here"]).starts_with("<place> and --here"));
    assert!(refused(&["here", "--on", "mac"]).starts_with("<place> and --on"));
    assert!(refused(&["9aws"]).starts_with("<place> \"9aws\":"));
}

#[test]
fn the_slots_are_the_shared_type_workplace() {
    let every = Slots {
        place: Some("aws".into()),
        on: Some("mac".into()),
        cloud: Some("aws".into()),
        zone: Some("eu".into()),
        here: true,
    };
    let value = serde_json::to_value(&every).unwrap();
    let mut fields: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    let declared = &link_component::shared::types()["workplace"];
    let mut names: Vec<&str> = declared
        .flags
        .keys()
        .chain(declared.positionals.keys())
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    assert_eq!(fields, names);
}

#[test]
fn each_slot_parses_to_its_target() {
    assert_eq!(parse(&[]), Ok(None));
    assert_eq!(parse(&["--here"]), Ok(Some(Target::Here)));
    assert_eq!(parse(&["--on", "mac"]), Ok(Some(Target::On("mac".into()))));
    assert_eq!(
        parse(&["--cloud", "aws"]),
        Ok(Some(Target::Cloud {
            cloud: "aws".into(),
            zone: None
        }))
    );
    assert_eq!(
        parse(&["--cloud", "gcp", "--zone", "europe-west4-b"]),
        Ok(Some(Target::Cloud {
            cloud: "gcp".into(),
            zone: Some("europe-west4-b".into())
        }))
    );
}

#[test]
fn a_command_without_slots_goes_here() {
    let none = Slots::default();
    assert_eq!(none.target_or_here(), Ok(Target::Here));
}

#[test]
fn two_slots_are_refused() {
    assert!(refused(&["--on", "mac", "--cloud", "aws"]).starts_with("--on and --cloud"));
    assert!(refused(&["--here", "--cloud", "aws"]).starts_with("--cloud and --here"));
}

#[test]
fn a_zone_needs_a_cloud() {
    for args in [
        &["--zone", "us-east-1"][..],
        &["--on", "mac", "--zone", "us-east-1"],
        &["--here", "--zone", "us-east-1"],
    ] {
        assert!(
            refused(args).starts_with("--zone goes with a cloud"),
            "{args:?}"
        );
    }
    // A cloud account that is one machine has no zones, by its positional
    // or its strict form.
    let single = ["lab".to_string()];
    let known = Known {
        clouds: &single,
        single: &single,
        ..Known::default()
    };
    for line in [
        &["lab", "--zone", "eu"][..],
        &["--cloud", "lab", "--zone", "eu"],
    ] {
        assert_eq!(
            resolved(line, &known).unwrap_err().to_string(),
            "lab is one machine, with no zones: --on lab",
            "{line:?}"
        );
    }
    assert_eq!(
        resolved(&["--cloud", "lab"], &known),
        Ok(Some(Target::Cloud {
            cloud: "lab".into(),
            zone: None
        }))
    );
}

#[test]
fn a_wrong_type_names_the_right_slot() {
    assert_eq!(
        refused(&["--on", "here"]),
        "here is this machine: use --here"
    );
    assert_eq!(
        refused(&["--cloud", "here"]),
        "here is this machine: use --here"
    );
}

#[test]
fn a_name_that_is_a_cloud_or_a_machine_is_told_apart_by_what_exists() {
    let machines = ["mac".to_string(), "shared".to_string()];
    let clouds = ["aws".to_string(), "shared".to_string()];
    let known = Known {
        machines: &machines,
        clouds: &clouds,
        ..Known::default()
    };
    let on = |name: &str| Target::On(name.into());
    let cloud = |name: &str| Target::Cloud {
        cloud: name.into(),
        zone: None,
    };
    assert_eq!(
        on("aws").check(&known).unwrap_err().to_string(),
        "aws is a cloud: --cloud aws"
    );
    assert_eq!(
        cloud("mac").check(&known).unwrap_err().to_string(),
        "mac is a machine: --on mac"
    );
    for fine in [
        on("mac"),
        on("shared"),
        cloud("aws"),
        cloud("shared"),
        Target::Here,
    ] {
        assert_eq!(fine.check(&known), Ok(()), "{fine}");
    }
}

/// `line` read as the slots, resolved against what exists.
fn resolved(line: &[&str], known: &Known<'_>) -> Result<Option<Target>, Refusal> {
    let Line {
        place,
        on,
        cloud,
        zone,
        here,
    } = Line::try_parse_from(std::iter::once("lnk").chain(line.iter().copied())).unwrap();
    Slots {
        place,
        on,
        cloud,
        zone,
        here,
    }
    .resolve(known)
}

#[test]
fn the_place_positional_names_a_machine_a_cloud_or_here() {
    let machines = ["mac".to_string(), "shared".to_string()];
    let clouds = ["aws".to_string(), "shared".to_string()];
    let known = Known {
        machines: &machines,
        clouds: &clouds,
        ..Known::default()
    };
    // A machine's name is short for --on, a cloud's for --cloud, and the
    // strict forms resolve to the same targets.
    assert_eq!(
        resolved(&["mac"], &known),
        Ok(Some(Target::On("mac".into())))
    );
    assert_eq!(
        resolved(&["mac"], &known),
        resolved(&["--on", "mac"], &known)
    );
    assert_eq!(
        resolved(&["aws"], &known),
        resolved(&["--cloud", "aws"], &known)
    );
    assert_eq!(resolved(&["here"], &known), Ok(Some(Target::Here)));
    // A name that is both is refused, giving both strict forms; each
    // strict form is right.
    assert_eq!(
        resolved(&["shared"], &known).unwrap_err().to_string(),
        "shared is both a machine and a cloud account: give --on shared or --cloud shared"
    );
    assert_eq!(
        resolved(&["--on", "shared"], &known),
        Ok(Some(Target::On("shared".into())))
    );
    // A zone goes with a cloud, never a machine.
    assert_eq!(
        resolved(&["mac", "--zone", "eu"], &known),
        Err(Refusal::ZoneAlone)
    );
    // A name that is neither stays a cloud, refused or not by its owner.
    assert_eq!(
        resolved(&["gcp"], &known),
        Ok(Some(Target::Cloud {
            cloud: "gcp".into(),
            zone: None
        }))
    );
    // The strict slots are checked as ever.
    assert_eq!(
        resolved(&["--on", "aws"], &known).unwrap_err().to_string(),
        "aws is a cloud: --cloud aws"
    );
}

#[test]
fn what_cannot_be_a_name_is_refused_in_its_slot() {
    assert!(refused(&["--on", "Mac"]).starts_with("--on \"Mac\":"));
    assert!(refused(&["--cloud", "9aws"]).starts_with("--cloud \"9aws\":"));
    assert!(refused(&["--cloud", "aws", "--zone", "US East"]).starts_with("--zone \"US East\":"));
}

#[test]
fn a_zone_is_what_its_refusal_says() {
    for fine in ["us-east-1", "europe-west4-b", "us_east_1"] {
        assert!(parse(&["--cloud", "aws", "--zone", fine]).is_ok(), "{fine}");
    }
    let said = refused(&["--cloud", "aws", "--zone", "US.East"]);
    assert!(
        said.contains("lowercase letters, digits, dashes and underscores"),
        "{said}"
    );
}

#[test]
fn a_target_displays_as_the_strict_form() {
    let text = |args: &[&str]| parse(args).unwrap().unwrap().to_string();
    assert_eq!(text(&["--on", "mac"]), "--on mac");
    assert_eq!(
        text(&["--cloud", "gcp", "--zone", "us-east1"]),
        "--cloud gcp --zone us-east1"
    );
    assert_eq!(text(&["--here"]), "--here");
}
