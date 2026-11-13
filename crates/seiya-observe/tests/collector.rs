use seiya_observe::*;
use serde_json::json;
use std::collections::VecDeque;

fn identity(pid: u32, start: &str) -> ProcessIdentity {
    ProcessIdentity::new(pid, start).unwrap()
}
fn target(pid: u32, role: Role) -> Target {
    Target {
        identity: identity(pid, "fixture-start"),
        role,
    }
}
fn present(pid: u32, rss_kib: u64) -> Observation {
    Observation::Present {
        identity: identity(pid, "fixture-start"),
        rss_kib,
    }
}

#[derive(Default)]
struct Fake {
    observations: VecDeque<Observation>,
    calls: Vec<ProcessIdentity>,
}
impl Fake {
    fn new(observations: Vec<Observation>) -> Self {
        Self {
            observations: observations.into(),
            calls: vec![],
        }
    }
}
impl Provider for Fake {
    fn observe(&mut self, expected: &ProcessIdentity) -> Observation {
        self.calls.push(expected.clone());
        self.observations.pop_front().expect("unexpected probe")
    }
}

#[test]
fn identity_rejects_pid_zero_empty_large_or_unprintable_markers() {
    for (pid, marker) in [
        (0, "start".into()),
        (1, "".into()),
        (1, "x".repeat(129)),
        (1, "start\nsecret".into()),
        (1, "start marker".into()),
    ] {
        assert_eq!(
            ProcessIdentity::new(pid, marker),
            Err(Error::InvalidIdentity)
        );
    }
    assert_eq!(identity(7, "synthetic-42").start_marker(), "synthetic-42");
}

#[test]
fn target_and_snapshot_limits_and_duplicate_pids_are_rejected() {
    for targets in [
        vec![],
        (1..=33).map(|pid| target(pid, Role::Runtime)).collect(),
    ] {
        assert!(matches!(
            Collector::new(targets, 1),
            Err(Error::InvalidLimits)
        ));
    }
    for count in [0, MAX_SNAPSHOTS + 1] {
        assert!(matches!(
            Collector::new(vec![target(1, Role::Runtime)], count),
            Err(Error::InvalidLimits)
        ));
    }
    assert!(matches!(
        Collector::new(
            vec![target(1, Role::Runtime), target(1, Role::Coordinator)],
            1
        ),
        Err(Error::DuplicatePid)
    ));
    assert!(Collector::new(
        (1..=32).map(|pid| target(pid, Role::Runtime)).collect(),
        MAX_SNAPSHOTS
    )
    .is_ok());
}

#[test]
fn samples_preserve_identity_role_phase_and_convert_rss_without_summing() {
    let mut collector = Collector::new(
        vec![
            target(1, Role::Runtime),
            target(2, Role::OpencodePrivateServer),
        ],
        1,
    )
    .unwrap();
    let mut fake = Fake::new(vec![present(1, 4096), present(2, 128)]);
    let sample = collector
        .collect(42, Phase::ToolRoundtrip, &mut fake)
        .unwrap();
    assert_eq!(sample.processes[0].rss.value(), Some(4194304));
    assert_eq!(sample.processes[1].rss.value(), Some(131072));
    assert_eq!(
        fake.calls,
        vec![identity(1, "fixture-start"), identity(2, "fixture-start")]
    );
    let value = serde_json::to_value(sample).unwrap();
    assert_eq!(value["elapsed_millis"], 42);
    assert_eq!(value["phase"], "tool-roundtrip");
    assert_eq!(value["processes"][1]["role"], "opencode_private_server");
    assert_eq!(
        value["processes"][0]["rss"],
        json!({"value":4194304,"unit":"bytes","scope":"process",
        "method":"rss-kib-times-1024","availability":"available","reason":null})
    );
    assert!(value.get("total_memory").is_none());
}

#[test]
fn unsupported_metrics_serialize_null_with_reason_not_zero() {
    let mut collector = Collector::new(vec![target(1, Role::Collector)], 1).unwrap();
    let mut fake = Fake::new(vec![present(1, 0)]);
    let sample = collector
        .collect(0, Phase::ListenerReady, &mut fake)
        .unwrap();
    assert_eq!(sample.processes[0].rss.value(), Some(0));
    let value = serde_json::to_value(sample).unwrap();
    for name in ["footprint", "mlx_active_memory", "metal_allocated_memory"] {
        let metric = &value["processes"][0][name];
        assert_eq!(metric["availability"], "unsupported");
        assert_eq!(metric["value"], serde_json::Value::Null);
        assert_eq!(metric["method"], serde_json::Value::Null);
        assert_eq!(metric["reason"], "no_validated_probe");
    }
}

