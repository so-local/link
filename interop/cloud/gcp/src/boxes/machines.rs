//! What a box is made of on Compute Engine: its image, its machine type
//! and the GPUs it attaches, read from a spec and from the zone's types.

use super::*;

/// The image of each OS and processor, without and with NVIDIA's driver
/// (x86_64 only), as an image family and the project publishing it:
/// Canonical's own projects, never a search of image names, which anyone
/// can publish under.
const IMAGES: &[(&str, &str, bool, &str, &str)] = &[
    (
        "ubuntu-24.04",
        "x86_64",
        false,
        "ubuntu-2404-lts-amd64",
        "ubuntu-os-cloud",
    ),
    (
        "ubuntu-22.04",
        "x86_64",
        false,
        "ubuntu-2204-lts",
        "ubuntu-os-cloud",
    ),
    (
        "ubuntu-24.04",
        "arm64",
        false,
        "ubuntu-2404-lts-arm64",
        "ubuntu-os-cloud",
    ),
    (
        "ubuntu-22.04",
        "arm64",
        false,
        "ubuntu-2204-lts-arm64",
        "ubuntu-os-cloud",
    ),
    (
        "ubuntu-24.04",
        "x86_64",
        true,
        "ubuntu-accelerator-2404-amd64-with-nvidia-550",
        "ubuntu-os-accelerator-images",
    ),
    (
        "ubuntu-22.04",
        "x86_64",
        true,
        "ubuntu-accelerator-2204-amd64-with-nvidia-550",
        "ubuntu-os-accelerator-images",
    ),
];

/// `spec`'s image: its family and project.
pub fn image(spec: &Spec) -> anyhow::Result<(&'static str, &'static str)> {
    spec.check()?;
    IMAGES
        .iter()
        .find(|(os, arch, gpu, _, _)| {
            *os == spec.os && *arch == spec.arch && *gpu == (spec.gpus > 0)
        })
        .map(|(_, _, _, family, project)| (*family, *project))
        .with_context(|| {
            format!(
                "no Google Cloud image for {} on {}",
                link_format::clean(&spec.os),
                link_format::clean(&spec.arch)
            )
        })
}

/// `spec`'s disk, spot and GPU arguments to `instances create`.
pub fn machine_args(spec: &Spec) -> anyhow::Result<Vec<String>> {
    spec.check()?;
    ensure!(
        spec.deadline.is_none_or(|d| d.get().as_secs() >= 30),
        "a Google Cloud deadline is at least 30 seconds"
    );
    let disk_type = match spec.disk_type.as_str() {
        "fast" => "pd-ssd",
        _ => "pd-balanced",
    };
    let mut args = vec![
        "--boot-disk-size".to_string(),
        format!("{}GB", spec.disk_gb),
        "--boot-disk-type".into(),
        disk_type.into(),
    ];
    if let Some(deadline) = spec.deadline {
        args.extend([
            "--max-run-duration".into(),
            format!("{}s", deadline.get().as_secs()),
            "--instance-termination-action=DELETE".into(),
        ]);
    }
    if spec.spot {
        // Stopped, never deleted, when Google takes it back: its disk stays.
        args.extend(
            [
                "--provisioning-model",
                "SPOT",
                "--instance-termination-action",
                if spec.deadline.is_some() {
                    "DELETE"
                } else {
                    "STOP"
                },
            ]
            .map(String::from),
        );
    }
    let (_, attached) = split_size(&spec.size)?;
    if let Some((accelerator, count)) = attached {
        args.extend([
            "--accelerator".to_string(),
            format!("type={accelerator},count={count}"),
        ]);
    }
    if spec.gpus > 0 || attached.is_some() {
        // A machine with GPUs can't move live to another host.
        args.extend(["--maintenance-policy", "TERMINATE"].map(String::from));
    }
    Ok(args)
}

/// Machine families in order of preference: general purpose first, the
/// cheapest family first; then compute, memory, and GPU families. Others
/// come after, by name.
const FAMILIES: &[&str] = &[
    "e2", "n4", "n2", "n2d", "t2d", "c4", "c3", "c3d", "c2", "c2d", "n1", "m3", "m1", "g2", "a2",
    "a3", "t2a", "c4a",
];
/// Families of arm64 machines.
pub(super) const ARM: &[&str] = &["t2a", "c4a", "n4a"];

