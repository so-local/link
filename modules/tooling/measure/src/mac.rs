//! What macOS says of processes and the machine: `libproc` for each
//! process (its footprint, CPU and disk, its children), `vm_stat` for the
//! machine's memory, `host_statistics` for its CPU, `netstat` for its
//! network and the ports listened on, `lsof` for which of this user's
//! processes listen on them, and `ioreg` for its GPU. A Mac has no
//! control group: an agent is its process tree. What the commands print
//! is read in [`crate::printed`] and [`crate::gpu`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::gpu::{WITHIN, output};
use crate::printed;
use crate::sample::{Machine, Proc};

/// `mach_timebase_info`'s answer: Mach absolute time to nanoseconds.
#[repr(C)]
struct Timebase {
    numer: u32,
    denom: u32,
}

unsafe extern "C" {
    fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
    fn mach_host_self() -> libc::mach_port_t;
}

/// Nanoseconds per unit of `ri_user_time` (Mach absolute time).
fn timebase() -> f64 {
    let mut info = Timebase { numer: 0, denom: 0 };
    // SAFETY: fills the struct it's given.
    let ok = unsafe { mach_timebase_info(&mut info) } == 0;
    if ok && info.denom > 0 {
        f64::from(info.numer) / f64::from(info.denom)
    } else {
        1.0
    }
}

/// Whether process `pid` runs.
pub fn alive(pid: u32) -> bool {
    let Ok(id) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only asks whether the process is there.
    id > 0
        && (unsafe { libc::kill(id, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

/// Process `pid` now, if it runs and is this user's to read; its
/// footprint, cheap to read here, whatever `_memory` says.
pub fn process(pid: u32, _memory: bool) -> Option<Proc> {
    let id = i32::try_from(pid).ok()?;
    // SAFETY: zeroed is a valid rusage_info_v2, which the call fills.
    let mut usage: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let filled = unsafe {
        libc::proc_pid_rusage(
            id,
            libc::RUSAGE_INFO_V2,
            (&mut usage as *mut libc::rusage_info_v2).cast(),
        )
    };
    if filled != 0 {
        return None;
    }
    // SAFETY: zeroed is a valid proc_bsdinfo, which the call fills.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let got = unsafe {
        libc::proc_pidinfo(
            id,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    let parent = if got == size { info.pbi_ppid } else { 0 };
    let started =
        (got == size).then(|| info.pbi_start_tvsec * 1_000_000_000 + info.pbi_start_tvusec * 1_000);
    let mut name = [0u8; 256];
    // SAFETY: writes at most the buffer's size.
    let len = unsafe { libc::proc_name(id, name.as_mut_ptr().cast(), name.len() as u32) };
    let name = String::from_utf8_lossy(&name[..usize::try_from(len).unwrap_or(0)]).to_string();
    let nanos = (usage.ri_user_time
        + usage.ri_system_time
        + usage.ri_child_user_time
        + usage.ri_child_system_time) as f64
        * timebase();
    Some(Proc {
        pid,
        parent,
        name,
        started,
        memory: Some(usage.ri_phys_footprint),
        cpu: nanos / 1e9,
        read: usage.ri_diskio_bytesread,
        written: usage.ri_diskio_byteswritten,
    })
}

/// `pid`'s children.
pub fn children(pid: u32) -> Vec<u32> {
    let Ok(id) = i32::try_from(pid) else {
        return Vec::new();
    };
    let mut pids = vec![0i32; 1024];
    let bytes = (pids.len() * std::mem::size_of::<i32>()) as i32;
    // SAFETY: writes at most `bytes` into the buffer.
    let n = unsafe { libc::proc_listchildpids(id, pids.as_mut_ptr().cast(), bytes) };
    // It answers with a count of pids (or, on some releases, of bytes).
    let n = usize::try_from(n).unwrap_or(0).min(pids.len());
    pids.truncate(n);
    pids.into_iter()
        .filter(|p| *p > 0)
        .map(|p| p as u32)
        .collect()
}

/// The TCP ports this user's processes listen on, by pid: `lsof` run
/// once a look, when a process's are first asked for.
#[derive(Default)]
pub struct Nets(Option<BTreeMap<u32, BTreeSet<u16>>>);

/// The TCP ports process `pid` listens on.
pub fn ports(pid: u32, nets: &mut Nets) -> BTreeSet<u16> {
    let listening = nets.0.get_or_insert_with(|| {
        output(
            Path::new("/usr/sbin/lsof"),
            &["-nP", "-w", "-iTCP", "-sTCP:LISTEN", "-Fpn"],
            WITHIN,
        )
        .map(|text| printed::lsof_listening(&text))
        .unwrap_or_default()
    });
    listening.get(&pid).cloned().unwrap_or_default()
}

/// A process's GPU isn't read on a Mac: only the machine's.
pub fn gpu(_pid: u32) -> Option<(f64, u64)> {
    None
}

/// The CPU every core has used since the machine started, in
/// core-seconds.
fn cpu() -> Option<f64> {
    static HOST: std::sync::OnceLock<libc::mach_port_t> = std::sync::OnceLock::new();
    // SAFETY: a right to this host's port, asked once and kept.
    let host = *HOST.get_or_init(|| unsafe { mach_host_self() });
    let mut info = libc::host_cpu_load_info { cpu_ticks: [0; 4] };
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: fills `info`, of `count` integers.
    let read = unsafe {
        libc::host_statistics(
            host,
            libc::HOST_CPU_LOAD_INFO,
            (&mut info as *mut libc::host_cpu_load_info).cast(),
            &mut count,
        )
    };
    // SAFETY: sysconf reads a constant of the system.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    (read == 0).then(|| printed::cpu_used(info.cpu_ticks, if hz > 0 { hz as f64 } else { 100.0 }))
}

/// The whole machine now: memory in use as Activity Monitor counts it
/// (app, wired and compressed pages), CPU, processes, the ports
/// listened on, disk used, its network interfaces' bytes, and its GPU.
pub fn machine(home: &Path, _nets: &mut Nets) -> Machine {
    let memory = link_secretive::process::command("/usr/bin/vm_stat")
        .output()
        .ok()
        .and_then(|o| crate::sample::vm_stat_used(&String::from_utf8_lossy(&o.stdout)));
    let mut pids = vec![0i32; 16_384];
    let bytes = (pids.len() * std::mem::size_of::<i32>()) as i32;
    // SAFETY: writes at most `bytes` into the buffer.
    let n = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    let netstat = |args: &[&str]| output(Path::new("/usr/sbin/netstat"), args, WITHIN);
    let ioreg = output(
        Path::new("/usr/sbin/ioreg"),
        &["-r", "-d", "1", "-w", "0", "-c", "IOAccelerator"],
        WITHIN,
    );
    Machine {
        memory,
        cpu: cpu(),
        processes: u64::try_from(n).ok(),
        ports: netstat(&["-anp", "tcp"]).map(|t| printed::netstat_listening(&t).len() as u64),
        disk_used: crate::sample::filesystem_used(home),
        booted: link_measure_contract::client::booted(),
        disks: None,
        networks: netstat(&["-ibn"]).map(|t| printed::netstat_interfaces(&t)),
        gpu: ioreg.and_then(|t| crate::gpu::ioreg(&t)),
    }
}