#[test]
fn reused_pid_or_changed_pid_is_terminal_and_never_reprobed() {
    for changed in [identity(1, "other-start"), identity(2, "fixture-start")] {
        let mut collector = Collector::new(vec![target(1, Role::Runtime)], 3).unwrap();
        let mut fake = Fake::new(vec![Observation::Present {
            identity: changed,
            rss_kib: 100,
        }]);
        assert!(matches!(
            collector.collect(0, Phase::ColdText, &mut fake),
            Err(Error::IdentityChanged)
        ));
        assert!(matches!(
            collector.collect(1, Phase::ColdText, &mut fake),
            Err(Error::Failed)
        ));
        assert_eq!(fake.calls.len(), 1);
    }
}

#[test]
fn backwards_time_fails_before_probe_and_equal_milliseconds_are_allowed() {
    let mut collector = Collector::new(vec![target(1, Role::Runtime)], 4).unwrap();
    let mut fake = Fake::new(vec![present(1, 1), present(1, 2)]);
    collector.collect(10, Phase::Idle, &mut fake).unwrap();
    collector.collect(10, Phase::Resume, &mut fake).unwrap();
    assert!(matches!(
        collector.collect(9, Phase::Idle, &mut fake),
        Err(Error::ClockRegressed)
    ));
    assert!(matches!(
        collector.collect(11, Phase::Idle, &mut fake),
        Err(Error::Failed)
    ));
    assert_eq!(fake.calls.len(), 2);
}

#[test]
fn exited_process_is_not_sampled_again_and_missing_is_not_zero() {
    let mut collector = Collector::new(vec![target(1, Role::Runtime)], 2).unwrap();
    let mut fake = Fake::new(vec![Observation::Exited]);
    for time in [0, 1] {
        let sample = collector.collect(time, Phase::Shutdown, &mut fake).unwrap();
        assert_eq!(sample.processes[0].rss.value(), None);
        assert_eq!(
            sample.processes[0].rss.availability(),
            Availability::Unavailable
        );
        assert_eq!(sample.processes[0].rss.reason(), Some(Reason::Exited));
    }
    assert_eq!(fake.calls.len(), 1);
}

#[test]
fn transient_probe_errors_and_overflow_can_recover_without_false_zero() {
    let mut collector = Collector::new(vec![target(1, Role::Runtime)], 5).unwrap();
    let mut fake = Fake::new(vec![
        Observation::Unavailable(ProbeFailure::Failed),
        Observation::Unavailable(ProbeFailure::PermissionDenied),
        Observation::Unavailable(ProbeFailure::InvalidData),
        present(1, u64::MAX),
        present(1, 8),
    ]);
    for (time, reason) in [
        Reason::ProbeFailed,
        Reason::PermissionDenied,
        Reason::InvalidData,
        Reason::InvalidData,
    ]
    .into_iter()
    .enumerate()
    {
        let sample = collector
            .collect(time as u64, Phase::WarmRepeat, &mut fake)
            .unwrap();
        assert_eq!(sample.processes[0].rss.value(), None);
        assert_eq!(sample.processes[0].rss.reason(), Some(reason));
    }
    assert_eq!(
        collector
            .collect(4, Phase::Resume, &mut fake)
            .unwrap()
            .processes[0]
            .rss
            .value(),
        Some(8192)
    );
}

#[test]
fn snapshot_budget_stops_without_reading_more_or_retaining_history() {
    let mut collector = Collector::new(vec![target(1, Role::Runtime)], 1).unwrap();
    let mut fake = Fake::new(vec![present(1, 1)]);
    let sample = collector.collect(0, Phase::Idle, &mut fake).unwrap();
    drop(sample);
    assert!(matches!(
        collector.collect(1, Phase::Idle, &mut fake),
        Err(Error::SnapshotLimit)
    ));
    assert_eq!(fake.calls.len(), 1);
}
