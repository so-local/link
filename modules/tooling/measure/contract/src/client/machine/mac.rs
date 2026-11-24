//! macOS capacity from sysctl. Unreported clocks and available memory
//! stay unknown rather than being inferred from the model of the machine.

use super::{Cpu, Memory};

fn bytes(name: &std::ffi::CStr) -> Option<Vec<u8>> {
    // SAFETY: sysctlbyname first reports the size, then fills that buffer.
    unsafe {
        let mut size = 0;
        if libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }
        let mut buffer = vec![0u8; size];
        (libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        ) == 0)
            .then(|| {
                buffer.truncate(size);
                buffer
            })
    }
}

fn number(name: &std::ffi::CStr) -> Option<u64> {
    let buffer = bytes(name)?;
    match buffer.len() {
        4 => Some(u32::from_ne_bytes(buffer.try_into().ok()?) as u64),
        8 => Some(u64::from_ne_bytes(buffer.try_into().ok()?)),
        _ => None,
    }
}

pub(super) fn capture() -> (Cpu, Memory) {
    let model = bytes(c"machdep.cpu.brand_string")
        .or_else(|| bytes(c"hw.model"))
        .map(|s| {
            String::from_utf8_lossy(&s)
                .trim_end_matches('\0')
                .to_owned()
        });
    let parallelism = std::thread::available_parallelism()
        .ok()
        .map(|n| n.get() as u64);
    (
        Cpu {
            model,
            logical_cores: number(c"hw.logicalcpu"),
            physical_cores: number(c"hw.physicalcpu"),
            parallelism_cores: parallelism,
            available_cores: parallelism.map(|n| n as f64),
            base_hz: number(c"hw.cpufrequency"),
            ..Cpu::default()
        },
        Memory {
            machine_total_bytes: number(c"hw.memsize"),
            ..Memory::default()
        },
    )
}
