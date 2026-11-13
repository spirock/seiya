//! Manual calibration policy, never a production admission gate or launcher.
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};

const MAX_INPUT: u64 = 65_536;
const MAX_SWAP_GROWTH: u64 = 512 * 1024 * 1024;
const TAGS: [&str; 2] = ["gemma4:e2b-mlx", "qwen3.5:9b-mlx"];

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Check {
    Snapshot {
        pressure: Option<u32>,
        swap_bytes: Option<u64>,
        baseline_swap_bytes: u64,
    },
    E2bCalibrationSnapshot {
        pressure: Option<u32>,
        previous_pressure: u32,
        delta_ms: u64,
        warning_used_ms: u64,
        swap_bytes: Option<u64>,
        baseline_swap_bytes: u64,
    },
    QwenCalibrationSnapshot {
        pressure: Option<u32>,
        previous_pressure: u32,
        delta_ms: u64,
        warning_used_ms: u64,
        swap_bytes: Option<u64>,
        baseline_swap_bytes: u64,
    },
    QwenCriticalCalibrationSnapshot {
        pressure: Option<u32>,
        previous_pressure: u32,
        delta_ms: u64,
        high_pressure_used_ms: u64,
        swap_bytes: Option<u64>,
        baseline_swap_bytes: u64,
    },
    QwenObserveSnapshot {
        pressure: Option<u32>,
        swap_bytes: Option<u64>,
        baseline_swap_bytes: Option<u64>,
    },
    Response {
        tag: String,
        response: Value,
    },
}

