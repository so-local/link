//! What a box is launched as on EC2: its image and disk, its tags and
//! spot options, and the instance types a region offers.

use super::*;

/// The image of each OS and processor, without and with NVIDIA's driver
/// (x86_64 only): SSM parameters only Canonical (Ubuntu's) and AWS (its
/// Deep Learning Base AMI on Ubuntu) publish, never a search of image
/// names, which anyone can publish under.
const IMAGES: &[(&str, &str, bool, &str)] = &[
    (
        "ubuntu-24.04",
        "x86_64",
        false,
        "/aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id",
    ),
    (
        "ubuntu-22.04",
        "x86_64",
        false,
        "/aws/service/canonical/ubuntu/server/22.04/stable/current/amd64/hvm/ebs-gp2/ami-id",
    ),
    (
        "ubuntu-24.04",
        "arm64",
        false,
        "/aws/service/canonical/ubuntu/server/24.04/stable/current/arm64/hvm/ebs-gp3/ami-id",
    ),
    (
        "ubuntu-22.04",
        "arm64",
        false,
        "/aws/service/canonical/ubuntu/server/22.04/stable/current/arm64/hvm/ebs-gp2/ami-id",
    ),
    (
        "ubuntu-24.04",
        "x86_64",
        true,
        "/aws/service/deeplearning/ami/x86_64/base-oss-nvidia-driver-gpu-ubuntu-24.04/latest/ami-id",
    ),
    (
        "ubuntu-22.04",
        "x86_64",
        true,
        "/aws/service/deeplearning/ami/x86_64/base-oss-nvidia-driver-gpu-ubuntu-22.04/latest/ami-id",
    ),
];

/// The SSM parameter naming `spec`'s image.
pub fn image_parameter(spec: &Spec) -> anyhow::Result<&'static str> {
    spec.check()?;
    IMAGES
        .iter()
        .find(|(os, arch, gpu, _)| *os == spec.os && *arch == spec.arch && *gpu == (spec.gpus > 0))
        .map(|(_, _, _, parameter)| *parameter)
        .with_context(|| {
            format!(
                "no AWS image for {} on {}",
                link_format::clean(&spec.os),
                link_format::clean(&spec.arch)
            )
        })
}

/// `--block-device-mappings` for `spec`'s disk as the image's root
/// device (`device`), as JSON: gp3, or io2 with 50 IOPS a GB (at least
/// 3,000, gp3's own, at most 64,000).
pub fn disk(spec: &Spec, device: &str) -> anyhow::Result<String> {
    spec.check()?;
    let mut ebs = serde_json::json!({
        "VolumeSize": spec.disk_gb,
        "VolumeType": "gp3",
        "DeleteOnTermination": true,
        "Encrypted": true,
    });
    if spec.disk_type == "fast" {
        ebs["VolumeType"] = "io2".into();
        ebs["Iops"] = (spec.disk_gb * 50).clamp(3000, 64_000).into();
    }
    Ok(serde_json::to_string(&serde_json::json!([
        {"DeviceName": device, "Ebs": ebs}
    ]))?)
}

/// The root device Ubuntu's images on AWS boot from.
pub(super) const ROOT: &str = "/dev/sda1";

/// An image's root device and its disk in GiB, as `describe-images`
/// says them: a box's disk is at least that, or AWS refuses to make it
/// ("volume smaller than snapshot"). The Deep Learning Base AMI's is
/// larger than a box's 20 GB.
pub fn image_root(images: &serde_json::Value) -> (String, Option<u64>) {
    let image = &images["Images"][0];
    let device = image["RootDeviceName"]
        .as_str()
        .filter(|d| d.starts_with("/dev/") && d.len() < 32)
        .unwrap_or(ROOT);
    let gb = image["BlockDeviceMappings"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["DeviceName"] == device)
        .and_then(|m| m["Ebs"]["VolumeSize"].as_u64());
    (device.to_string(), gb)
}

