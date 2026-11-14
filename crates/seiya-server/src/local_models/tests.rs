use super::*;
#[test]
fn image_catalog_is_opt_in_strict_and_requires_a_valid_tool_capable_brain() {
    let documented = Catalog::parse(include_bytes!(
        "../../../../contracts/local-models/v1/images.example.json"
    ))
    .unwrap();
    assert!(documented.image_chat);
    assert_eq!(documented.image.as_deref(), Some("image-local"));
    let mut value = example();
    let mut runtime = value["models"][0]["runtime"].clone();
    runtime["id"] = json!("image-a");
    runtime["model"] = json!("arbitrary-image-binding");
    runtime["base_url"] = json!("http://127.0.0.1:21005/v1");
    value["image_models"] = json!([{"id":"image-a","runtime":runtime}]);
    value["image"] = json!("image-a");
    assert!(Catalog::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
    value["image_chat"] = json!(true);
    assert!(Catalog::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    value["models"][0]["supports_tools"] = json!(true);
    assert!(Catalog::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
    for (key, bad) in [
        ("image", json!("not-registered")),
        ("image_models", json!([])),
    ] {
        let mut broken = value.clone();
        broken[key] = bad;
        assert!(Catalog::parse(&serde_json::to_vec(&broken).unwrap()).is_err());
    }
    let mut duplicate = value.clone();
    duplicate["image_models"][0]["id"] = value["models"][0]["id"].clone();
    assert!(Catalog::parse(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    let mut remote = value.clone();
    remote["image_models"][0]["runtime"]["base_url"] = json!("https://remote.invalid/v1");
    assert!(Catalog::parse(&serde_json::to_vec(&remote).unwrap()).is_err());
    let old = Catalog::parse(&serde_json::to_vec(&example()).unwrap()).unwrap();
    assert!(old.image_models.is_empty());
    assert!(!old.image_chat);
}
use serde_json::json;
fn example() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../../contracts/local-models/v1/catalog.example.json"
    ))
    .unwrap()
}
#[test]
fn catalog_roundtrip_and_multiple_non_vendor_models_are_supported() {
    let c = Catalog::parse(&serde_json::to_vec(&example()).unwrap()).unwrap();
    assert_eq!(c.assignments.brain, "brain-small");
    let mut changed = c.clone();
    changed.assignments.brain = "code-large".into();
    changed.validate().unwrap();
    assert!(Catalog::parse(&serde_json::to_vec(&changed).unwrap()).is_ok());
}
#[test]
fn web_is_opt_in_and_requires_tools_and_explicit_input_budget() {
    let old = Catalog::parse(&serde_json::to_vec(&example()).unwrap()).unwrap();
    assert!(!old.web_search);
    let mut value = example();
    value["web_search"] = json!(true);
    assert!(Catalog::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    value["models"][0]["supports_tools"] = json!(true);
    assert!(Catalog::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
    value["models"][0]["max_input_bytes"] = json!(4096);
    assert!(Catalog::parse(&serde_json::to_vec(&value).unwrap()).is_err());
}
#[test]
fn unknown_fields_endpoints_capabilities_and_duplicates_fail_closed() {
    for case in 0..9 {
        let mut v = example();
        match case {
            0 => v["unknown"] = json!(true),
            1 => v["models"][0]["runtime"]["base_url"] = json!("https://remote.invalid/v1"),
            2 => v["models"][0]["runtime"]["base_url"] = json!("http://localhost:8080/v1"),
            3 => v["models"][0]["runtime"]["base_url"] = json!("http://127.0.0.1:8015/v1"),
            4 => v["assignments"]["brain"] = json!("missing"),
            5 => v["assignments"]["workers"] = json!(["brain-small"]),
            6 => v["assignments"]["workers"] = json!(["code-large", "code-large"]),
            7 => v["assignments"]["workers"] = json!(["code-large"]),
            _ => v["models"][0]["runtime"]["backend"] = json!("mlx"),
        }
        assert!(
            Catalog::parse(&serde_json::to_vec(&v).unwrap()).is_err(),
            "case {case}"
        );
    }
    assert!(Catalog::parse(&vec![b' '; 65537]).is_err());
}