/// Google Cloud's accelerator types in Link's words.
const GPUS: &[(&str, &str)] = &[
    ("nvidia-tesla-t4", "nvidia-t4"),
    ("nvidia-l4", "nvidia-l4"),
    ("nvidia-tesla-a100", "nvidia-a100"),
    ("nvidia-a100-80gb", "nvidia-a100"),
    ("nvidia-h100-80gb", "nvidia-h100"),
    ("nvidia-h100-mega-80gb", "nvidia-h100"),
    ("nvidia-h200-141gb", "nvidia-h200"),
    ("nvidia-b200", "nvidia-b200"),
];

/// The counts of a GPU a machine can have, each with the most vCPUs it
/// may have with that many.
type Counts = &'static [(u32, u32)];
/// GPUs Google attaches to an N1 machine rather than building them into
/// its type.
const ATTACHED: &[(&str, Counts)] = &[("nvidia-tesla-t4", &[(1, 48), (2, 48), (4, 96)])];
/// The machine types those GPUs attach to, by the start of their name.
const ATTACH_TO: &[&str] = &["n1-standard-", "n1-highmem-", "n1-highcpu-"];

/// A machine type with GPUs attached, as Link names it:
/// `<machine type>.<accelerator type>.<count>` (`n1-standard-4.nvidia-tesla-t4.1`).
/// Google names neither with a dot, so the name splits back.
fn attached_size(machine: &str, accelerator: &str, count: u32) -> String {
    format!("{machine}.{accelerator}.{count}")
}

