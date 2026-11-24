//! A spec read: from a file, checked against what it says, and with its
//! field flags laid over it.

use super::*;

/// The line in `text` giving `key` in table `[table]`, counting from 1,
/// when it's written as a table of its own.
fn line_of(text: &str, table: &str, key: &str) -> Option<usize> {
    let mut inside = table.is_empty();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            inside = header.split(']').next().is_some_and(|h| h.trim() == table);
            continue;
        }
        if inside && line.split('=').next().is_some_and(|k| k.trim() == key) {
            return Some(i + 1);
        }
    }
    None
}

/// At most the largest spec of `path`'s text, or none when it can't be read:
/// only a refusal's line numbers come from it.
fn file_text(path: &std::path::Path) -> String {
    use std::io::Read;
    let mut text = String::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(link_spec::MAX_BYTES).read_to_string(&mut text);
    }
    text
}

/// A key of `[machine]`, or of a cloud's table in it, that is none of its
/// fields and no cloud's table, in `text`, said with its line. Serde reads
/// every other key of `[machine]` as a cloud's table, so such a key would
/// be refused as a value that isn't a table, never by its name.
fn unknown_machine_key(text: &str) -> Option<anyhow::Error> {
    let value: toml::Table = text.parse().ok()?;
    let machine = value.get("machine")?.as_table()?;
    let at = |table: &str, key: &str| match line_of(text, table, key) {
        Some(line) => format!("line {line}: "),
        None => String::new(),
    };
    let known = |key: &str| Fields::NAMES.contains(&key);
    let fields = Fields::NAMES.join(", ");
    for (key, given) in machine.iter().filter(|(key, _)| !known(key)) {
        match given.as_table() {
            Some(cloud) => {
                let name = format!("machine.{key}");
                if let Some((inner, _)) = cloud.iter().find(|(k, v)| !known(k) && !v.is_table()) {
                    return Some(anyhow::anyhow!(
                        "{}[{name}] {inner} isn't one of its fields: {fields}",
                        at(&name, inner)
                    ));
                }
            }
            None => {
                return Some(anyhow::anyhow!(
                    "{}[machine] {key} isn't one of its fields: {fields}; a cloud's own \
                     fields go in [machine.<cloud>]",
                    at("machine", key)
                ));
            }
        }
    }
    None
}

/// `refused` with its line in `text`, a file's, where the file alone is
/// refused the same way: the kind's own check, run in every read, has no
/// text to count lines in. A key of `[machine]` that's none of its fields
/// is refused by its name.
fn with_line(refused: anyhow::Error, text: &str) -> anyhow::Error {
    use link_files::schema::Specification;
    if let Some(unknown) = unknown_machine_key(text) {
        return match refused.chain().count() > 1 {
            true => unknown.context(refused.to_string()),
            false => unknown,
        };
    }
    match BoxSpec::format().read::<BoxSpec>(text) {
        Ok(read) => match read.spec.check(text, false) {
            // The file it was read from, where the refusal named one.
            Err(lined) if refused.chain().count() > 1 => lined.context(refused.to_string()),
            Err(lined) => lined,
            Ok(()) => refused,
        },
        Err(_) => refused,
    }
}