fn check(input: Check) -> Value {
    let critical = matches!(&input, Check::QwenCriticalCalibrationSnapshot { .. });
    let warning_policy = match &input {
        Check::QwenCalibrationSnapshot { .. } => "ADR0041",
        Check::QwenCriticalCalibrationSnapshot { .. } => "ADR0042",
        _ => "ADR0040",
    };
    match input {
        Check::QwenObserveSnapshot {
            pressure,
            swap_bytes,
            baseline_swap_bytes,
        } => {
            json!({"continue":true,"experimental_exception":"ADR0043",
                "memory_stop_enabled":false,"not_admission":true,
                "pressure":pressure,"swap_bytes":swap_bytes,
                "swap_growth_bytes":swap_bytes.zip(baseline_swap_bytes).map(|(s,b)|s.saturating_sub(b)),
                "available_bytes":null})
        }
        Check::Snapshot {
            pressure,
            swap_bytes,
            baseline_swap_bytes,
        } => {
            let allowed = pressure == Some(1)
                && swap_bytes
                    .is_some_and(|s| s.saturating_sub(baseline_swap_bytes) <= MAX_SWAP_GROWTH);
            json!({"allowed":allowed,"experimental_exception":"ADR0039","available_bytes":null})
        }
        Check::E2bCalibrationSnapshot {
            pressure,
            previous_pressure,
            delta_ms,
            warning_used_ms,
            swap_bytes,
            baseline_swap_bytes,
        }
        | Check::QwenCalibrationSnapshot {
            pressure,
            previous_pressure,
            delta_ms,
            warning_used_ms,
            swap_bytes,
            baseline_swap_bytes,
        }
        | Check::QwenCriticalCalibrationSnapshot {
            pressure,
            previous_pressure,
            delta_ms,
            high_pressure_used_ms: warning_used_ms,
            swap_bytes,
            baseline_swap_bytes,
        } => {
            // Charge the full interval at either high-pressure endpoint.
            // State belongs to the host supervisor, never model/tool input.
            let high = |p| p == 2 || critical && p == 4;
            let known = |p| p == 1 || high(p);
            let used = warning_used_ms.checked_add(
                if high(previous_pressure) || pressure.is_some_and(high) {
                    delta_ms
                } else {
                    0
                },
            );
            let allowed = pressure.is_some_and(known)
                && known(previous_pressure)
                && delta_ms <= 1000
                && used.is_some_and(|n| n <= 10_000)
                && swap_bytes
                    .is_some_and(|s| s.saturating_sub(baseline_swap_bytes) <= MAX_SWAP_GROWTH);
            let mut result = json!({"allowed":allowed,"experimental_exception":warning_policy,
                "available_bytes":null});
            result[if critical {
                "high_pressure_used_ms"
            } else {
                "warning_used_ms"
            }] = json!(used);
            result
        }
        Check::Response { tag, response: r } => {
            let m = &r["message"];
            let valid = TAGS.contains(&tag.as_str())
                && r["model"].as_str() == Some(tag.as_str())
                && r["done"].as_bool() == Some(true)
                && r["done_reason"].as_str() == Some("stop")
                && r.get("error").is_none()
                && m["role"].as_str() == Some("assistant")
                && m["content"]
                    .as_str()
                    .is_some_and(|s| s.trim() == "SEIYA_OK")
                && m.get("thinking").is_none_or(|v| v.as_str() == Some(""))
                && m.get("tool_calls")
                    .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
                && r["eval_count"]
                    .as_u64()
                    .is_some_and(|n| (1..=256).contains(&n))
                && r["prompt_eval_count"]
                    .as_u64()
                    .is_some_and(|n| (1..=4096).contains(&n))
                && [
                    "total_duration",
                    "load_duration",
                    "prompt_eval_duration",
                    "eval_duration",
                ]
                .iter()
                .all(|k| r[*k].as_u64().is_some());
            json!({"valid":valid,"model":tag,"effective_context_validated":false,
                "tools_validated":false,"sustained_session_validated":false})
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err("input limit".into());
    }
    let result = check(serde_json::from_slice(&bytes)?);
    serde_json::to_writer(std::io::stdout(), &result)?;
    std::io::stdout().flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_unknown_swap_growth_and_overflow_fail_closed() {
        for (pressure, swap, baseline, expected) in [
            (Some(1), Some(0), 0, true),
            (Some(1), Some(MAX_SWAP_GROWTH), 0, true),
            (Some(1), Some(MAX_SWAP_GROWTH + 1), 0, false),
            (Some(2), Some(0), 0, false),
            (Some(4), Some(0), 0, false),
            (None, Some(0), 0, false),
            (Some(1), None, 0, false),
            (Some(1), Some(u64::MAX), 0, false),
            (Some(1), Some(u64::MAX), u64::MAX, true),
        ] {
            assert_eq!(
                check(Check::Snapshot {
                    pressure,
                    swap_bytes: swap,
                    baseline_swap_bytes: baseline
                })["allowed"],
                expected
            );
        }
    }

    fn response() -> Value {
        json!({"model":TAGS[0],"done":true,"done_reason":"stop",
            "message":{"role":"assistant","content":"SEIYA_OK"},
            "eval_count":4,"prompt_eval_count":20,"total_duration":12,
            "load_duration":4,"prompt_eval_duration":3,"eval_duration":5})
    }

    #[test]
    fn response_requires_identity_usage_stop_and_no_reasoning_or_tools() {
        let validate = |r| {
            check(Check::Response {
                tag: TAGS[0].into(),
                response: r,
            })["valid"]
                .as_bool()
                .unwrap()
        };
        assert!(validate(response()));
        for (field, value) in [
            ("model", json!(TAGS[1])),
            ("done", json!(false)),
            ("done_reason", json!("length")),
            ("eval_count", json!(257)),
            ("eval_count", json!(0)),
            ("prompt_eval_count", json!(4097)),
            ("load_duration", Value::Null),
            ("error", json!("failed")),
        ] {
            let mut r = response();
            r[field] = value;
            assert!(!validate(r), "{field}");
        }
        for (field, value) in [
            ("content", json!("<think>x</think>SEIYA_OK")),
            ("thinking", json!("private")),
            ("tool_calls", json!([{}])),
            ("role", json!("user")),
        ] {
            let mut r = response();
            r["message"][field] = value;
            assert!(!validate(r));
        }
        assert!(!check(Check::Response {
            tag: "unknown".into(),
            response: response()
        })["valid"]
            .as_bool()
            .unwrap());
    }

    #[test]
    fn snapshot_has_no_synthetic_available_memory_or_hidden_overrides() {
        assert!(
            serde_json::from_value::<Check>(json!({"operation":"snapshot","pressure":1,
            "swap_bytes":0,"baseline_swap_bytes":0,"skip_guard":true}))
            .is_err()
        );
        assert!(check(Check::Snapshot {
            pressure: Some(1),
            swap_bytes: Some(0),
            baseline_swap_bytes: 0
        })["available_bytes"]
            .is_null());
    }

    fn calibration(
        pressure: Option<u32>,
        previous_pressure: u32,
        delta_ms: u64,
        warning_used_ms: u64,
    ) -> Value {
        check(Check::E2bCalibrationSnapshot {
            pressure,
            previous_pressure,
            delta_ms,
            warning_used_ms,
            swap_bytes: Some(0),
            baseline_swap_bytes: 0,
        })
    }

    #[test]
    fn calibration_counts_warning_across_normal_transitions_without_reset() {
        let first = calibration(Some(2), 1, 250, 0);
        assert_eq!(first["warning_used_ms"], 250);
        assert_eq!(first["allowed"], true);
        let next = calibration(Some(1), 2, 300, 250);
        assert_eq!(next["warning_used_ms"], 550);
        assert_eq!(calibration(Some(1), 1, 1000, 550)["warning_used_ms"], 550);
        assert_eq!(calibration(Some(2), 1, 1000, 9000)["allowed"], true);
        assert_eq!(calibration(Some(2), 2, 1, 10000)["allowed"], false);
        assert_eq!(calibration(Some(1), 1, 0, 10001)["allowed"], false);
    }

    #[test]
    fn calibration_critical_missing_stale_overflow_and_swap_fail_closed() {
        for pressure in [None, Some(0), Some(3), Some(4)] {
            assert_eq!(calibration(pressure, 1, 250, 0)["allowed"], false);
        }
        assert_eq!(calibration(Some(1), 4, 250, 0)["allowed"], false);
        assert_eq!(calibration(Some(1), 1, 1001, 0)["allowed"], false);
        assert_eq!(calibration(Some(2), 1, 1, u64::MAX)["allowed"], false);
        for swap_bytes in [None, Some(MAX_SWAP_GROWTH + 1)] {
            assert_eq!(
                check(Check::E2bCalibrationSnapshot {
                    pressure: Some(1),
                    previous_pressure: 1,
                    delta_ms: 250,
                    warning_used_ms: 0,
                    swap_bytes,
                    baseline_swap_bytes: 0
                })["allowed"],
                false
            );
        }
    }

    #[test]
    fn qwen_has_a_distinct_policy_identity_and_identical_closed_limits() {
        for (pressure, previous, delta, used, swap) in [
            (Some(2), 1, 250, 0, Some(0)),
            (Some(1), 2, 250, 9750, Some(0)),
            (Some(2), 2, 1, 10000, Some(0)),
            (Some(4), 1, 250, 0, Some(0)),
            (None, 1, 250, 0, Some(0)),
            (Some(1), 1, 1001, 0, Some(0)),
            (Some(2), 1, 1, u64::MAX, Some(0)),
            (Some(1), 1, 250, 0, None),
            (Some(1), 1, 250, 0, Some(MAX_SWAP_GROWTH + 1)),
        ] {
            let qwen = check(Check::QwenCalibrationSnapshot {
                pressure,
                previous_pressure: previous,
                delta_ms: delta,
                warning_used_ms: used,
                swap_bytes: swap,
                baseline_swap_bytes: 0,
            });
            let gemma = check(Check::E2bCalibrationSnapshot {
                pressure,
                previous_pressure: previous,
                delta_ms: delta,
                warning_used_ms: used,
                swap_bytes: swap,
                baseline_swap_bytes: 0,
            });
            assert_eq!(qwen["experimental_exception"], "ADR0041");
            assert_eq!(gemma["experimental_exception"], "ADR0040");
            assert_eq!(qwen["allowed"], gemma["allowed"]);
            assert_eq!(qwen["warning_used_ms"], gemma["warning_used_ms"]);
        }
    }

    #[test]
    fn critical_is_bounded_and_shares_warning_budget_only_in_adr0042() {
        let call = |p, prev, delta, used, swap| {
            check(Check::QwenCriticalCalibrationSnapshot {
                pressure: p,
                previous_pressure: prev,
                delta_ms: delta,
                high_pressure_used_ms: used,
                swap_bytes: swap,
                baseline_swap_bytes: 0,
            })
        };
        let r = call(Some(4), 2, 300, 7000, Some(0));
        assert_eq!(r["experimental_exception"], "ADR0042");
        assert_eq!(r["allowed"], true);
        assert_eq!(r["high_pressure_used_ms"], 7300);
        assert!(r.get("warning_used_ms").is_none());
        assert_eq!(
            call(Some(2), 4, 300, 7300, Some(0))["high_pressure_used_ms"],
            7600
        );
        assert_eq!(
            call(Some(1), 4, 300, 7600, Some(0))["high_pressure_used_ms"],
            7900
        );
        assert_eq!(
            call(Some(1), 1, 300, 7900, Some(0))["high_pressure_used_ms"],
            7900
        );
        assert_eq!(call(Some(4), 4, 1000, 9000, Some(0))["allowed"], true);
        assert_eq!(call(Some(4), 4, 1, 10000, Some(0))["allowed"], false);
        for (p, prev, delta, used, swap) in [
            (None, 1, 250, 0, Some(0)),
            (Some(3), 1, 250, 0, Some(0)),
            (Some(4), 3, 250, 0, Some(0)),
            (Some(4), 4, 1001, 0, Some(0)),
            (Some(4), 4, 1, u64::MAX, Some(0)),
            (Some(4), 4, 250, 0, None),
            (Some(4), 4, 250, 0, Some(MAX_SWAP_GROWTH + 1)),
        ] {
            assert_eq!(call(p, prev, delta, used, swap)["allowed"], false);
        }
        // Existing policies still reject critical, irrespective of budget left.
        assert_eq!(calibration(Some(4), 1, 250, 0)["allowed"], false);
        assert_eq!(
            check(Check::QwenCalibrationSnapshot {
                pressure: Some(4),
                previous_pressure: 1,
                delta_ms: 250,
                warning_used_ms: 0,
                swap_bytes: Some(0),
                baseline_swap_bytes: 0
            })["allowed"],
            false
        );
    }

    #[test]
    fn observe_mode_never_turns_memory_metrics_into_an_admission_or_stop() {
        for (pressure, swap, baseline) in [
            (Some(1), Some(0), Some(0)),
            (Some(2), Some(MAX_SWAP_GROWTH + 1), Some(0)),
            (Some(4), Some(u64::MAX), Some(0)),
            (None, None, None),
            (Some(999), Some(10), None),
        ] {
            let r = check(Check::QwenObserveSnapshot {
                pressure,
                swap_bytes: swap,
                baseline_swap_bytes: baseline,
            });
            assert_eq!(r["continue"], true);
            assert_eq!(r["memory_stop_enabled"], false);
            assert_eq!(r["not_admission"], true);
            assert_eq!(r["experimental_exception"], "ADR0043");
            assert!(r.get("allowed").is_none());
            assert!(r["available_bytes"].is_null());
        }
        assert_eq!(
            check(Check::Snapshot {
                pressure: Some(4),
                swap_bytes: Some(0),
                baseline_swap_bytes: 0
            })["allowed"],
            false
        );
    }
}