/// `size` as its machine type and the GPUs attached to it, if any: their
/// accelerator type and count, one of [`ATTACHED`]'s, so neither can add
/// a field to `--accelerator`'s list.
pub fn split_size(size: &str) -> anyhow::Result<(&str, Option<(&'static str, u32)>)> {
    let mut parts = size.split('.');
    let machine = parts.next().unwrap_or_default();
    let (Some(accelerator), Some(count)) = (parts.next(), parts.next()) else {
        ensure!(
            !size.contains('.'),
            "{:?} isn't a Google Cloud machine type, or one with GPUs attached \
             (n1-standard-4.nvidia-tesla-t4.1)",
            link_format::clean(size)
        );
        return Ok((machine, None));
    };
    let attached = ATTACHED.iter().find(|(name, _)| *name == accelerator);
    let count: Option<u32> = count.parse().ok();
    match (attached, count) {
        (Some((name, counts)), Some(count))
            if parts.next().is_none()
                && counts.iter().any(|(n, _)| *n == count)
                && ATTACH_TO.iter().any(|f| machine.starts_with(f)) =>
        {
            Ok((machine, Some((name, count))))
        }
        _ => anyhow::bail!(
            "{:?} isn't a machine type with GPUs Google attaches: an N1 machine with 1, 2 or 4 \
             nvidia-tesla-t4 (n1-standard-4.nvidia-tesla-t4.1)",
            link_format::clean(size)
        ),
    }
}

/// The machine types `machine-types list` lists for a zone: not
/// deprecated, with no GPU or GPUs built in that Link names, and each N1
/// machine again with every count of GPUs `accelerator-types list` says
/// the zone attaches to it ([`attached_size`]); in [`FAMILIES`]' order,
/// then by CPUs, memory and GPUs. Google gives no price there.
pub fn parse_sizes(
    types: &serde_json::Value,
    accelerators: &serde_json::Value,
) -> Vec<MachineSize> {
    let mut sizes: Vec<MachineSize> = types
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let size = t["name"].as_str()?;
            if t["deprecated"]["state"].is_string() {
                return None;
            }
            let family = size.split('-').next().unwrap_or(size);
            let arch = match t["architecture"].as_str() {
                Some("ARM64") => "arm64",
                Some(_) => "x86_64",
                None if ARM.contains(&family) => "arm64",
                None => "x86_64",
            };
            let (mut gpus, mut model) = (0, None);
            for gpu in t["accelerators"].as_array().into_iter().flatten() {
                let kind = gpu["guestAcceleratorType"].as_str().unwrap_or_default();
                let words = GPUS.iter().find(|(k, _)| *k == kind).map(|(_, w)| *w);
                words?;
                gpus += gpu["guestAcceleratorCount"].as_u64().unwrap_or(0) as u32;
                model = words;
            }
            Some(MachineSize {
                size: size.to_string(),
                cpus: t["guestCpus"].as_u64()?,
                memory_mb: t["memoryMb"].as_u64()?,
                gpus: gpus.into(),
                gpu_model: model.map(str::to_string),
                arch: arch.to_string(),
                price: None,
            })
        })
        .collect();
    // The GPUs this zone attaches, and how many at most.
    let here: Vec<(&str, Counts, u64)> = accelerators
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let name = a["name"].as_str()?;
            let (name, counts) = ATTACHED.iter().find(|(n, _)| *n == name)?;
            let most = a["maximumCardsPerInstance"].as_u64()?;
            Some((*name, *counts, most))
        })
        .collect();
    let mut with_gpus = Vec::new();
    for size in sizes.iter().filter(|s| s.gpus == 0) {
        if !ATTACH_TO.iter().any(|f| size.size.starts_with(f)) {
            continue;
        }
        for (name, counts, most) in &here {
            let model = GPUS.iter().find(|(k, _)| k == name).map(|(_, w)| *w);
            for (count, cpus) in counts.iter() {
                if u64::from(*count) <= *most && size.cpus() <= *cpus {
                    with_gpus.push(MachineSize {
                        size: attached_size(&size.size, name, *count),
                        gpus: (*count).into(),
                        gpu_model: model.map(str::to_string),
                        ..size.clone()
                    });
                }
            }
        }
    }
    sizes.extend(with_gpus);
    let family = |size: &str| {
        let name = size.split('-').next().unwrap_or(size);
        FAMILIES
            .iter()
            .position(|f| *f == name)
            .unwrap_or(FAMILIES.len())
    };
    sizes.sort_by(|a, b| {
        (
            family(&a.size),
            a.size.split('-').next(),
            a.cpus,
            a.memory_mb,
            a.gpus,
        )
            .cmp(&(
                family(&b.size),
                b.size.split('-').next(),
                b.cpus,
                b.memory_mb,
                b.gpus,
            ))
    });
    sizes
}

impl Gcp {
    /// The machine types a box can be in this zone, with the GPUs it
    /// attaches to N1 machines.
    pub fn sizes(&self) -> anyhow::Result<Vec<MachineSize>> {
        let zone = format!("zone:{}", self.zone);
        let (types, accelerators) = both(
            || {
                self.cli.json(&[
                    "compute",
                    "machine-types",
                    "list",
                    "--zones",
                    &self.zone,
                    "--format",
                    "json",
                ])
            },
            || {
                self.cli.json(&[
                    "compute",
                    "accelerator-types",
                    "list",
                    "--filter",
                    &zone,
                    "--format",
                    "json",
                ])
            },
        );
        Ok(parse_sizes(&types?, &accelerators?))
    }

    /// The disk in GiB the current image of `family` in `project` needs
    /// at least, as Google says: none when it won't say.
    pub(super) fn image_disk_gb(&self, family: &str, project: &str) -> Option<u64> {
        let image = self
            .cli
            .json(&[
                "compute",
                "images",
                "describe-from-family",
                family,
                "--project",
                project,
                "--format",
                "json",
            ])
            .ok()?;
        image_gb(&image)
    }
}

/// An image's disk in GiB, as `images describe` says it (a string).
pub fn image_gb(image: &serde_json::Value) -> Option<u64> {
    let size = &image["diskSizeGb"];
    size.as_str().and_then(|s| s.parse().ok()).or(size.as_u64())
}

