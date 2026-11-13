use seiya_server::config::{Config, MAX_CONFIG_BYTES};

#[test]
fn explicit_loopback_config_and_ephemeral_port_are_valid() {
    for port in [0, 8012] {
        let json = format!(r#"{{"listen":"127.0.0.1:{port}"}}"#);
        let config = Config::parse(json.as_bytes()).unwrap();
        assert_eq!(config.listen.port(), port);
        assert!(config
            .resolve_api_key(|_| panic!("no env lookup expected"))
            .unwrap()
            .is_none());
    }
}

#[test]
fn non_loopback_and_missing_or_unknown_fields_are_rejected() {
    for json in [
        r#"{"listen":"0.0.0.0:8012"}"#,
        r#"{"listen":"192.0.2.1:8012"}"#,
        r#"{"listen":"[::1]:8012"}"#,
        r#"{"listen":"localhost:8012"}"#,
        r#"{"api_key_env":"SYNTHETIC_KEY"}"#,
        r#"{"listen":"127.0.0.1:8012","api_key":"do-not-embed-fixture"}"#,
        r#"{"listen":"127.0.0.1:8012","runtime":"not-supported-yet"}"#,
        r#"{"listen":"127.0.0.1:8012","listen":"127.0.0.1:8013"}"#,
    ] {
        assert!(Config::parse(json.as_bytes()).is_err());
    }
}

#[test]
fn config_size_and_parse_errors_do_not_echo_content() {
    let secret = "synthetic-secret-must-not-appear";
    let json = format!(r#"{{"listen":"127.0.0.1:8012","api_key":"{secret}"}}"#);
    let error = Config::parse(json.as_bytes()).err().unwrap();
    assert!(!error.to_string().contains(secret));
    let mut oversized = b"{}".to_vec();
    oversized.resize(MAX_CONFIG_BYTES as usize + 1, b' ');
    assert_eq!(
        Config::parse(&oversized).err().unwrap().0,
        "configuration file exceeds 65536 bytes"
    );
    assert!(Config::parse(b"\xff").is_err());
}

#[test]
fn environment_names_are_checked_without_reading_values() {
    for name in ["", "1KEY", "KEY-NAME", "KEY NAME", "../KEY", "CLAVE_ñ"] {
        let json = serde_json::json!({"listen":"127.0.0.1:8012","api_key_env":name});
        assert!(Config::parse(&serde_json::to_vec(&json).unwrap()).is_err());
    }
    let config =
        Config::parse(br#"{"listen":"127.0.0.1:8012","api_key_env":"_SEIYA_TEST_KEY_1"}"#).unwrap();
    let key = config
        .resolve_api_key(|name| {
            assert_eq!(name, "_SEIYA_TEST_KEY_1");
            Some("synthetic-key".into())
        })
        .unwrap();
    assert_eq!(key.as_deref(), Some("synthetic-key"));
}

#[test]
fn configured_key_fails_closed_if_missing_empty_or_invalid() {
    let config =
        Config::parse(br#"{"listen":"127.0.0.1:8012","api_key_env":"SEIYA_TEST_KEY"}"#).unwrap();
    assert!(config.resolve_api_key(|_| None).is_err());
    for key in ["", "contains space", "line\nbreak", "tab\tkey", "ñ", "\0"] {
        assert!(config.resolve_api_key(|_| Some(key.into())).is_err());
    }
    assert!(config.resolve_api_key(|_| Some("x".repeat(4097))).is_err());
    assert!(config.resolve_api_key(|_| Some("x".repeat(4096))).is_ok());
}

#[test]
fn runtime_is_explicit_validated_and_not_the_frontend() {
    let runtime = serde_json::json!({"id":"worker","model":"synthetic","backend":"mlx",
        "base_url":"http://127.0.0.1:8081/v1","timeout_seconds":5,"max_output_tokens":256});
    let mut value = serde_json::json!({"listen":"127.0.0.1:8012","runtime":runtime,
        "queue_wait_ms":120000,"request_read_timeout_ms":30000});
    let config = Config::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(config.runtime.is_some());
    value["runtime"]["base_url"] = "http://127.0.0.1:8012/v1".into();
    assert!(Config::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    value["runtime"]["base_url"] = "https://untrusted.invalid/v1".into();
    assert!(Config::parse(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn queue_and_body_read_deadlines_have_explicit_bounds() {
    for (field, value) in [
        ("queue_wait_ms", 0),
        ("queue_wait_ms", 120001),
        ("request_read_timeout_ms", 0),
        ("request_read_timeout_ms", 30001),
    ] {
        let mut config = serde_json::json!({"listen":"127.0.0.1:0"});
        config[field] = value.into();
        assert!(Config::parse(&serde_json::to_vec(&config).unwrap()).is_err());
    }
}