/// `--tag-specifications` as JSON: `tags` on the instance and its disk
/// (so the bill says what each box's disk costs), and on a spot box's
/// request, which `cleanup` finds by them.
pub fn tag_specifications(tags: &BTreeMap<String, String>, spot: bool) -> anyhow::Result<String> {
    for (key, value) in tags {
        tag_value(key)?;
        tag_value(value)?;
    }
    let tags: Vec<_> = tags
        .iter()
        .map(|(k, v)| serde_json::json!({"Key": k, "Value": v}))
        .collect();
    let mut kinds = vec!["instance", "volume"];
    if spot {
        kinds.push("spot-instances-request");
    }
    let specs: Vec<_> = kinds
        .into_iter()
        .map(|kind| serde_json::json!({"ResourceType": kind, "Tags": tags}))
        .collect();
    Ok(serde_json::to_string(&specs)?)
}

/// `--instance-market-options` for a spot box: a persistent request,
/// stopping (never terminating) the instance when AWS takes it back, so
/// its disk stays.
pub fn spot_options() -> String {
    serde_json::json!({
        "MarketType": "spot",
        "SpotOptions": {
            "SpotInstanceType": "persistent",
            "InstanceInterruptionBehavior": "stop",
        },
    })
    .to_string()
}

/// Instance families in order of preference: general purpose and
/// burstable first, the current generation, the cheapest family first;
/// then compute, memory, and GPU families. Others come after, by name.
const FAMILIES: &[&str] = &[
    "t3", "t3a", "m7i-flex", "m7i", "m7a", "m6i", "m6a", "m5", "m5a", "c7i", "c7a", "c6i", "c6a",
    "c5", "r7i", "r7a", "r6i", "r6a", "r5", "g6", "g6e", "g5", "g4dn", "p4d", "p5", "t4g", "m7g",
    "m6g", "c7g", "c6g", "r7g", "r6g",
];

/// AWS's GPU names (`GpuInfo`) in Link's words.
const GPUS: &[(&str, &str)] = &[
    ("T4", "nvidia-t4"),
    ("L4", "nvidia-l4"),
    ("L40S", "nvidia-l40s"),
    ("A10G", "nvidia-a10g"),
    ("A100", "nvidia-a100"),
    ("H100", "nvidia-h100"),
    ("H200", "nvidia-h200"),
    ("B200", "nvidia-b200"),
];

/// The machine types `describe-instance-types` lists (`types`) that
/// `describe-instance-type-offerings` (`offered`) says the region has:
/// virtual, x86_64 or arm64 Linux, with no GPU or one Link names, in
/// [`FAMILIES`]' order, then by CPUs and memory. AWS gives no price.
pub fn parse_sizes(types: &serde_json::Value, offered: &serde_json::Value) -> Vec<MachineSize> {
    let offered: std::collections::HashSet<&str> = offered["InstanceTypeOfferings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| o["InstanceType"].as_str())
        .collect();
    let mut sizes: Vec<MachineSize> = types["InstanceTypes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let size = t["InstanceType"].as_str()?;
            if !offered.contains(size) || t["BareMetal"] == true {
                return None;
            }
            let archs = t["ProcessorInfo"]["SupportedArchitectures"].as_array()?;
            let arch = ["x86_64", "arm64"]
                .into_iter()
                .find(|a| archs.iter().any(|x| x == a))?;
            let (mut gpus, mut model) = (0, None);
            for gpu in t["GpuInfo"]["Gpus"].as_array().into_iter().flatten() {
                let name = gpu["Name"].as_str().unwrap_or_default();
                let words = GPUS.iter().find(|(n, _)| *n == name).map(|(_, w)| *w);
                if gpu["Manufacturer"] != "NVIDIA" || words.is_none() {
                    return None;
                }
                gpus += gpu["Count"].as_u64().unwrap_or(0) as u32;
                model = words;
            }
            Some(MachineSize {
                size: size.to_string(),
                cpus: t["VCpuInfo"]["DefaultVCpus"].as_u64()?,
                memory_mb: t["MemoryInfo"]["SizeInMiB"].as_u64()?,
                gpus: gpus.into(),
                gpu_model: model.map(str::to_string),
                arch: arch.to_string(),
                price: None,
            })
        })
        .collect();
    let family = |size: &str| {
        let name = size.split('.').next().unwrap_or(size);
        FAMILIES
            .iter()
            .position(|f| *f == name)
            .unwrap_or(FAMILIES.len())
    };
    sizes.sort_by(|a, b| {
        (
            family(&a.size),
            a.size.split('.').next(),
            a.cpus,
            a.memory_mb,
        )
            .cmp(&(
                family(&b.size),
                b.size.split('.').next(),
                b.cpus,
                b.memory_mb,
            ))
    });
    sizes
}