impl BoxSpec {
    /// The checks serde can't make, on a spec as written (`text`, for its
    /// lines): a cloud's own field in `[machine]`, `size` and what it has
    /// built in in one table, too many tags, and from a file (`from_file`)
    /// an `lnk` behind this computer's.
    pub fn check(&self, text: &str, from_file: bool) -> anyhow::Result<()> {
        let at = |table: &str, key: &str| match line_of(text, table, key) {
            Some(line) => format!("line {line}: "),
            None => String::new(),
        };
        let machine = self.machine();
        for (key, given) in [
            ("region", machine.region.is_some()),
            ("size", machine.size.is_some()),
        ] {
            ensure!(
                !given,
                "{}[machine] {key} names one cloud's: put it in that cloud's own table, \
                 [machine.<cloud>], <cloud> as lnk cloud list says its kind",
                at("machine", key)
            );
        }
        ensure!(
            machine.tags.len() <= MAX_TAGS,
            "{} tags: a box takes at most {MAX_TAGS} of yours",
            machine.tags.len()
        );
        for (kind, table) in &machine.others {
            let name = format!("machine.{kind}");
            if let Some(inner) = table.others.keys().next() {
                bail!(
                    "[{name}] can't have a cloud's table in it, and {inner} isn't one of its \
                     fields: {}",
                    Fields::NAMES.join(", ")
                );
            }
            let typed = Fields::of(table).unwrap_or_default();
            if let (Some(_), Some(field)) = (&table.size, typed.sizes_it()) {
                bail!(
                    "{}[{name}] gives size and {field}: a machine type has its {field} built in, \
                     so one of them would be ignored; give one",
                    at(&name, field)
                );
            }
            ensure!(
                table.tags.len() <= MAX_TAGS,
                "{} tags in [{name}]: a box takes at most {MAX_TAGS} of yours",
                table.tags.len()
            );
        }
        if from_file && let Some(version) = &self.lnk_version() {
            let this = link_version::version();
            ensure!(
                *version >= this,
                "{}[lnk] version {version} is behind this computer's lnk {this}: a spec \
                 can't install a lower lnk, which may have a known hole. To mean it, give \
                 --lnk.version {version} yourself",
                at("lnk", "version")
            );
        }
        Ok(())
    }

    /// Reads a spec from `text`, strictly (`link_spec::parse`),
    /// and [`BoxSpec::check`]s it as a file's.
    pub fn parse(text: &str) -> anyhow::Result<BoxSpec> {
        let spec: BoxSpec = link_spec::parse_spec(text).map_err(|e| with_line(e, text))?;
        spec.check(text, true)?;
        Ok(spec)
    }

    /// The spec `file`, if given, and `flags` make, read once through
    /// the spec's reader: a flag replaces its key, a file's relative paths
    /// are its folder's. `zone`, the slots' place for an account of cloud
    /// kind `kind`, is that cloud's table's region. A named field is
    /// replaced before its value is checked. A flag may explicitly name
    /// an `lnk` behind this computer's.
    pub fn made(
        file: Option<&std::path::Path>,
        flags: &link_spec::FieldFlags,
        zone: Option<(&str, &str)>,
    ) -> anyhow::Result<link_spec::Made<BoxSpec>> {
        // What doesn't make a box spec is the person's to fix.
        Self::making(file, flags, zone).map_err(link_format::refused)
    }

