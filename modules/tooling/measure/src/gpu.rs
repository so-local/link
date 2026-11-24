//! A machine's GPUs and what each process uses of them, where a system
//! says: NVIDIA's `nvidia-smi`, the kernel's DRM files on Linux (each
//! card's in `/sys/class/drm`, each process's open GPU in
//! `/proc/<pid>/fdinfo`), and a Mac's `ioreg`. Each reader takes where
//! it reads, so tests read fakes; what's read is numbers only.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A machine's GPUs now, together.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gpus {
    /// How busy they are: each GPU's share of the time, added up (two
    /// GPUs at half are 1.0).
    pub busy: f64,
    /// Their memory in use, in bytes.
    pub memory: u64,
}

impl Gpus {
    fn plus(self, busy: f64, memory: u64) -> Self {
        Self {
            busy: self.busy + busy,
            memory: self.memory.saturating_add(memory),
        }
    }
}

/// How long `nvidia-smi` and the like may take: a driver that hangs
/// never holds up a look past it.
pub const WITHIN: Duration = Duration::from_secs(5);

/// What `program` with `args` printed, once it ended within `within`
/// (whatever its status: `lsof` that finds nothing ends with 1); none
/// when it couldn't run or took longer, when it's ended.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a child within its deadline"
)]
pub fn output(program: &Path, args: &[&str], within: Duration) -> Option<String> {
    let mut child = link_secretive::process::command(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let out = reader.join().ok()?;
                return Some(String::from_utf8_lossy(&out).into_owned());
            }
            Ok(None) if start.elapsed() < within => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// `nvidia-smi`, where NVIDIA's driver put it: on the PATH, or in
/// `/usr/bin`. Looked for once.
pub fn nvidia_smi() -> Option<&'static Path> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            let path = std::env::var_os("PATH").unwrap_or_default();
            std::env::split_paths(&path)
                .chain([PathBuf::from("/usr/bin")])
                .map(|dir| dir.join("nvidia-smi"))
                .find(|p| p.is_file())
        })
        .as_deref()
}

/// Each NVIDIA GPU's use and memory, from `nvidia-smi`.
pub fn nvidia(program: &Path) -> Option<Gpus> {
    let text = output(
        program,
        &[
            "--query-gpu=utilization.gpu,memory.used",
            "--format=csv,noheader,nounits",
        ],
        WITHIN,
    )?;
    nvidia_gpus(&text)
}

/// `nvidia-smi --query-gpu=utilization.gpu,memory.used
/// --format=csv,noheader,nounits`, read: a line a GPU, its use in
/// percent and its memory in MiB. A value it can't say (`[N/A]`) counts
/// as none; no GPU at all, none.
pub fn nvidia_gpus(text: &str) -> Option<Gpus> {
    let mut gpus: Option<Gpus> = None;
    for line in text.lines() {
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        let [busy, memory] = fields[..] else {
            continue;
        };
        let (busy, memory) = (busy.parse::<f64>().ok(), memory.parse::<u64>().ok());
        if busy.is_none() && memory.is_none() {
            continue;
        }
        let busy = busy
            .filter(|b| b.is_finite())
            .unwrap_or(0.0)
            .clamp(0.0, 100.0);
        gpus = Some(
            gpus.unwrap_or_default()
                .plus(busy / 100.0, memory.unwrap_or(0) * 1024 * 1024),
        );
    }
    gpus
}

/// The GPU memory each process uses on NVIDIA's GPUs, by pid, from
/// `nvidia-smi`: none without it.
pub fn nvidia_processes(program: &Path) -> BTreeMap<u32, u64> {
    output(
        program,
        &[
            "--query-compute-apps=pid,used_memory",
            "--format=csv,noheader,nounits",
        ],
        WITHIN,
    )
    .map(|text| nvidia_apps(&text))
    .unwrap_or_default()
}

/// `nvidia-smi --query-compute-apps=pid,used_memory
/// --format=csv,noheader,nounits`, read: a line a process on a GPU, its
/// pid and its memory there in MiB, added up across GPUs.
pub fn nvidia_apps(text: &str) -> BTreeMap<u32, u64> {
    let mut apps = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        let [pid, memory] = fields[..] else {
            continue;
        };
        if let (Ok(pid), Ok(memory)) = (pid.parse::<u32>(), memory.parse::<u64>()) {
            let sum: &mut u64 = apps.entry(pid).or_default();
            *sum = sum.saturating_add(memory * 1024 * 1024);
        }
    }
    apps
}