impl Aws {
    /// The machine types a box can be in this region.
    pub fn sizes(&self) -> anyhow::Result<Vec<MachineSize>> {
        let (offered, types) = both(
            || {
                self.cli.json(&[
                    "ec2",
                    "describe-instance-type-offerings",
                    "--location-type",
                    "region",
                ])
            },
            || {
                self.cli.json(&[
                    "ec2",
                    "describe-instance-types",
                    "--filters",
                    "Name=current-generation,Values=true",
                    "Name=bare-metal,Values=false",
                ])
            },
        );
        Ok(parse_sizes(&types?, &offered?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_the_regions_in_order_of_preference() {
        let types = serde_json::json!({"InstanceTypes": [
            {"InstanceType": "m7i.large", "BareMetal": false,
             "ProcessorInfo": {"SupportedArchitectures": ["x86_64"]},
             "VCpuInfo": {"DefaultVCpus": 2}, "MemoryInfo": {"SizeInMiB": 8192}},
            {"InstanceType": "t3.small", "BareMetal": false,
             "ProcessorInfo": {"SupportedArchitectures": ["i386", "x86_64"]},
             "VCpuInfo": {"DefaultVCpus": 2}, "MemoryInfo": {"SizeInMiB": 2048}},
            {"InstanceType": "t3.large", "BareMetal": false,
             "ProcessorInfo": {"SupportedArchitectures": ["x86_64"]},
             "VCpuInfo": {"DefaultVCpus": 2}, "MemoryInfo": {"SizeInMiB": 8192}},
            {"InstanceType": "g6.xlarge", "BareMetal": false,
             "ProcessorInfo": {"SupportedArchitectures": ["x86_64"]},
             "VCpuInfo": {"DefaultVCpus": 4}, "MemoryInfo": {"SizeInMiB": 16384},
             "GpuInfo": {"Gpus": [{"Name": "L4", "Manufacturer": "NVIDIA", "Count": 1}]}},
            {"InstanceType": "g4ad.xlarge", "BareMetal": false,
             "ProcessorInfo": {"SupportedArchitectures": ["x86_64"]},
             "VCpuInfo": {"DefaultVCpus": 4}, "MemoryInfo": {"SizeInMiB": 16384},
             "GpuInfo": {"Gpus": [{"Name": "Radeon Pro V520", "Manufacturer": "AMD", "Count": 1}]}},
            {"InstanceType": "t4g.small", "BareMetal": false,
             "ProcessorInfo": {"SupportedArchitectures": ["arm64"]},
             "VCpuInfo": {"DefaultVCpus": 2}, "MemoryInfo": {"SizeInMiB": 2048}},
            {"InstanceType": "mac2.metal", "BareMetal": true,
             "ProcessorInfo": {"SupportedArchitectures": ["arm64_mac"]},
             "VCpuInfo": {"DefaultVCpus": 8}, "MemoryInfo": {"SizeInMiB": 16384}},
            {"InstanceType": "m7i.xlarge", "BareMetal": false,
             "ProcessorInfo": {"SupportedArchitectures": ["x86_64"]},
             "VCpuInfo": {"DefaultVCpus": 4}, "MemoryInfo": {"SizeInMiB": 16384}}
        ]});
        let offered = serde_json::json!({"InstanceTypeOfferings": [
            {"InstanceType": "m7i.large"}, {"InstanceType": "t3.small"}, {"InstanceType": "t3.large"},
            {"InstanceType": "g6.xlarge"}, {"InstanceType": "g4ad.xlarge"},
            {"InstanceType": "t4g.small"}, {"InstanceType": "mac2.metal"}
        ]});
        let sizes = parse_sizes(&types, &offered);
        let names: Vec<_> = sizes.iter().map(|s| s.size.as_str()).collect();
        assert_eq!(
            names,
            [
                "t3.small",
                "t3.large",
                "m7i.large",
                "g6.xlarge",
                "t4g.small"
            ]
        );
        assert_eq!(sizes[3].gpus, 1);
        assert_eq!(sizes[3].gpu_model(), "nvidia-l4");
        assert_eq!(sizes[4].arch, "arm64");
        assert_eq!(sizes[0].memory_mb, 2048);
    }

    #[test]
    fn the_disk_is_at_least_the_images() {
        let images = serde_json::json!({"Images": [{"RootDeviceName": "/dev/sda1",
        "BlockDeviceMappings": [
            {"DeviceName": "/dev/sdb", "Ebs": {"VolumeSize": 500}},
            {"DeviceName": "/dev/sda1", "Ebs": {"VolumeSize": 75}}
        ]}]});
        assert_eq!(image_root(&images), ("/dev/sda1".to_string(), Some(75)));
        let odd = serde_json::json!({"Images": [{"RootDeviceName": "\"; x"}]});
        assert_eq!(image_root(&odd), ("/dev/sda1".to_string(), None));
        assert_eq!(image_root(&serde_json::Value::Null).1, None);
    }

    #[test]
    fn an_empty_spec_is_todays_box() {
        let spec = Spec::default();
        assert_eq!(
            image_parameter(&spec).unwrap(),
            "/aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id"
        );
        let arm = Spec {
            arch: "arm64".into(),
            ..Spec::default()
        };
        assert_eq!(
            image_parameter(&arm).unwrap(),
            "/aws/service/canonical/ubuntu/server/24.04/stable/current/arm64/hvm/ebs-gp3/ami-id"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&disk(&spec, "/dev/sda1").unwrap()).unwrap(),
            serde_json::json!([{"DeviceName":"/dev/sda1","Ebs":{"Encrypted":true,"DeleteOnTermination":true,"VolumeSize":20,"VolumeType":"gp3"}}])
        );
    }

    #[test]
    fn the_image_is_by_its_publisher_and_the_rest_typed() {
        let gpu = Spec {
            os: "ubuntu-22.04".into(),
            gpus: 1,
            gpu_model: Some("nvidia-l4".into()),
            disk_gb: 100,
            disk_type: "fast".into(),
            ..Spec::default()
        };
        assert!(image_parameter(&gpu).unwrap().starts_with(
            "/aws/service/deeplearning/ami/x86_64/base-oss-nvidia-driver-gpu-ubuntu-22.04/"
        ));
        let mapped: serde_json::Value =
            serde_json::from_str(&disk(&gpu, "/dev/xvda").unwrap()).unwrap();
        assert_eq!(mapped[0]["DeviceName"], "/dev/xvda");
        assert_eq!(mapped[0]["Ebs"]["VolumeType"], "io2");
        assert_eq!(mapped[0]["Ebs"]["Iops"], 5000);
        for bad in [
            Spec {
                os: "ubuntu-20.04".into(),
                ..Spec::default()
            },
            Spec {
                arch: "arm64".into(),
                gpus: 1,
                ..Spec::default()
            },
            Spec {
                disk_type: "gp2".into(),
                ..Spec::default()
            },
        ] {
            assert!(image_parameter(&bad).is_err(), "{bad:?}");
        }
        let tags = BTreeMap::from([("team".to_string(), "research".to_string())]);
        let specs: serde_json::Value =
            serde_json::from_str(&tag_specifications(&tags, true).unwrap()).unwrap();
        assert_eq!(specs[2]["ResourceType"], "spot-instances-request");
        assert_eq!(specs[0]["Tags"][0]["Value"], "research");
        let comma = BTreeMap::from([("a".to_string(), "b},{Key=c".to_string())]);
        assert!(tag_specifications(&comma, false).is_err());
        let spot: serde_json::Value = serde_json::from_str(&spot_options()).unwrap();
        assert_eq!(spot["SpotOptions"]["SpotInstanceType"], "persistent");
        assert_eq!(spot["SpotOptions"]["InstanceInterruptionBehavior"], "stop");
    }
}