    fn making(
        file: Option<&std::path::Path>,
        flags: &link_spec::FieldFlags,
        zone: Option<(&str, &str)>,
    ) -> anyhow::Result<link_spec::Made<BoxSpec>> {
        let mut made = link_spec::read_with::<BoxSpec>(file, flags)
            .map_err(|e| with_line(e, &file.map(file_text).unwrap_or_default()))?;
        if let Some((kind, zone)) = zone {
            ensure!(
                flags.get(&["machine", kind, "region"]).is_none(),
                "--zone and --machine.{kind}.region both name the region: give one",
                kind = link_format::clean(kind)
            );
            let kind = CloudKind::try_from(kind.to_string()).map_err(anyhow::Error::msg)?;
            let region = CloudName::try_from(zone.to_string())
                .map_err(|why| anyhow::anyhow!("--zone {}: {why}", link_format::clean(zone)))?;
            made.spec
                .machine
                .get_or_insert_default()
                .others
                .entry(kind.to_string())
                .or_default()
                .region = Some(region.to_string());
        }
        // A version the flags give is yours. An unchanged file value
        // still cannot install a lower release with a known hole.
        let from_file = file.is_some() && flags.get(&["lnk", "version"]).is_none();
        // The file's text, for the lines a refusal names; the flags have
        // none of their own.
        let text = made.file.as_deref().map(file_text).unwrap_or_default();
        made.spec.check(&text, from_file)?;
        Ok(made)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text`, written as a person writes a spec, naming its schema first.
    fn parse(text: &str) -> anyhow::Result<BoxSpec> {
        BoxSpec::parse(&format!("schema = 1\n{text}"))
    }

    /// An empty file is refused, naming the schema line it lacks; one
    /// holding that line alone is today's box.
    #[test]
    fn an_empty_file_is_refused_and_one_naming_its_schema_is_todays_box() {
        let err = format!("{:#}", BoxSpec::parse("").unwrap_err());
        assert!(err.contains("add schema = 1"), "{err}");
        let spec = parse("").unwrap();
        assert_eq!(spec, BoxSpec::default());
        let want = spec.want("aws");
        assert!(!want.matching);
        assert_eq!(want.disk, Size::gb(20));
        assert_eq!(want.disk_type, DiskType::Balanced);
        assert_eq!(want.os, Os::Ubuntu2404);
        assert_eq!(want.arch, Arch::X8664);
        assert!(!want.spot);
        assert_eq!(want.gpus, Range::exactly(0));
    }

    #[test]
    fn values_are_typed() {
        let spec = parse(
            "name = \"web\"\nversion = 3\n[lnk]\nversion = \"9.0.0\"\n[machine]\n\
             cpus = { min = 2, max = 8 }\nmemory = { min = \"4 GB\", max = \"16 GB\" }\n\
             gpus = { min = 1, model = \"nvidia-l4\" }\ndisk = { min = \"40 GB\", type = \"fast\" }\n\
             tags = { team = \"research\" }\n",
        )
        .unwrap();
        let want = spec.want("aws");
        assert_eq!(want.cpus.min, Some(2));
        assert_eq!(want.memory.max, Some(Size::gb(16)));
        assert_eq!(want.gpu_model, Some(GpuModel::NvidiaL4));
        assert_eq!(want.disk_type, DiskType::Fast);
        assert!(want.matching);
        for (bad, says) in [
            ("[machine]\nmemory = 8\n", "untagged enum SizeRange"),
            ("[machine]\nmemory = \"8 gigs\"\n", "isn't a size"),
            ("[machine]\nos = \"ubuntu-20.04\"\n", "unknown variant"),
            ("[machine]\narch = \"riscv\"\n", "unknown variant"),
            ("[machine]\ncpus = { min = 8, max = 2 }\n", "over its max"),
            (
                "[machine]\ncpus = { least = 2 }\n",
                "untagged enum CountRange",
            ),
            ("[lnk]\nversion = \"latest\"\n", "is not a version"),
            ("[lnk]\nversion = \"0.10.3\"\n", "no modules"),
            ("[lnk]\nversion = \"1.2.3; rm -rf /\"\n", "is not a version"),
            ("name = \"Web Server\"\n", "can't start a box's name"),
            (
                "[machine.aws]\nregion = \"--profile=x\"\n",
                "isn't a cloud's region",
            ),
            ("[machine]\ntags = { lnk-box = \"x\" }\n", "Link's own"),
            ("[machine]\ntags = { Name = \"x\" }\n", "Link's own"),
            ("[machine]\ntags = { \"aws:x\" = \"x\" }\n", "AWS's own"),
            (
                "[machine]\nregion = \"eu-west-2\"\n",
                "line 3: [machine] region",
            ),
            (
                "[machine.aws]\nsize = \"t3.small\"\ncpus = 4\n",
                "line 4: [machine.aws] gives size and cpus",
            ),
            ("[machine.gcp]\n", "line 2: [machine.gcp] is empty"),
            (
                "[machine]\nspots = true\n",
                "line 3: [machine] spots isn't one of its fields",
            ),
        ] {
            let err = format!("{:#}", parse(bad).unwrap_err());
            assert!(err.contains(says), "{bad}: {err}");
        }
        let many: Vec<String> = (0..41).map(|i| format!("t{i} = \"x\"")).collect();
        let err = parse(&format!("[machine]\ntags = {{ {} }}\n", many.join(", "))).unwrap_err();
        assert!(err.to_string().contains("at most 40"), "{err}");
    }

    /// The field flags `words` give a command binding the box spec.
    fn flags(words: &[&str]) -> link_spec::FieldFlags {
        let args = std::iter::once("lnk")
            .chain(words.iter().copied())
            .map(Into::into);
        let binding = link_spec::Binding::of::<BoxSpec>();
        let (flags, rest) = link_spec::split(args, Some(&binding), None).unwrap();
        assert_eq!(rest.len(), 1, "{rest:?}");
        flags
    }

    /// The spec a file of `text`, if any, and `words` make, with `zone`.
    fn made(
        text: Option<&str>,
        words: &[&str],
        zone: Option<(&str, &str)>,
    ) -> anyhow::Result<BoxSpec> {
        let dir = link_test_kit::temp::TempDir::new("box-spec");
        let file = text.map(|text| {
            let path = dir.path().join("box.toml");
            std::fs::write(&path, format!("schema = 1\n{text}")).unwrap();
            path
        });
        BoxSpec::made(file.as_deref(), &flags(words), zone).map(|m| m.spec)
    }

    #[test]
    fn a_field_override_is_read_before_the_replaced_files_value_is_validated() {
        for text in ["version = 'bad'\n", "version = -1\n"] {
            let spec = made(Some(text), &["--spec.version", "2"], None).unwrap();
            assert_eq!(spec.version, Some(2));
        }
        let spec = made(
            Some("[machine.aws]\nsize = 't3.small'\ncpus = 4\n"),
            &["--machine.aws", "{cpus=2}"],
            None,
        )
        .unwrap();
        assert_eq!(spec.want("aws").cpus.min, Some(2));
    }

    #[test]
    fn an_older_lnk_only_from_a_flag() {
        let older = Version::new(0, 11, 0);
        assert!(older < link_version::version() || link_version::version() == FLOOR);
        if older < link_version::version() {
            let err = parse("[lnk]\nversion = \"0.11.0\"\n").unwrap_err();
            assert!(err.to_string().contains("--lnk.version 0.11.0"), "{err}");
        }
        let spec = made(None, &["--lnk.version", "0.11.0"], None).unwrap();
        assert_eq!(spec.lnk_version(), Some(older.clone()));
        assert!(made(None, &["--lnk.version", "0.9.0"], None).is_err());
        // A spec printed with that flag reads back with it: the flag's
        // version is yours, and the file's it replaces isn't held to
        // this computer's lnk.
        if older < link_version::version() {
            let printed = toml::to_string(&spec).unwrap();
            assert!(made(Some(&printed), &[], None).is_err());
            let again = made(Some(&printed), &["--lnk.version", "0.11.0"], None).unwrap();
            assert_eq!(again, spec);
        }
    }

    #[test]
    fn a_cloud_table_replaces_each_field_whole_and_a_flag_sets_its_key() {
        let text = "[machine]\nmemory = { min = \"4 GB\", max = \"16 GB\" }\ncpus = 2\n\
             [machine.gcp]\nmemory = { min = \"8 GB\" }\nregion = \"europe-west2-b\"\n\
             [machine.aws]\nsize = \"g6.xlarge\"\n";
        let spec = parse(text).unwrap();
        let gcp = spec.want("gcp");
        assert_eq!(gcp.memory.max, None, "replaced whole, not merged");
        assert_eq!(gcp.cpus, Range::exactly(2));
        assert_eq!(gcp.region.as_ref().unwrap().as_str(), "europe-west2-b");
        let aws = spec.want("aws");
        assert_eq!(aws.size.as_ref().unwrap().as_str(), "g6.xlarge");
        assert!(!aws.matching);
        // A flag sets its key alone, as the file's line would: a cloud's
        // table still has its say there.
        let laid = made(Some(text), &["--machine.cpus", "4"], None).unwrap();
        assert_eq!(laid.want("aws").size.unwrap().as_str(), "g6.xlarge");
        assert_eq!(laid.want("gcp").cpus, Range::exactly(4));
        // `--cloud gcp --zone x-1`: the slots' place is that cloud's region.
        let laid = made(
            Some(text),
            &["--machine.aws.cpus", "4"],
            Some(("gcp", "x-1")),
        );
        let err = format!("{:#}", laid.unwrap_err());
        assert!(err.contains("gives size and cpus"), "{err}");
        let laid = made(Some(text), &[], Some(("gcp", "x-1"))).unwrap();
        assert_eq!(laid.want("gcp").region.unwrap().as_str(), "x-1");
        assert_eq!(laid.want("aws").size.unwrap().as_str(), "g6.xlarge");
        // A cloud's place or size in [machine] is refused, flag or file.
        let err = format!(
            "{:#}",
            made(None, &["--machine.size", "x"], None).unwrap_err()
        );
        assert!(err.contains("names one cloud's"), "{err}");
    }

    /// `--zone` is the cloud's region, so it and that region's field flag
    /// are refused together rather than one silently dropping the other.
    #[test]
    fn a_zone_and_its_region_flag_are_refused_together() {
        let err = format!(
            "{:#}",
            made(
                None,
                &["--machine.aws.region", "us-east-1"],
                Some(("aws", "us-west-2"))
            )
            .unwrap_err()
        );
        assert!(err.contains("--zone and --machine.aws.region"), "{err}");
        // Another cloud's region is no collision.
        let spec = made(
            None,
            &["--machine.gcp.region", "x-1"],
            Some(("aws", "us-west-2")),
        )
        .unwrap();
        assert_eq!(spec.want("aws").region.unwrap().as_str(), "us-west-2");
        assert_eq!(spec.want("gcp").region.unwrap().as_str(), "x-1");
    }

    /// A file's refusal names its line when flags are given beside it too.
    #[test]
    fn a_files_refusal_keeps_its_line_beside_flags() {
        let text = "[machine]\nregion = 'us-east-1'\n";
        let err = format!(
            "{:#}",
            made(Some(text), &["--machine.cpus", "4"], None).unwrap_err()
        );
        assert!(err.contains("line 3: [machine] region"), "{err}");
        assert!(err.contains("box.toml"), "{err}");
    }

    /// A mistyped flag's nearest is one the spec takes, never itself nor
    /// a cloud's table named for a field (`machine.disk.size`).
    #[test]
    fn a_mistyped_flag_s_nearest_is_one_it_takes() {
        let binding = link_spec::Binding::of::<BoxSpec>();
        let refused = |flag: &str| {
            let args = ["lnk", flag, "1"].map(std::ffi::OsString::from);
            format!(
                "{:#}",
                link_spec::split(args, Some(&binding), None).unwrap_err()
            )
        };
        let said = refused("--machine.disk.size");
        assert!(said.contains("the nearest is --machine.disk.min"), "{said}");
        let said = refused("--machine.gpus.count");
        assert!(!said.contains("nearest is --machine.gpus.count"), "{said}");
        assert!(!said.contains("--machine.gpus.cpus"), "{said}");
    }

    /// A range is a count or a size, or a `{ min, max }` table, a flag's
    /// and a file's alike; its old text form, `2..4`, is refused.
    #[test]
    fn a_range_is_a_value_or_its_ends() {
        let from_flags = made(
            None,
            &[
                "--machine.cpus",
                "{min=2,max=4}",
                "--machine.memory",
                "{min='4 GB'}",
                "--machine.gpus",
                "{min=1,max=2}",
            ],
            None,
        )
        .unwrap();
        let from_file = parse(
            "[machine]\ncpus = { min = 2, max = 4 }\nmemory = { min = \"4 GB\" }\n\
             gpus = { min = 1, max = 2 }\n",
        )
        .unwrap();
        assert_eq!(from_flags, from_file);
        let want = from_flags.want("aws");
        assert_eq!(want.cpus.min, Some(2));
        assert_eq!(want.cpus.max, Some(4));
        assert_eq!(want.memory.min, Some(Size::gb(4)));
        assert_eq!(want.gpus.max, Some(2));
        let exact = made(
            None,
            &["--machine.cpus", "4", "--machine.memory", "8 GB"],
            None,
        )
        .unwrap()
        .want("aws");
        assert_eq!(exact.cpus, Range::exactly(4));
        assert_eq!(exact.memory, Range::exactly(Size::gb(8)));
        for text in [
            "[machine]\ncpus = \"2..4\"\n",
            "[machine]\nmemory = \"4GB..\"\n",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }

    /// A cloud's table is named by its kind, any cloud's: the box module
    /// knows none. Its fields are as strict as `[machine]`'s.
    #[test]
    fn any_cloud_has_a_table_of_its_own() {
        let spec =
            parse("[machine]\ncpus = 2\n[machine.third]\nsize = \"x1\"\nspot = true\n").unwrap();
        let third = spec.want("third");
        assert_eq!(third.size.unwrap().as_str(), "x1");
        assert!(third.spot && !third.matching);
        assert_eq!(spec.want("aws").cpus, Range::exactly(2));
        // A cloud's size and the slots' zone go in its table.
        let laid = made(None, &["--machine.other.size=x2"], Some(("other", "r-1"))).unwrap();
        let printed = link_spec::print(&laid).unwrap();
        assert_eq!(
            printed,
            "[machine.other]\nregion = \"r-1\"\nsize = \"x2\"\n"
        );
        assert_eq!(parse(&printed).unwrap(), laid);
        for (bad, says) in [
            (
                "[machine.third]\nspots = true\n",
                "line 3: [machine.third] spots isn't one of its fields",
            ),
            (
                "[machine.third.inner]\ncpus = 2\n",
                "can't have a cloud's table",
            ),
            ("[machine.Third]\ncpus = 2\n", "isn't a cloud"),
            (
                "[machine]\nspots = true\n",
                "[machine] spots isn't one of its fields",
            ),
        ] {
            let err = format!("{:#}", parse(bad).unwrap_err());
            assert!(err.contains(says), "{bad}: {err}");
        }
        // Every field's name is one a file writes.
        let mut full = parse(
            "[machine]\ncpus = 2\nmemory = \"4 GB\"\ngpus = 1\narch = \"x86_64\"\n\
             disk = { min = \"40 GB\" }\nos = \"ubuntu-24.04\"\nspot = true\ntags = { a = \"b\" }\n",
        )
        .unwrap();
        let machine = full.machine.get_or_insert_default();
        machine.region = Some("r".into());
        machine.size = Some("s".into());
        let value = toml::Value::try_from(&*machine).unwrap();
        let mut names: Vec<_> = value.as_table().unwrap().keys().cloned().collect();
        let mut known: Vec<_> = Fields::NAMES.iter().map(|n| n.to_string()).collect();
        names.sort();
        known.sort();
        assert_eq!(names, known);
    }

    #[test]
    fn switches_without_a_value() {
        let spec = made(
            None,
            &[
                "--machine.spot",
                "--lnk.login=false",
                "--machine.tags.a",
                "b",
            ],
            None,
        )
        .unwrap();
        assert_eq!(spec.machine().spot, Some(true));
        assert_eq!(spec.lnk().login, Some(false));
        assert_eq!(spec.machine().tags.len(), 1);
        let spec = made(None, &["--machine.spot=false"], None).unwrap();
        assert_eq!(spec.machine().spot, Some(false));
        assert_eq!(made(None, &[], None).unwrap(), BoxSpec::default());
    }

    #[test]
    fn a_spec_prints_back_as_itself() {
        let text = "name = \"web\"\nversion = 3\n[lnk]\nversion = \"9.0.0\"\nlogin = false\n\
                    [machine]\ncpus = { min = 2, max = 8 }\nmemory = \"8 GB\"\ngpus = 1\n\
                    spot = true\ntags = { team = \"research\" }\n\
                    [machine.aws]\nsize = \"t3.small\"\ndisk = { min = \"40 GB\" }\n";
        let spec = parse(text).unwrap();
        let printed = link_spec::print_spec(&spec).unwrap();
        assert_eq!(BoxSpec::parse(&printed).unwrap(), spec, "{printed}");
    }
}