/// `spec` with the GPUs its size attaches, when it names some.
pub(super) fn with_attached(spec: &Spec) -> anyhow::Result<Spec> {
    let (_, attached) = split_size(&spec.size)?;
    Ok(match attached {
        Some((accelerator, count)) => Spec {
            gpus: count.into(),
            gpu_model: GPUS
                .iter()
                .find(|(k, _)| *k == accelerator)
                .map(|(_, w)| w.to_string()),
            ..spec.clone()
        },
        None => spec.clone(),
    })
}

/// A box's size as Link names it: its machine type, with the GPUs
/// attached to it when they're ones Link attaches.
pub(super) fn size_of(i: &serde_json::Value) -> String {
    let machine = last(&i["machineType"]);
    let attached = i["guestAccelerators"]
        .as_array()
        .and_then(|all| match all.as_slice() {
            [one] => {
                let kind = last(&one["acceleratorType"]);
                let count = one["acceleratorCount"].as_u64()? as u32;
                let size = attached_size(&machine, &kind, count);
                split_size(&size).ok()?.1.map(|_| size)
            }
            _ => None,
        });
    attached.unwrap_or(machine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_the_zones_in_order_of_preference() {
        let types = serde_json::json!([
            {"name": "n2-standard-2", "guestCpus": 2, "memoryMb": 8192},
            {"name": "e2-small", "guestCpus": 2, "memoryMb": 2048, "isSharedCpu": true},
            {"name": "e2-standard-4", "guestCpus": 4, "memoryMb": 16384},
            {"name": "g2-standard-4", "guestCpus": 4, "memoryMb": 16384,
             "accelerators": [{"guestAcceleratorType": "nvidia-l4", "guestAcceleratorCount": 1}]},
            {"name": "x9-odd-2", "guestCpus": 2, "memoryMb": 4096,
             "accelerators": [{"guestAcceleratorType": "some-tpu", "guestAcceleratorCount": 1}]},
            {"name": "t2a-standard-1", "guestCpus": 1, "memoryMb": 4096},
            {"name": "n1-standard-1", "guestCpus": 1, "memoryMb": 3840,
             "deprecated": {"state": "DEPRECATED"}}
        ]);
        let sizes = parse_sizes(&types, &serde_json::json!([]));
        let names: Vec<_> = sizes.iter().map(|s| s.size.as_str()).collect();
        assert_eq!(
            names,
            [
                "e2-small",
                "e2-standard-4",
                "n2-standard-2",
                "g2-standard-4",
                "t2a-standard-1"
            ]
        );
        assert_eq!(sizes[3].gpu_model(), "nvidia-l4");
        assert_eq!(sizes[4].arch, "arm64");
    }

    #[test]
    fn an_empty_spec_is_todays_box() {
        let spec = Spec::default();
        assert_eq!(
            image(&spec).unwrap(),
            ("ubuntu-2404-lts-amd64", "ubuntu-os-cloud")
        );
        assert_eq!(
            machine_args(&spec).unwrap(),
            [
                "--boot-disk-size",
                "20GB",
                "--boot-disk-type",
                "pd-balanced"
            ]
        );
        assert_eq!(image_gb(&serde_json::json!({"diskSizeGb": "50"})), Some(50));
        assert_eq!(image_gb(&serde_json::json!({"diskSizeGb": 10})), Some(10));
        assert_eq!(image_gb(&serde_json::json!({})), None);
        let bad = Spec {
            os: "debian-12".into(),
            ..Spec::default()
        };
        assert!(image(&bad).is_err() && machine_args(&bad).is_err());
        // An arm64 box: Canonical's arm64 image; with GPUs, none.
        let arm = Spec {
            arch: "arm64".into(),
            os: "ubuntu-22.04".into(),
            ..Spec::default()
        };
        assert_eq!(
            image(&arm).unwrap(),
            ("ubuntu-2204-lts-arm64", "ubuntu-os-cloud")
        );
        let arm_gpu = Spec { gpus: 1, ..arm };
        assert!(image(&arm_gpu).is_err());
        assert!(crate::cli::is_region("europe-west2"));
        assert!(!crate::cli::is_region("europe-west2-b"));
    }

    #[test]
    fn n1_machines_are_listed_again_with_the_gpus_the_zone_attaches() {
        let types = serde_json::json!([
            {"name": "n1-standard-4", "guestCpus": 4, "memoryMb": 15360},
            {"name": "n1-standard-64", "guestCpus": 64, "memoryMb": 245760},
            {"name": "n1-megamem-96", "guestCpus": 96, "memoryMb": 1468006},
            {"name": "e2-standard-4", "guestCpus": 4, "memoryMb": 16384}
        ]);
        let accelerators = serde_json::json!([
            {"name": "nvidia-tesla-t4", "maximumCardsPerInstance": 2},
            {"name": "nvidia-tesla-p100", "maximumCardsPerInstance": 4}
        ]);
        let sizes = parse_sizes(&types, &accelerators);
        let names: Vec<_> = sizes.iter().map(|s| s.size.as_str()).collect();
        // Up to the zone's most cards, and only within the vCPUs each count
        // allows; never on a machine GPUs don't attach to.
        assert_eq!(
            names,
            [
                "e2-standard-4",
                "n1-standard-4",
                "n1-standard-4.nvidia-tesla-t4.1",
                "n1-standard-4.nvidia-tesla-t4.2",
                "n1-standard-64",
                "n1-megamem-96"
            ]
        );
        assert_eq!((sizes[2].gpus, sizes[2].gpu_model()), (1, "nvidia-t4"));
        assert_eq!((sizes[3].cpus, sizes[3].memory_mb), (4, 15360));
    }

    #[test]
    fn a_size_with_gpus_attached_splits_into_its_machine_and_accelerator() {
        assert_eq!(split_size("e2-small").unwrap(), ("e2-small", None));
        assert_eq!(
            split_size("n1-highmem-8.nvidia-tesla-t4.4").unwrap(),
            ("n1-highmem-8", Some(("nvidia-tesla-t4", 4)))
        );
        for bad in [
            "n1-standard-4.nvidia-tesla-t4.3",
            "n1-standard-4.nvidia-tesla-p100.1",
            "e2-standard-4.nvidia-tesla-t4.1",
            "n1-standard-4.nvidia-tesla-t4.1.x",
            "n1-standard-4.nvidia-tesla-t4",
            "n1-standard-4.nvidia-tesla-t4.1,count=8",
        ] {
            assert!(split_size(bad).is_err(), "{bad}");
        }
        let spec = Spec {
            size: "n1-standard-4.nvidia-tesla-t4.1".into(),
            ..Spec::default()
        };
        let spec = with_attached(&spec).unwrap();
        assert_eq!((spec.gpus, spec.gpu_model()), (1, "nvidia-t4"));
        assert_eq!(
            image(&spec).unwrap(),
            (
                "ubuntu-accelerator-2404-amd64-with-nvidia-550",
                "ubuntu-os-accelerator-images"
            )
        );
        let args = machine_args(&spec).unwrap();
        assert!(
            args.join(" ").ends_with(
                "--accelerator type=nvidia-tesla-t4,count=1 --maintenance-policy TERMINATE"
            ),
            "{args:?}"
        );
    }

    #[test]
    fn a_box_with_gpus_attached_says_so_in_its_size() {
        let box_with = |accelerators: serde_json::Value| {
            size_of(&serde_json::json!({
                "machineType": "https://x/zones/us-central1-a/machineTypes/n1-standard-4",
                "guestAccelerators": accelerators
            }))
        };
        assert_eq!(
            box_with(serde_json::json!([{
                "acceleratorType": "https://x/zones/us-central1-a/acceleratorTypes/nvidia-tesla-t4",
                "acceleratorCount": 1
            }])),
            "n1-standard-4.nvidia-tesla-t4.1"
        );
        // One Link doesn't attach: the machine type alone.
        assert_eq!(
            box_with(serde_json::json!([{
                "acceleratorType": "https://x/acceleratorTypes/nvidia-tesla-v100",
                "acceleratorCount": 1
            }])),
            "n1-standard-4"
        );
        assert_eq!(box_with(serde_json::json!(null)), "n1-standard-4");
    }
}
