//! What a spec wants of a cloud: its machine pinned for a kind, the want
//! it makes, and an image's disk and arch fitted to it.

use super::*;

impl BoxSpec {
    /// This spec, the one a box in cloud `kind` was made from, with what
    /// the cloud says of the box (`found`) pinned in that cloud's table:
    /// its machine type, dropping what that has built in, and its region,
    /// then its disk, OS and spot where they differ from what the spec
    /// says there. So the file makes exactly that box there, and the best
    /// fit elsewhere. What the cloud says is checked as a file's values
    /// are, and left out when it isn't one.
    pub fn pinned(mut self, kind: &str, found: &link_cloud_contract::Found) -> BoxSpec {
        let want = self.want(kind);
        let Ok(kind) = CloudKind::try_from(kind.to_string()) else {
            return self;
        };
        let machine = self.machine.get_or_insert_default();
        let table = machine.others.entry(kind.to_string()).or_default();
        if let Ok(size) = CloudName::try_from(found.size.clone()) {
            table.size = Some(size.as_str().to_string());
            table.cpus = None;
            table.memory = None;
            table.gpus = None;
            table.arch = None;
        }
        if let Ok(region) = CloudName::try_from(found.place.clone()) {
            table.region = Some(region.as_str().to_string());
        }
        if let Some(gb) = found.disk_gb.filter(|gb| (1..=16 * 1024).contains(gb)) {
            let disk_type = found.disk_type.as_deref().and_then(from_word::<DiskType>);
            if gb != want.disk.whole_gb() || disk_type.is_some_and(|t| t != want.disk_type) {
                table.disk = Some(Disk {
                    min: Some(Size::gb(gb).to_string()),
                    r#type: disk_type.and_then(|t| same(&t).ok()),
                });
            }
        }
        if let Some(os) = found.os.as_deref().and_then(from_word::<Os>)
            && os != want.os
        {
            table.os = Some(os);
        }
        if let Some(spot) = found.spot
            && spot != want.spot
        {
            table.spot = Some(spot);
        }
        if *table == Machine::default() {
            machine.others.remove(kind.as_str());
        }
        if *machine == Machine::default() {
            self.machine = None;
        }
        self
    }

    /// What `flags` set in `[machine]` that cloud `kind`'s own table
    /// outranks there, each said for a note: the table's own key, or its
    /// size, which has its cpus, memory and GPUs built in. (A size's
    /// processor is checked against `arch`, never outranked.)
    pub fn outranked(&self, kind: &str, flags: &link_spec::FieldFlags) -> Vec<String> {
        let Some(table) = self.machine().others.get(kind) else {
            return Vec::new();
        };
        let own = toml::Table::try_from(table).unwrap_or_default();
        Fields::NAMES
            .iter()
            .filter(|field| flags.get(&["machine", field]).is_some())
            .filter_map(|field| match (&table.size, own.contains_key(*field)) {
                (_, true) => Some(format!(
                    "[machine.{kind}] {field} outranks --machine.{field} there: \
                     give --machine.{kind}.{field} to change it."
                )),
                (Some(size), false) if matches!(*field, "cpus" | "memory" | "gpus") => {
                    Some(format!(
                        "[machine.{kind}] size {size} has its {field} built in, so \
                         --machine.{field} doesn't change it there: give \
                         --machine.{kind}.size another type to."
                    ))
                }
                _ => None,
            })
            .collect()
    }

    /// What the spec asks of a box in cloud `kind`: its table's fields,
    /// else `[machine]`'s, else Link's defaults.
    pub fn want(&self, kind: &str) -> Want {
        let base = self.fields();
        let table = self.cloud(kind);
        let table = table.as_ref();
        macro_rules! pick {
            ($field:ident) => {
                table
                    .and_then(|t| t.$field.clone())
                    .or_else(|| base.$field.clone())
            };
        }
        // GPUs, the disk and the tags are each replaced whole.
        let gpus = |f: &Fields| f.gpus.is_some() || f.gpu_model.is_some();
        let (gpu_count, gpu_model) = match table.filter(|t| gpus(t)) {
            Some(t) => (t.gpus, t.gpu_model),
            None => (base.gpus, base.gpu_model),
        };
        let disk = |f: &Fields| f.disk_min.is_some() || f.disk_type.is_some();
        let (disk_min, disk_type) = match table.filter(|t| disk(t)) {
            Some(t) => (t.disk_min, t.disk_type),
            None => (base.disk_min, base.disk_type),
        };
        let tags = match table.filter(|t| !t.tags.is_empty()) {
            Some(t) => t.tags.clone(),
            None => base.tags.clone(),
        };
        let size = table.and_then(|t| t.size.clone()).or(base.size.clone());
        let cpus = pick!(cpus);
        let memory = pick!(memory);
        let arch = pick!(arch);
        // An arm64 box is matched too: the adapter's default is x86_64.
        let matching = size.is_none()
            && (cpus.is_some()
                || memory.is_some()
                || gpu_count.is_some()
                || gpu_model.is_some()
                || arch.is_some_and(|a| a != Arch::X8664));
        Want {
            cpus: cpus.unwrap_or(Range {
                min: None,
                max: None,
            }),
            memory: memory.unwrap_or(Range {
                min: Some(Size {
                    mb: LEAST_MEMORY_MB,
                }),
                max: None,
            }),
            gpus: match (gpu_count, gpu_model) {
                (Some(count), _) => count,
                (None, Some(_)) => Range {
                    min: Some(1),
                    max: None,
                },
                (None, None) => Range::exactly(0),
            },
            gpu_model,
            arch: arch.unwrap_or_default(),
            disk: disk_min.unwrap_or(Size::gb(DISK_GB)),
            disk_type: disk_type.unwrap_or_default(),
            os: pick!(os).unwrap_or_default(),
            spot: pick!(spot).unwrap_or(false),
            tags,
            region: pick!(region),
            size,
            matching,
        }
    }
}

