//! Exploratory aggregation of already validated runner evidence, not a second
//! ownership/authentication validator. Never sums overlapping process counters.
use crate::runner::{Report, RunError};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn range(mut values: Vec<u64>) -> Value {
    values.sort_unstable();
    let len = values.len();
    let median = values[(len - 1) / 2] + (values[len / 2] - values[(len - 1) / 2]) / 2;
    json!({"min":values[0],"median":median,"max":values[len-1],"samples":len})
}

/// Caller must enforce the on-disk 32MiB artifact cap before parsing JSON.
/// Partial campaigns are explicitly non-comparable; source events stay intact.
pub fn summarize(report: &Report, events: &[Value]) -> Result<Value, RunError> {
    if !matches!(report.mode, "model-campaign" | "campaign-rehearsal") || events.len() > 10_000 {
        return Err(RunError::InvalidEvidence);
    }
    let stops: Vec<_> = events.iter().filter(|e| e["event"] == "run_stop").collect();
    if !report.completed {
        return Ok(json!({"version":1,"mode":report.mode,"comparable":false,
            "reason":"incomplete-campaign","runs_completed":report.runs_completed,
            "error":report.error,"cleanup_confirmed":report.cleanup_confirmed,
            "run_outcomes":stops.iter().map(|e| json!({"run":e["run"],"arm":e["arm"],"error":e["error"],"cleanup_confirmed":e["confirmed"]})).collect::<Vec<_>>()}));
    }
    if report.runs_completed != 12
        || !report.cleanup_confirmed
        || report.error.is_some()
        || stops.len() != 12
    {
        return Err(RunError::InvalidEvidence);
    }
    let phases = [
        ("listener-ready", 0),
        ("cold-text", 1),
        ("tool-roundtrip", 2),
        ("warm-repeat", 3),
        ("idle", 0),
        ("resume", 1),
        ("shutdown", 0),
    ];
    let order = ["python", "rust", "rust", "python"].repeat(3);
    let mut runs = Vec::new();
    for (index, arm) in order.iter().enumerate() {
        let stop = stops[index];
        if stop["run"] != index
            || stop["arm"] != *arm
            || stop["confirmed"] != true
            || !stop["error"].is_null()
        {
            return Err(RunError::InvalidEvidence);
        }
        let evidence: Vec<_> = events
            .iter()
            .filter(|e| e["event"] == "phase" && e["run"] == index)
            .collect();
        if evidence.len() != phases.len() {
            return Err(RunError::InvalidEvidence);
        }
        let mut rss: BTreeMap<String, u64> = BTreeMap::new();
        let mut timings = Vec::new();
        let mut request_sum = 0u64;
        for (e, (phase, count)) in evidence.iter().zip(phases) {
            if e["source"] != report.mode
                || e["arm"] != *arm
                || e["phase"] != phase
                || e["requests"].as_array().is_none_or(|v| v.len() != count)
                || e["results"].as_array().is_none_or(|v| v.len() != count)
            {
                return Err(RunError::InvalidEvidence);
            }
            let snapshots = e["snapshots"]
                .as_array()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or(RunError::InvalidEvidence)?;
            for snapshot in snapshots {
                let processes = snapshot["processes"]
                    .as_array()
                    .ok_or(RunError::InvalidEvidence)?;
                for role in ["runtime", "coordinator", "collector"] {
                    let process = processes
                        .iter()
                        .find(|p| p["role"] == role)
                        .ok_or(RunError::InvalidEvidence)?;
                    let bytes = process["rss"]["value"]
                        .as_u64()
                        .ok_or(RunError::InvalidEvidence)?;
                    rss.entry(role.into())
                        .and_modify(|v| *v = (*v).max(bytes))
                        .or_insert(bytes);
                }
            }
            for result in e["results"].as_array().unwrap() {
                let millis = result["timing"]["request_millis"]
                    .as_u64()
                    .ok_or(RunError::InvalidEvidence)?;
                request_sum = request_sum.checked_add(millis).ok_or(RunError::Limit)?;
                timings
                    .push(json!({"phase":phase,"request_millis":millis,"usage":result["usage"]}));
            }
        }
        runs.push(json!({"run":index,"arm":arm,"sampled_rss_max_bytes":rss,
            "request_sum_millis":request_sum,"requests":timings}));
    }
    let mut arms = BTreeMap::new();
    for arm in ["python", "rust"] {
        let selected: Vec<_> = runs.iter().filter(|r| r["arm"] == arm).collect();
        let mut rss = BTreeMap::new();
        for role in ["runtime", "coordinator", "collector"] {
            rss.insert(
                role,
                range(
                    selected
                        .iter()
                        .map(|r| r["sampled_rss_max_bytes"][role].as_u64().unwrap())
                        .collect(),
                ),
            );
        }
        arms.insert(arm, json!({"sampled_rss_max_bytes":rss,
            "request_sum_millis":range(selected.iter().map(|r| r["request_sum_millis"].as_u64().unwrap()).collect())}));
    }
    Ok(
        json!({"version":1,"mode":report.mode,"comparable":true,"model_inference":report.mode == "model-campaign",
        "scope":"exploratory-six-runs-per-arm-not-physical-memory-or-speedup-proof",
        "sums_overlapping_memory":false,"ttft_millis":null,"robust_p95":null,
        "runs":runs,"arms":arms}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Report, Vec<Value>) {
        let report = Report {
            mode: "campaign-rehearsal",
            completed: true,
            runs_completed: 12,
            error: None,
            cleanup_confirmed: true,
        };
        let mut events = Vec::new();
        for (run, arm) in ["python", "rust", "rust", "python"]
            .repeat(3)
            .iter()
            .enumerate()
        {
            for (phase, count) in [
                ("listener-ready", 0),
                ("cold-text", 1),
                ("tool-roundtrip", 2),
                ("warm-repeat", 3),
                ("idle", 0),
                ("resume", 1),
                ("shutdown", 0),
            ] {
                events.push(json!({"event":"phase","run":run,"arm":arm,"phase":phase,"source":report.mode,
                    "requests":vec![json!({});count],"results":vec![json!({"timing":{"request_millis":10},"usage":null});count],
                    "snapshots":[{"processes":[{"role":"runtime","rss":{"value":100}}, {"role":"coordinator","rss":{"value":20}},{"role":"collector","rss":{"value":30}}]}]}));
            }
            events.push(
                json!({"event":"run_stop","run":run,"arm":arm,"confirmed":true,"error":null}),
            );
        }
        (report, events)
    }

    #[test]
    fn exploratory_ranges_keep_every_run_and_role_separate() {
        let (report, events) = fixture();
        let value = summarize(&report, &events).unwrap();
        assert_eq!(value["comparable"], true);
        assert_eq!(value["model_inference"], false);
        assert_eq!(value["runs"].as_array().unwrap().len(), 12);
        assert_eq!(value["arms"]["rust"]["request_sum_millis"]["median"], 70);
        assert_eq!(
            value["arms"]["python"]["sampled_rss_max_bytes"]["runtime"]["max"],
            100
        );
        assert_eq!(value["sums_overlapping_memory"], false);
        assert!(value["robust_p95"].is_null());
    }

    #[test]
    fn partial_is_not_comparable_and_calibration_is_not_abba() {
        let (mut report, events) = fixture();
        report.completed = false;
        report.runs_completed = 0;
        report.error = Some(RunError::PreflightBlocked);
        assert_eq!(summarize(&report, &events).unwrap()["comparable"], false);
        report.mode = "model-calibration";
        assert!(summarize(&report, &events).is_err());
    }

    #[test]
    fn missing_timing_rss_phase_and_bad_order_fail_closed() {
        let (report, events) = fixture();
        for kind in 0..4 {
            let mut bad = events.clone();
            match kind {
                0 => bad[1]["results"][0]["timing"] = Value::Null,
                1 => bad[0]["snapshots"][0]["processes"][0]["rss"]["value"] = Value::Null,
                2 => {
                    bad.remove(0);
                }
                _ => bad[7]["arm"] = json!("rust"),
            }
            assert!(summarize(&report, &bad).is_err());
        }
    }
}
