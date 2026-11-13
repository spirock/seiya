//! Fixed, read-only macOS stop-only pressure input. Never capacity/admission.
#[cfg(any(target_os = "macos", test))]
use crate::campaign::Pressure;
use crate::campaign::PressureSample;
use crate::session_supervisor::OwnerError;

#[cfg(any(target_os = "macos", test))]
fn decode(
    result: Result<(i32, usize), OwnerError>,
    sampled_at_millis: u64,
) -> Result<PressureSample, OwnerError> {
    let (value, length) = result?;
    if length != std::mem::size_of::<i32>() {
        return Err(OwnerError::Failed);
    }
    Ok(PressureSample {
        sampled_at_millis,
        pressure: match value {
            1 => Pressure::Normal,
            2 => Pressure::Warning,
            4 => Pressure::Critical,
            _ => Pressure::Unknown,
        },
    })
}

/// Timestamp must come from the consuming guard before the read, not wall time.
/// Fixed synchronous kernel query; no shell/helper to orphan on cancellation.
#[cfg(target_os = "macos")]
pub fn sample(sampled_at_millis: u64) -> Result<PressureSample, OwnerError> {
    let mut value = 0i32;
    let mut length = std::mem::size_of_val(&value);
    // SAFETY: fixed NUL-terminated OS selector, aligned writable i32/size buffers;
    // null newp/newlen means read-only, never a sysctl configuration change.
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.memorystatus_vm_pressure_level".as_ptr(),
            (&mut value as *mut i32).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    decode(
        if result == 0 {
            Ok((value, length))
        } else {
            Err(OwnerError::Failed)
        },
        sampled_at_millis,
    )
}

#[cfg(not(target_os = "macos"))]
pub fn sample(_: u64) -> Result<PressureSample, OwnerError> {
    Err(OwnerError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        campaign::MonitorStop,
        session_guard::{Block, Clock, MemoryGuard},
    };
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };
    struct Manual(AtomicU64);
    impl Clock for Manual {
        fn now_millis(&self) -> Option<u64> {
            Some(self.0.load(Ordering::SeqCst))
        }
    }

    #[test]
    fn exact_integer_values_unknown_and_syscall_failures_are_not_normalized() {
        for (value, expected) in [
            (1, Pressure::Normal),
            (2, Pressure::Warning),
            (4, Pressure::Critical),
            (0, Pressure::Unknown),
            (3, Pressure::Unknown),
            (-1, Pressure::Unknown),
        ] {
            let reading = decode(Ok((value, 4)), 123).unwrap();
            assert_eq!(reading.pressure, expected);
            assert_eq!(reading.sampled_at_millis, 123);
        }
        for length in [0, 1, 8, usize::MAX] {
            assert!(decode(Ok((1, length)), 0).is_err());
        }
        assert!(decode(Err(OwnerError::Failed), 0).is_err());
    }
    #[test]
    fn pre_read_clock_cannot_rejuvenate_slow_normal_sample_or_rearm_failure() {
        let clock = Arc::new(Manual(AtomicU64::new(10)));
        let guard = MemoryGuard::with_clock(clock.clone());
        let before = guard.now_millis().unwrap();
        clock.0.store(2011, Ordering::SeqCst);
        let sample = decode(Ok((1, 4)), before).unwrap();
        assert_eq!(
            guard.observe(Ok(sample)),
            Err(Block::Stopped(MonitorStop::StaleSample))
        );
        assert!(guard
            .observe(Ok(decode(Ok((1, 4)), 2011).unwrap()))
            .is_err());
    }
}