/// Each card in `drm` (`/sys/class/drm`) whose driver says how busy it
/// is or its memory in use (AMD's, `gpu_busy_percent` and
/// `mem_info_vram_used`): none where no card says either.
pub fn cards(drm: &Path) -> Option<Gpus> {
    let mut gpus: Option<Gpus> = None;
    for entry in std::fs::read_dir(drm).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // A card, not one of its connectors (`card0-HDMI-A-1`).
        let Some(n) = name.strip_prefix("card") else {
            continue;
        };
        if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let device = entry.path().join("device");
        let read = |file: &str| -> Option<u64> {
            std::fs::read_to_string(device.join(file))
                .ok()?
                .trim()
                .parse()
                .ok()
        };
        let (busy, memory) = (read("gpu_busy_percent"), read("mem_info_vram_used"));
        if busy.is_none() && memory.is_none() {
            continue;
        }
        let busy = busy.unwrap_or(0).min(100) as f64 / 100.0;
        gpus = Some(gpus.unwrap_or_default().plus(busy, memory.unwrap_or(0)));
    }
    gpus
}

/// A machine's GPUs, from `nvidia-smi` where it's `nvidia` and the cards
/// in `drm`: none where neither says.
pub fn machine(nvidia_smi: Option<&Path>, drm: &Path) -> Option<Gpus> {
    let found = [nvidia_smi.and_then(nvidia), cards(drm)];
    found
        .into_iter()
        .flatten()
        .reduce(|a, b| a.plus(b.busy, b.memory))
}

/// One GPU client a process has open, as its `fdinfo` says (the kernel's
/// DRM usage stats): who it is, every engine's busy time, and its
/// memory on the GPU's own (`vram`, `local`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Client {
    /// Its device and its id there: a client open on two descriptors is
    /// one.
    pub id: String,
    /// Every engine's busy time, added up, in seconds.
    pub seconds: f64,
    pub memory: u64,
}

/// A process's `/proc/<pid>/fdinfo/<fd>`, read, when it's a GPU's.
pub fn fdinfo(text: &str) -> Option<Client> {
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(key.trim(), value.trim());
        }
    }
    fields.get("drm-driver")?;
    let id = format!(
        "{}/{}",
        fields.get("drm-pdev").copied().unwrap_or(""),
        fields.get("drm-client-id")?
    );
    let mut nanos = 0u64;
    let mut resident = 0u64;
    let mut memory = 0u64;
    let mut has_resident = false;
    for (key, value) in &fields {
        if let Some(engine) = key.strip_prefix("drm-engine-") {
            if engine.starts_with("capacity-") {
                continue;
            }
            if let Some(ns) = value
                .strip_suffix("ns")
                .and_then(|n| n.trim().parse::<u64>().ok())
            {
                nanos = nanos.saturating_add(ns);
            }
        } else if let Some(region) = key.strip_prefix("drm-resident-") {
            if own(region) {
                has_resident = true;
                resident = resident.saturating_add(bytes(value).unwrap_or(0));
            }
        } else if let Some(region) = key.strip_prefix("drm-memory-")
            && own(region)
        {
            memory = memory.saturating_add(bytes(value).unwrap_or(0));
        }
    }
    Some(Client {
        id,
        seconds: nanos as f64 / 1e9,
        // The newer key where the kernel gives both for one region.
        memory: if has_resident { resident } else { memory },
    })
}

/// Whether a DRM memory region is the GPU's own memory, not the
/// machine's it maps (`gtt`, `system`, `cpu`).
fn own(region: &str) -> bool {
    region.starts_with("vram") || region.starts_with("local")
}

/// `2048 KiB`, `3 MiB`, `512`: in bytes.
fn bytes(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let n: u64 = parts.next()?.parse().ok()?;
    let scale = match parts.next() {
        None => 1,
        Some("KiB") => 1 << 10,
        Some("MiB") => 1 << 20,
        Some("GiB") => 1 << 30,
        Some(_) => return None,
    };
    Some(n.saturating_mul(scale))
}

