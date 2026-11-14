//! Read-only OS resource bounds, sampled only for explicitly requested work.
pub(crate) trait Resources: Send + Sync {
    fn admits(&self, minimum_mib: u64) -> bool;
}
pub(crate) struct Native;
impl Resources for Native {
    fn admits(&self, minimum_mib: u64) -> bool {
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|s| linux_available(&s))
                .is_some_and(|n| n >= minimum_mib.saturating_mul(1024 * 1024))
        }
        #[cfg(target_os = "windows")]
        {
            windows_available()
                .is_some_and(|(load, n)| load < 95 && n >= minimum_mib.saturating_mul(1024 * 1024))
        }
        #[cfg(target_os = "macos")]
        {
            let _ = minimum_mib;
            seiya_observe::image::native::memory()["pressure"] == 1
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let _ = minimum_mib;
            false
        }
    }
}
#[cfg(any(test, target_os = "linux"))]
fn linux_available(text: &str) -> Option<u64> {
    if text.len() > 65536 {
        return None;
    }
    let rows: Vec<_> = text
        .lines()
        .filter(|s| s.starts_with("MemAvailable:"))
        .collect();
    if rows.len() != 1 {
        return None;
    }
    let fields: Vec<_> = rows[0].split_whitespace().collect();
    if fields.len() != 3 || fields[2] != "kB" {
        return None;
    }
    fields[1].parse::<u64>().ok()?.checked_mul(1024)
}
#[cfg(target_os = "windows")]
fn windows_available() -> Option<(u32, u64)> {
    #[repr(C)]
    struct Memory {
        length: u32,
        load: u32,
        total_physical: u64,
        available_physical: u64,
        total_page_file: u64,
        available_page_file: u64,
        total_virtual: u64,
        available_virtual: u64,
        available_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(memory: *mut Memory) -> i32;
    }
    let mut memory = Memory {
        length: std::mem::size_of::<Memory>() as u32,
        load: 100,
        total_physical: 0,
        available_physical: 0,
        total_page_file: 0,
        available_page_file: 0,
        total_virtual: 0,
        available_virtual: 0,
        available_extended_virtual: 0,
    };
    // SAFETY: exact repr(C) MEMORYSTATUSEX with initialized length, writable for
    // the synchronous kernel call. No handles/processes or personal data accessed.
    if unsafe { GlobalMemoryStatusEx(&mut memory) } == 0 {
        return None;
    }
    if memory.load > 100 || memory.available_physical > memory.total_physical {
        return None;
    }
    Some((memory.load, memory.available_physical))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_fixtures_reject_unknown_duplicate_or_overflow() {
        assert_eq!(
            linux_available("MemAvailable: 2097152 kB\n"),
            Some(2 * 1024 * 1024 * 1024)
        );
        for s in [
            "MemFree: 1000 kB",
            "MemAvailable: 1 kB\nMemAvailable: 2 kB",
            "MemAvailable: -1 kB",
            "MemAvailable: 18446744073709551615 kB",
            "MemAvailable: 5 MB",
        ] {
            assert_eq!(linux_available(s), None);
        }
    }
}