/// `want`, made to fit `image`: its region the image's (an AWS image is
/// in one), its disk at least the image's (raised, saying so in what's
/// returned). An image not ready, of another processor, or in another
/// region than `want` names is refused.
pub fn fit(
    image: &link_cloud_contract::FoundImage,
    want: &mut Want,
) -> anyhow::Result<Option<String>> {
    ensure!(
        image.state == "ready",
        "image {} is {}, not ready yet",
        image.name,
        link_format::clean(&image.state)
    );
    if let Some(arch) = &image.arch {
        let box_arch = word(&want.arch);
        ensure!(
            *arch == box_arch,
            "image {} is {}, and the box would be {box_arch}: a box from it has the image's \
             processor",
            image.name,
            link_format::clean(arch)
        );
    }
    if !image.place.is_empty() {
        let place = CloudName::try_from(image.place.clone()).map_err(|e| anyhow::anyhow!(e))?;
        match &want.region {
            Some(region) if *region != place => bail!(
                "image {} is in {place}, and a box from it is made there, not in {region}: leave \
                 out the region",
                image.name
            ),
            _ => want.region = Some(place),
        }
    }
    let Some(gb) = image.disk_gb else {
        return Ok(None);
    };
    if want.disk.whole_gb() >= gb {
        return Ok(None);
    }
    let asked = want.disk.whole_gb();
    want.disk = Size::gb(gb);
    Ok(Some(format!(
        "Its disk is {gb} GB, the image's size, rather than {asked} GB."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found_image(place: &str, gb: u64, arch: &str) -> link_cloud_contract::FoundImage {
        link_cloud_contract::FoundImage {
            id: "ami-1".into(),
            name: "web-ready".into(),
            place: place.into(),
            state: "ready".into(),
            disk_gb: Some(gb),
            arch: Some(arch.into()),
            created: None,
            hash: None,
        }
    }

    /// A flag for `[machine]` that a cloud's table outranks is said, by
    /// the table's own key or its size; one nothing outranks isn't.
    #[test]
    fn a_flag_a_cloud_table_outranks_is_said() {
        let spec = BoxSpec::parse(
            "schema = 1\n[machine.aws]\nsize = \"m7i.large\"\n[machine.gcp]\nos = \"debian-13\"\n",
        )
        .unwrap();
        let binding = link_spec::Binding::of::<BoxSpec>();
        let flags = |words: &[&str]| {
            let args = std::iter::once("lnk")
                .chain(words.iter().copied())
                .map(Into::into);
            link_spec::split(args, Some(&binding), None).unwrap().0
        };
        let given = flags(&["--machine.cpus", "4", "--machine.os", "ubuntu-24.04"]);
        let aws = spec.outranked("aws", &given);
        assert_eq!(aws.len(), 1, "{aws:?}");
        assert!(
            aws[0].contains("size m7i.large has its cpus built in"),
            "{aws:?}"
        );
        let gcp = spec.outranked("gcp", &given);
        assert_eq!(gcp.len(), 1, "{gcp:?}");
        assert!(
            gcp[0].contains("[machine.gcp] os outranks --machine.os"),
            "{gcp:?}"
        );
        // The loosening: arch beside a size is checked, not outranked, and
        // a cloud with no table of its own outranks nothing.
        assert!(
            spec.outranked("aws", &flags(&["--machine.arch", "arm64"]))
                .is_empty()
        );
        assert!(spec.outranked("azure", &given).is_empty());
    }

    #[test]
    fn a_box_from_an_image_fits_it() {
        let mut want = BoxSpec::default().want("aws");
        let said = fit(&found_image("eu-west-2", 30, "x86_64"), &mut want).unwrap();
        assert_eq!(
            said.as_deref(),
            Some("Its disk is 30 GB, the image's size, rather than 20 GB.")
        );
        assert_eq!(want.disk.whole_gb(), 30);
        assert_eq!(want.region.unwrap().as_str(), "eu-west-2");
        // A bigger disk asked stays; an image anywhere leaves the region.
        let mut big = BoxSpec::default().want("gcp");
        big.disk = Size::gb(100);
        assert_eq!(fit(&found_image("", 30, "x86_64"), &mut big).unwrap(), None);
        assert_eq!(big.disk.whole_gb(), 100);
        assert_eq!(big.region, None);
        // Another processor, region, or not ready: refused.
        let mut want = BoxSpec::default().want("aws");
        let arm = fit(&found_image("eu-west-2", 8, "arm64"), &mut want).unwrap_err();
        assert!(arm.to_string().contains("is arm64"), "{arm}");
        want.region = Some(CloudName::try_from("us-east-1".to_string()).unwrap());
        let away = fit(&found_image("eu-west-2", 8, "x86_64"), &mut want).unwrap_err();
        assert!(away.to_string().contains("is in eu-west-2"), "{away}");
        let mut pending = found_image("", 8, "x86_64");
        pending.state = "pending".into();
        assert!(fit(&pending, &mut want).is_err());
    }
}