/// What process `dir` (`/proc/<pid>`) uses of GPUs, by its open GPU
/// clients (those of its descriptors on `/dev/dri`): every engine's busy
/// time since it started, in seconds, and its memory on them. None when
/// it has no GPU open.
pub fn process(dir: &Path) -> Option<(f64, u64)> {
    let mut clients: BTreeMap<String, Client> = BTreeMap::new();
    for fd in std::fs::read_dir(dir.join("fd")).ok()?.flatten() {
        let Ok(target) = std::fs::read_link(fd.path()) else {
            continue;
        };
        if !target.starts_with("/dev/dri/") {
            continue;
        }
        let info = std::fs::read_to_string(dir.join("fdinfo").join(fd.file_name()));
        if let Some(client) = info.ok().as_deref().and_then(fdinfo) {
            clients.insert(client.id.clone(), client);
        }
    }
    (!clients.is_empty()).then(|| {
        clients.values().fold((0.0, 0u64), |(s, m), c| {
            (s + c.seconds, m.saturating_add(c.memory))
        })
    })
}

/// A Mac's GPUs, from `ioreg -r -d 1 -w 0 -c IOAccelerator`: each
/// accelerator's `PerformanceStatistics`, its `Device Utilization %`
/// and its `In use system memory`. None where none says either.
pub fn ioreg(text: &str) -> Option<Gpus> {
    let mut gpus: Option<Gpus> = None;
    for stats in text.split("\"PerformanceStatistics\" = {").skip(1) {
        let stats = stats.split('}').next().unwrap_or("");
        let value = |key: &str| -> Option<u64> {
            let rest = stats.split(&format!("\"{key}\"=")).nth(1)?;
            let end = rest.find([',', '}']).unwrap_or(rest.len());
            rest[..end].trim().parse().ok()
        };
        let (busy, memory) = (value("Device Utilization %"), value("In use system memory"));
        if busy.is_none() && memory.is_none() {
            continue;
        }
        let busy = busy.unwrap_or(0).min(100) as f64 / 100.0;
        gpus = Some(gpus.unwrap_or_default().plus(busy, memory.unwrap_or(0)));
    }
    gpus
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> link_test_kit::TempDir {
        link_test_kit::TempDir::new(&format!("measure-gpu-{name}"))
    }

    /// A fake `nvidia-smi`: what a machine with two GPUs prints, each
    /// query its own.
    fn fake_nvidia_smi(dir: &Path) -> PathBuf {
        let path = dir.join("nvidia-smi");
        // Never open for writing here, so it runs at once.
        link_files::write_program(
            &path,
            "#!/bin/sh\n\
             case \"$1\" in\n\
             --query-gpu=*) printf '45, 1024\\n15, [N/A]\\n' ;;\n\
             --query-compute-apps=*) printf '4242, 512\\n4242, 256\\n77, 100\\n' ;;\n\
             *) exit 9 ;;\n\
             esac\n",
        )
        .unwrap();
        path
    }

    #[test]
    fn nvidias_gpus_and_processes_are_read_from_nvidia_smi() {
        let dir = dir("nvidia");
        let smi = fake_nvidia_smi(&dir);
        let gpus = nvidia(&smi).unwrap();
        assert!((gpus.busy - 0.6).abs() < 1e-9, "{gpus:?}");
        assert_eq!(gpus.memory, 1024 * 1024 * 1024);
        let apps = nvidia_processes(&smi);
        assert_eq!(apps[&4242], 768 * 1024 * 1024);
        assert_eq!(apps[&77], 100 * 1024 * 1024);
        // Without a GPU, nothing: none rather than a GPU at 0.
        assert_eq!(nvidia_gpus("No devices were found\n"), None);
        assert!(nvidia_apps("").is_empty());
        // A machine with no nvidia-smi and no card that says.
        assert_eq!(machine(None, &dir.join("drm")), None);
        assert_eq!(machine(Some(&smi), &dir.join("drm")), Some(gpus));
    }

    /// A program that never ends is let go of, and the look goes on.
    #[test]
    fn a_hung_program_is_let_go_of() {
        let started = Instant::now();
        assert_eq!(
            output(Path::new("/bin/sleep"), &["30"], Duration::from_millis(200)),
            None
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// A fake `/sys/class/drm`: an AMD card that says, its connector,
    /// and a card that says nothing.
    #[test]
    fn a_cards_use_and_memory_are_read_from_sysfs() {
        let drm = dir("sysfs");
        let amd = drm.join("card0").join("device");
        std::fs::create_dir_all(&amd).unwrap();
        std::fs::write(amd.join("gpu_busy_percent"), "30\n").unwrap();
        std::fs::write(amd.join("mem_info_vram_used"), "2147483648\n").unwrap();
        std::fs::create_dir_all(drm.join("card0-HDMI-A-1").join("device")).unwrap();
        std::fs::write(
            drm.join("card0-HDMI-A-1")
                .join("device")
                .join("gpu_busy_percent"),
            "99\n",
        )
        .unwrap();
        std::fs::create_dir_all(drm.join("card1").join("device")).unwrap();
        let gpus = cards(&drm).unwrap();
        assert!((gpus.busy - 0.3).abs() < 1e-9, "{gpus:?}");
        assert_eq!(gpus.memory, 2 << 30);
        std::fs::remove_dir_all(&drm).unwrap();
        assert_eq!(cards(&drm), None);
    }

    /// A process's GPU clients, from a fake `/proc/<pid>`: one client
    /// open twice counted once, every engine's time added, only the
    /// GPU's own memory; a descriptor that isn't a GPU's, passed over.
    #[test]
    fn a_processs_gpu_is_read_from_its_fdinfo() {
        let proc = dir("fdinfo");
        std::fs::create_dir_all(proc.join("fd")).unwrap();
        std::fs::create_dir_all(proc.join("fdinfo")).unwrap();
        let amd = "pos:\t0\nflags:\t02100002\ndrm-driver:\tamdgpu\n\
                   drm-pdev:\t0000:03:00.0\ndrm-client-id:\t42\n\
                   drm-engine-gfx:\t1500000000 ns\ndrm-engine-compute:\t500000000 ns\n\
                   drm-engine-capacity-gfx:\t2\n\
                   drm-memory-vram:\t2048 KiB\ndrm-memory-gtt:\t4096 KiB\n\
                   drm-resident-vram:\t2048 KiB\n";
        let intel = "drm-driver:\ti915\ndrm-pdev:\t0000:00:02.0\ndrm-client-id:\t7\n\
                     drm-engine-render:\t250000000 ns\ndrm-total-system0:\t8 MiB\n";
        for (fd, target, info) in [
            ("3", "/dev/dri/renderD128", amd),
            ("4", "/dev/dri/renderD128", amd),
            ("5", "/dev/dri/card1", intel),
            ("6", "/tmp/not-a-gpu", amd),
        ] {
            std::os::unix::fs::symlink(target, proc.join("fd").join(fd)).unwrap();
            std::fs::write(proc.join("fdinfo").join(fd), info).unwrap();
        }
        let (seconds, memory) = process(&proc).unwrap();
        assert!((seconds - 2.25).abs() < 1e-9, "{seconds}");
        assert_eq!(memory, 2048 * 1024);
        assert_eq!(fdinfo("pos:\t0\nflags:\t02\n"), None);
        assert_eq!(bytes("3 MiB"), Some(3 << 20));
        std::fs::remove_dir_all(proc.join("fd")).unwrap();
        std::fs::create_dir_all(proc.join("fd")).unwrap();
        assert_eq!(process(&proc), None);
        std::fs::remove_dir_all(&proc).unwrap();
    }

    /// What `ioreg -r -d 1 -w 0 -c IOAccelerator` prints on a Mac with
    /// Apple's GPU, as recorded.
    #[test]
    fn a_macs_gpu_is_read_from_ioreg() {
        let text = r#"+-o AGXAcceleratorG13X  <class AGXAcceleratorG13X, id 0x1000003b3, registered, matched, active, busy 0 (2 ms), retain 85>
    {
      "IOClass" = "AGXAcceleratorG13X"
      "PerformanceStatistics" = {"In use system memory (driver)"=0,"Alloc system memory"=1610612736,"Tiler Utilization %"=3,"recoveryCount"=0,"Renderer Utilization %"=4,"Device Utilization %"=12,"SplitSceneCount"=0,"In use system memory"=402653184}
      "IOMatchCategory" = "IOAccelerator"
    }
"#;
        let gpus = ioreg(text).unwrap();
        assert!((gpus.busy - 0.12).abs() < 1e-9, "{gpus:?}");
        assert_eq!(gpus.memory, 402_653_184);
        assert_eq!(ioreg("+-o IOAccelerator\n"), None);
    }
}
