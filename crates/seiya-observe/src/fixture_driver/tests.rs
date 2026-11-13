use super::*;

#[test]
fn hashes_must_be_actual_digest_shape_not_placeholder_or_multiline_data() {
    let valid = format!("{}  fixture.bin\n", "0123456789abcdef".repeat(4));
    assert_eq!(
        process::parse_hash(valid.as_bytes()).unwrap(),
        "0123456789abcdef".repeat(4)
    );
    for text in ["<hash>", "private-data", "ABCDEF", "0123\n0123", ""] {
        assert!(process::parse_hash(text.as_bytes()).is_err());
    }
}

#[test]
fn no_model_paths_or_live_endpoints_are_present_in_client_payload() {
    let body = request(json!([{"role":"user","content":"synthetic"}]));
    assert_eq!(body["max_tokens"], 256);
    assert_eq!(body["stream"], true);
    assert_eq!(body.as_object().unwrap().len(), 5);
    assert!(body.get("base_url").is_none());
    assert!(body.get("model_path").is_none());
}

#[test]
fn reserved_ports_are_owned_ephemeral_listeners_not_operational_ports() {
    // Only a synthetic ephemeral socket, no host process query or child launch.
    let (listener, port) = process::reserve_port().unwrap();
    assert_eq!(listener.local_addr().unwrap().port(), port);
    assert!(![8010, 8012, 8081].contains(&port));
}
