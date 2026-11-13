//! These tests read shared fixtures; they do NOT test a Rust HTTP server.
use seiya_contracts::SessionRequest;
use serde_json::Value;
use std::collections::HashSet;

const SCENARIOS: &str = include_str!("../../../contracts/openai-compat/v0.1.0/scenarios.json");
const VALIDATION: &str = include_str!("../../../contracts/openai-compat/v0.1.0/validation.json");
const HTTP_POLICY: &str = include_str!("../../../contracts/openai-compat/v0.1.0/http-policy.json");

#[test]
fn shared_openai_fixture_documents_are_readable_and_distinct_from_native_requests() {
    let document: Value = serde_json::from_str(SCENARIOS).unwrap();
    assert_eq!(document["fixture_version"], 1);
    assert_eq!(document["reference_version"], "0.1.0");
    assert_eq!(document["synthetic"], true);
    let mut ids = HashSet::new();
    let mut found_tool_roundtrip = false;
    let mut found_sse = false;
    for scenario in document["scenarios"].as_array().unwrap() {
        assert!(ids.insert(scenario["id"].as_str().unwrap()));
        for step in scenario["steps"].as_array().unwrap() {
            assert!(step["path"].as_str().unwrap().starts_with('/'));
            let status = step["status"].as_u64().unwrap();
            assert!((200..600).contains(&status));
            if let Some(expected_sse) = step.get("expected_sse") {
                assert_eq!(expected_sse.as_array().unwrap().last().unwrap(), "[DONE]");
                found_sse = true;
            } else {
                assert!(step["expected"].is_object());
            }
            if let Some(body) = step.get("body") {
                assert!(SessionRequest::parse(&serde_json::to_vec(body).unwrap()).is_err());
                if let Some(messages) = body["messages"].as_array() {
                    found_tool_roundtrip |= messages.iter().any(|m| m["role"] == "tool");
                }
            }
        }
    }
    assert!(found_tool_roundtrip);
    assert!(found_sse);
}

#[test]
fn shared_validation_fixtures_preserve_types_limits_and_error_classes() {
    let validation: Value = serde_json::from_str(VALIDATION).unwrap();
    let policy: Value = serde_json::from_str(HTTP_POLICY).unwrap();
    assert_eq!(validation["fixture_version"], 1);
    assert_eq!(policy["fixture_version"], 1);
    assert_eq!(validation["limits"]["body_bytes"], 262144);
    assert_eq!(validation["limits"]["messages"], 100);
    assert_eq!(validation["limits"]["tools"], 64);
    let invalid = validation["invalid_requests"].as_array().unwrap();
    assert!(invalid
        .iter()
        .any(|case| case["patch"]["max_tokens"].is_boolean()));
    assert!(invalid
        .iter()
        .any(|case| case["patch"]["stream"].is_string()));
    let mut ids = HashSet::new();
    for case in invalid {
        assert!(ids.insert(case["id"].as_str().unwrap()));
        assert!(case["error"].is_string());
        assert!(case.get("patch").is_some() ^ case.get("body").is_some());
    }
    for case in policy["cases"].as_array().unwrap() {
        assert!([400, 401, 403, 404].contains(&case["status"].as_u64().unwrap()));
        assert!(case["headers"].is_object());
        assert!(case["error"].is_string());
    }
}
