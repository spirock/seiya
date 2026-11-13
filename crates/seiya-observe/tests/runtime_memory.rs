//! Shared synthetic reports only. Does not import MLX or run native probes.
use seiya_observe::runtime_memory::{DecodeError, RuntimeMemoryReport, MAX_REPORT_BYTES};
use serde_json::{json, Value};

const FIXTURE: &[u8] = include_bytes!(
    "../../../contracts/benchmarks/memory-baseline/v0.1.0/runtime-memory.fixture.json"
);

fn fixture() -> Value {
    serde_json::from_slice(FIXTURE).unwrap()
}

fn decode(value: Value) -> Result<RuntimeMemoryReport, DecodeError> {
    RuntimeMemoryReport::decode(&serde_json::to_vec(&value).unwrap(), 42)
}

#[test]
fn shared_fixture_roundtrip_keeps_rss_and_metal_distinct() {
    let report = RuntimeMemoryReport::decode(FIXTURE, 42).unwrap();
    assert_eq!(report.mlx_cache_bytes.value, Some(0));
    assert_eq!(report.metal_allocated_bytes.value, None);
    assert_eq!(report.footprint_bytes.value, None);
    assert_eq!(serde_json::to_value(report).unwrap(), fixture());
    assert!(fixture().get("rss_bytes").is_none());
}

#[test]
fn rejects_bad_identity_size_json_and_unknown_fields() {
    for pid in [0, 41] {
        assert_eq!(
            RuntimeMemoryReport::decode(FIXTURE, pid),
            Err(DecodeError::IdentityMismatch)
        );
    }
    assert_eq!(RuntimeMemoryReport::decode(&[], 42), Err(DecodeError::Size));
    assert_eq!(
        RuntimeMemoryReport::decode(&vec![b' '; MAX_REPORT_BYTES + 1], 42),
        Err(DecodeError::Size)
    );
    for bytes in [b"null".as_slice(), b"{}{}", &[255]] {
        assert_eq!(
            RuntimeMemoryReport::decode(bytes, 42),
            Err(DecodeError::Json)
        );
    }
    let mut value = fixture();
    value["rss_bytes"] = json!(12);
    assert!(decode(value).is_err());
    let duplicate = String::from_utf8(FIXTURE.to_vec()).unwrap().replacen(
        "\"pid\": 42",
        "\"pid\": 42, \"pid\": 42",
        1,
    );
    assert!(RuntimeMemoryReport::decode(duplicate.as_bytes(), 42).is_err());
}

#[test]
fn rejects_bad_scope_clock_version_and_counter_shapes() {
    for (field, bad) in [
        ("version", json!("future/2")),
        ("peak_scope", json!("request")),
        ("read_finished_ns", json!(99)),
        ("sequence", json!(-1)),
        ("mlx_active_bytes", json!({"value": true, "reason": null})),
        ("mlx_active_bytes", json!({"value": -1, "reason": null})),
        ("mlx_active_bytes", json!({"value": 1.5, "reason": null})),
        ("mlx_active_bytes", json!({"value": null, "reason": null})),
        (
            "mlx_active_bytes",
            json!({"value": 1, "reason": "probe_failed"}),
        ),
        ("mlx_active_bytes", json!({"value": 1})),
        ("mlx_active_bytes", json!({"reason": "missing_api"})),
        (
            "mlx_active_bytes",
            json!({"value": null, "reason": "unknown"}),
        ),
        (
            "mlx_active_bytes",
            json!({"value": 1, "reason": null, "extra": 0}),
        ),
        ("metal_allocated_bytes", json!({"value": 1, "reason": null})),
        (
            "footprint_bytes",
            json!({"value": null, "reason": "missing_api"}),
        ),
    ] {
        let mut value = fixture();
        value[field] = bad;
        assert!(decode(value).is_err(), "{field}");
    }
    for field in fixture().as_object().unwrap().keys() {
        let mut value = fixture();
        value.as_object_mut().unwrap().remove(field);
        assert!(decode(value).is_err(), "missing {field}");
    }
}

#[test]
fn unavailable_and_max_values_remain_explicit_without_peak_comparison() {
    for reason in [
        "missing_api",
        "probe_failed",
        "invalid_value",
        "no_validated_probe",
    ] {
        let mut value = fixture();
        value["mlx_active_bytes"] = json!({"value": null, "reason": reason});
        assert!(decode(value).is_ok());
    }
    let mut value = fixture();
    value["mlx_active_bytes"]["value"] = json!(u64::MAX);
    // No false cross-counter invariant: sequential reads, externally reset peak.
    assert!(decode(value).is_ok());
}
