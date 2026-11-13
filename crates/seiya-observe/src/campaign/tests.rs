use super::*;
use std::collections::BTreeMap;

fn recipe() -> MlxRecipe {
    MlxRecipe {
        python: "/synthetic/venv/bin/python3".into(),
        runtime_entrypoint: "/synthetic/venv/bin/mlx_lm.server".into(),
        checkpoint: "/synthetic/checkpoint with spaces".into(),
        coordination_directory: "/synthetic/shared-locks".into(),
        minimum_free_bytes: 2 * 1024 * 1024 * 1024,
    }
}
fn observation() -> HostObservation {
    HostObservation {
        sampled_at_millis: 1000,
        physical_bytes: Some(16 * 1024 * 1024 * 1024),
        free_bytes: Some(4 * 1024 * 1024 * 1024),
        pressure: Pressure::Normal,
        external_runtime_clearance: true,
    }
}

#[cfg(target_os = "macos")]
#[test]
fn calibration_exception_never_relaxes_normal_admission_or_claims_capacity() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .unwrap()
        .join(format!(
            "seiya-calibration-policy-{}",
            uuid::Uuid::new_v4().simple()
        ));
    std::fs::create_dir(&directory).unwrap();
    let mut recipe = recipe();
    recipe.coordination_directory = directory.clone();
    let reservation = Reservation::acquire(&recipe).unwrap();
    let samples: Vec<_> = [0, 1000, 2000]
        .into_iter()
        .map(|time| {
            let mut sample = observation();
            sample.sampled_at_millis = time;
            sample.free_bytes = Some(80 << 20);
            sample
        })
        .collect();
    let normal = assess(&recipe, samples[2].clone(), 2000, Some(&reservation)).unwrap();
    assert!(normal.blockers.contains(&Blocker::BelowThreshold));
    let policy = CalibrationPolicy {
        runtime_seconds: 120,
    };
    let ready = assess_calibration(&recipe, &policy, &samples, 2000, Some(&reservation)).unwrap();
    assert!(ready.eligible_for_experimental_calibration);
    assert!(ready.free_pages_threshold_waived_for_this_experiment);
    assert!(!ready.capacity_guaranteed && !ready.model_load_authorized);
    assert!(
        !assess_calibration(&recipe, &policy, &samples, 2000, None)
            .unwrap()
            .eligible_for_experimental_calibration
    );
    for pressure in [Pressure::Warning, Pressure::Critical, Pressure::Unknown] {
        let mut bad = samples.clone();
        bad[1].pressure = pressure;
        assert!(
            !assess_calibration(&recipe, &policy, &bad, 2000, Some(&reservation))
                .unwrap()
                .eligible_for_experimental_calibration
        );
    }
    drop(reservation);
    std::fs::remove_file(directory.join("seiya-mlx-baseline.lock")).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn recipe_is_fixed_local_argv_not_a_shell_or_launch_permission() {
    let recipe = recipe();
    let args = recipe.runtime_arguments(55000).unwrap();
    assert_eq!(args[0], recipe.runtime_entrypoint.to_str().unwrap());
    assert_eq!(args[2], "/synthetic/checkpoint with spaces");
    for (flag, expected) in [
        ("--host", "127.0.0.1"),
        ("--port", "55000"),
        ("--max-tokens", "256"),
        ("--decode-concurrency", "1"),
        ("--prompt-concurrency", "1"),
        ("--prefill-step-size", "256"),
        ("--prompt-cache-size", "1"),
        ("--prompt-cache-bytes", "128MB"),
        ("--chat-template-args", "{\"enable_thinking\":false}"),
    ] {
        assert_eq!(
            args[args.iter().position(|a| a == flag).unwrap() + 1],
            expected
        );
    }
    for port in [0, 8010, 8012, 8081, 49151] {
        assert!(recipe.runtime_arguments(port).is_err());
    }
    let mut invalid = recipe.clone();
    invalid.checkpoint = "remote/model".into();
    assert!(invalid.validate().is_err());
    invalid.checkpoint = "/models/../private".into();
    assert!(invalid.validate().is_err());
    invalid.checkpoint = "/models/line\nbreak".into();
    assert!(invalid.validate().is_err());
    invalid.checkpoint = "/".into();
    assert!(invalid.validate().is_err());
    let mut invalid = recipe.clone();
    invalid.minimum_free_bytes = 0;
    assert!(invalid.validate().is_err());
    let mut json = serde_json::to_value(recipe).unwrap();
    json["extra_flags"] = serde_json::json!(["--trust-remote-code"]);
    assert!(serde_json::from_value::<MlxRecipe>(json).is_err());
}

#[test]
fn preflight_missing_stale_inconsistent_or_critical_data_never_passes_silently() {
    let recipe = recipe();
    let ready = assess(&recipe, observation(), 1000, None).unwrap();
    assert_eq!(ready.blockers, vec![Blocker::NoReservation]);
    assert!(!ready.model_load_authorized);
    for time in [999, 6001, u64::MAX] {
        assert!(assess(&recipe, observation(), time, None)
            .unwrap()
            .blockers
            .contains(&Blocker::StaleObservation));
    }
    for (total, free) in [
        (None, Some(1)),
        (Some(0), Some(0)),
        (Some(10), Some(11)),
        (Some(10), None),
    ] {
        let mut host = observation();
        host.physical_bytes = total;
        host.free_bytes = free;
        assert!(assess(&recipe, host, 1000, None)
            .unwrap()
            .blockers
            .contains(&Blocker::MemoryUnavailable));
    }
    let mut host = observation();
    host.free_bytes = Some(0);
    host.pressure = Pressure::Unknown;
    host.external_runtime_clearance = false;
    let result = assess(&recipe, host, 1000, None).unwrap();
    for blocker in [
        Blocker::BelowThreshold,
        Blocker::PressureNotNormal,
        Blocker::ExternalRuntimeNotCleared,
    ] {
        assert!(result.blockers.contains(&blocker));
    }
    for pressure in [Pressure::Critical, Pressure::Warning] {
        let mut host = observation();
        host.pressure = pressure;
        assert!(assess(&recipe, host, 1000, None)
            .unwrap()
            .blockers
            .contains(&Blocker::PressureNotNormal));
    }
}

#[derive(Clone)]
struct Source {
    contents: BTreeMap<String, Vec<u8>>,
    sizes: Option<Vec<(String, u64)>>,
    versions: BTreeMap<String, String>,
    bad_hash: bool,
    calls: usize,
}
impl Source {
    fn valid() -> Self {
        Self {
            contents: BTreeMap::from([
                (
                    "config.json".into(),
                    b"{\"model_type\":\"synthetic\"}".to_vec(),
                ),
                ("tokenizer.json".into(), b"{}".to_vec()),
                (
                    "tokenizer_config.json".into(),
                    b"{\"chat_template\":\"synthetic template\"}".to_vec(),
                ),
                (
                    "model.safetensors".into(),
                    b"synthetic-not-real-weights".to_vec(),
                ),
            ]),
            sizes: None,
            versions: BTreeMap::from([
                ("python".into(), "3.13.0-fixture".into()),
                ("mlx".into(), "0.0.fixture".into()),
                ("mlx-lm".into(), "0.0.fixture".into()),
                ("transformers".into(), "0.0.fixture".into()),
            ]),
            bad_hash: false,
            calls: 0,
        }
    }
    fn set_json(&mut self, name: &str, value: serde_json::Value) {
        self.contents
            .insert(name.into(), serde_json::to_vec(&value).unwrap());
    }
}
impl ProvenanceSource for Source {
    fn files(&mut self) -> Result<Vec<(String, u64)>, PreparationError> {
        Ok(self.sizes.clone().unwrap_or_else(|| {
            self.contents
                .iter()
                .map(|(name, bytes)| (name.clone(), bytes.len() as u64))
                .collect()
        }))
    }
    fn sha256(&mut self, _: &str) -> Result<String, PreparationError> {
        self.calls += 1;
        // Deliberately synthetic digest for structural tests; never native evidence.
        Ok(if self.bad_hash {
            "<not-a-hash>".into()
        } else {
            "0123456789abcdef".repeat(4)
        })
    }
    fn read_metadata(&mut self, name: &str, cap: usize) -> Result<Vec<u8>, PreparationError> {
        let bytes = self.contents.get(name).unwrap();
        assert!(bytes.len() <= cap);
        Ok(bytes.clone())
    }
    fn versions(&mut self) -> Result<BTreeMap<String, String>, PreparationError> {
        Ok(self.versions.clone())
    }
}

#[test]
fn provenance_is_deterministic_bounded_and_explicit_about_injected_evidence() {
    let result = Provenance::collect(&mut Source::valid()).unwrap();
    assert_eq!(result.assets.len(), 4);
    assert_eq!(result.versions.len(), 4);
    assert_eq!(
        result.verification,
        "injected-source-structural-validation-only"
    );
    assert!(result
        .assets
        .windows(2)
        .all(|pair| pair[0].name < pair[1].name));
    assert_eq!(
        result.total_asset_bytes,
        result.assets.iter().map(|a| a.bytes).sum::<u64>()
    );
    let mut source = Source::valid();
    source.bad_hash = true;
    assert!(Provenance::collect(&mut source).is_err());
    let mut source = Source::valid();
    source.versions.remove("mlx");
    assert!(Provenance::collect(&mut source).is_err());
    let mut source = Source::valid();
    source.versions.insert("mlx".into(), "secret\nline".into());
    assert!(Provenance::collect(&mut source).is_err());
}

#[test]
fn inventory_rejects_escape_scripts_duplicates_sizes_and_excess_before_any_hash() {
    for entries in [
        vec![("../private.json".into(), 1)],
        vec![("model.py".into(), 1)],
        vec![("config.json".into(), 1), ("config.json".into(), 1)],
        vec![("config.json".into(), 0)],
        vec![("config.json".into(), 2 * 1024 * 1024 + 1)],
        vec![("model.safetensors".into(), u64::MAX)],
        (0..257)
            .map(|i| (format!("part-{i}.safetensors"), 1))
            .collect(),
    ] {
        let mut source = Source::valid();
        source.sizes = Some(entries);
        assert!(Provenance::collect(&mut source).is_err());
        assert_eq!(source.calls, 0);
    }
}

#[test]
fn only_exact_tokenizer_payload_names_receive_the_larger_cap() {
    use super::provenance::metadata_cap;
    assert_eq!(metadata_cap("tokenizer.json"), 32 * 1024 * 1024);
    assert_eq!(metadata_cap("tokenizer.model"), 32 * 1024 * 1024);
    for name in [
        "config.json",
        "tokenizer_config.json",
        "special_tokens_map.json",
        "model.safetensors.index.json",
        "chat_template.jinja",
        "copy-tokenizer.json",
        "Tokenizer.json",
        "tokenizer.json.backup",
    ] {
        assert_eq!(metadata_cap(name), 2 * 1024 * 1024);
    }
    for (name, size) in [
        ("tokenizer.json", 32 * 1024 * 1024 + 1),
        ("tokenizer.model", 32 * 1024 * 1024 + 1),
        ("tokenizer_config.json", 2 * 1024 * 1024 + 1),
        ("copy-tokenizer.json", 3 * 1024 * 1024),
    ] {
        let mut source = Source::valid();
        source.sizes = Some(vec![(name.into(), size)]);
        assert_eq!(
            Provenance::collect(&mut source).unwrap_err(),
            PreparationError::Limit
        );
        assert_eq!(source.calls, 0);
    }
}

#[test]
fn aggregated_metadata_limit_is_checked_before_hashes_or_large_reads() {
    let mut source = Source::valid();
    source.sizes = Some(vec![
        ("tokenizer.json".into(), 32 * 1024 * 1024),
        ("tokenizer.model".into(), 32 * 1024 * 1024),
    ]);
    assert_eq!(
        Provenance::collect(&mut source).unwrap_err(),
        PreparationError::Limit
    );
    assert_eq!(source.calls, 0);
}

#[test]
fn larger_tokenizer_json_is_syntax_checked_without_materializing_vocabulary() {
    let mut source = Source::valid();
    source.set_json(
        "tokenizer.json",
        serde_json::json!({"synthetic_padding":"x".repeat(3*1024*1024)}),
    );
    let report = Provenance::collect(&mut source).unwrap();
    assert!(
        report
            .assets
            .iter()
            .find(|a| a.name == "tokenizer.json")
            .unwrap()
            .bytes
            > 2 * 1024 * 1024
    );
    // Type, truncation, malformed JSON and a second document cannot pass.
    for bytes in [
        b"[]".as_slice(),
        b"null",
        b"{\"vocab\":",
        b"{} {}",
        b"{\"vocab\":[1,]}",
        b"{\"vocab\":\"\xff\"}",
    ] {
        let mut source = Source::valid();
        source
            .contents
            .insert("tokenizer.json".into(), bytes.to_vec());
        assert_eq!(
            Provenance::collect(&mut source).unwrap_err(),
            PreparationError::InvalidProvenance
        );
    }
    // Binary SentencePiece payload is opaque; it is not parsed as JSON.
    let mut source = Source::valid();
    source.contents.remove("tokenizer.json");
    source
        .contents
        .insert("tokenizer.model".into(), vec![0xff; 3 * 1024 * 1024]);
    assert!(Provenance::collect(&mut source).is_ok());
}

#[test]
fn shards_require_complete_index_coverage_and_local_assets_not_runtime_code() {
    let mut source = Source::valid();
    source.contents.remove("model.safetensors");
    source
        .contents
        .insert("model-00001-of-00002.safetensors".into(), vec![1]);
    source
        .contents
        .insert("model-00002-of-00002.safetensors".into(), vec![2]);
    assert!(Provenance::collect(&mut source.clone()).is_err());
    source.set_json("model.safetensors.index.json", serde_json::json!({"weight_map":{"a":"model-00001-of-00002.safetensors","b":"model-00002-of-00002.safetensors"}}));
    assert!(Provenance::collect(&mut source.clone()).is_ok());
    for index in [
        serde_json::json!({"weight_map":{"a":"model-00001-of-00002.safetensors"}}),
        serde_json::json!({"weight_map":{"a":"../model.safetensors"}}),
        serde_json::json!({"weight_map":{}}),
    ] {
        let mut invalid = source.clone();
        invalid.set_json("model.safetensors.index.json", index);
        assert!(Provenance::collect(&mut invalid).is_err());
    }
    let mut source = Source::valid();
    source.set_json(
        "config.json",
        serde_json::json!({"auto_map":{"AutoModel":"remote.Code"}}),
    );
    assert!(Provenance::collect(&mut source).is_err());
    let mut source = Source::valid();
    source.set_json("tokenizer_config.json", serde_json::json!({}));
    assert!(Provenance::collect(&mut source).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn cooperative_lease_excludes_same_location_and_held_ports_cannot_be_rebound() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .unwrap()
        .join(format!("seiya-lease-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&directory).unwrap();
    let mut recipe = recipe();
    recipe.coordination_directory = directory.clone();
    let first = Reservation::acquire(&recipe).unwrap();
    assert!(matches!(
        Reservation::acquire(&recipe),
        Err(PreparationError::Busy)
    ));
    let (a, b) = first.ports().unwrap();
    assert_ne!(a, b);
    for port in [a, b] {
        assert!(std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_err());
    }
    let result = assess(&recipe, observation(), 1000, Some(&first)).unwrap();
    assert!(result.blockers.is_empty());
    assert!(!result.model_load_authorized);
    let mut other = recipe.clone();
    other.coordination_directory = directory.join("other");
    assert!(assess(&other, observation(), 1000, Some(&first))
        .unwrap()
        .blockers
        .contains(&Blocker::NoReservation));
    drop(first);
    assert!(directory.join("seiya-mlx-baseline.lock").exists());
    let second = Reservation::acquire(&recipe).unwrap();
    drop(second);
    // Only known files in this newly created synthetic directory, no recursive removal.
    std::fs::remove_file(directory.join("seiya-mlx-baseline.lock")).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn lease_survives_port_handoff_and_rearms_only_after_both_sockets_are_released() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .unwrap()
        .join(format!("seiya-handoff-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&directory).unwrap();
    let mut recipe = recipe();
    recipe.coordination_directory = directory.clone();
    let mut reservation = Reservation::acquire(&recipe).unwrap();
    let ports = reservation.ports().unwrap();
    assert_eq!(reservation.release_runtime().unwrap(), ports.0);
    assert!(reservation.rearm().is_err());
    assert!(matches!(
        Reservation::acquire(&recipe),
        Err(PreparationError::Busy)
    ));
    assert_eq!(reservation.release_coordinator().unwrap(), ports.1);
    assert!(reservation.ports().is_err());
    assert!(matches!(
        Reservation::acquire(&recipe),
        Err(PreparationError::Busy)
    ));
    reservation.rearm().unwrap();
    assert!(reservation.ports().is_ok());
    drop(reservation);
    std::fs::remove_file(directory.join("seiya-mlx-baseline.lock")).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn lock_symlink_never_overwrites_foreign_target() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .unwrap()
        .join(format!("seiya-lease-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&directory).unwrap();
    let target = directory.join("sentinel");
    std::fs::write(&target, "unchanged").unwrap();
    let lock = directory.join("seiya-mlx-baseline.lock");
    std::os::unix::fs::symlink(&target, &lock).unwrap();
    let mut recipe = recipe();
    recipe.coordination_directory = directory.clone();
    assert!(matches!(
        Reservation::acquire(&recipe),
        Err(PreparationError::Io)
    ));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "unchanged");
    std::fs::remove_file(lock).unwrap();
    std::fs::remove_file(target).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn installed_versions_are_read_as_metadata_not_imported_or_executed() {
    assert_eq!(
        super::native::package_version(
            b"Metadata-Version: 2.1\nName: mlx_lm\nVersion: 0.1.fixture\n\nLong description\n",
            "mlx-lm"
        )
        .unwrap(),
        "0.1.fixture"
    );
    for bytes in [
        b"Name: mlx\nVersion: 1\nVersion: 2\n".as_slice(),
        b"Name: wrong\nVersion: 1\n",
        b"Name: mlx\nVersion: \n",
        b"Name: mlx\nVersion: 1 secret\n",
        b"Name: mlx\n\nVersion: 1\n",
    ] {
        assert!(super::native::package_version(bytes, "mlx").is_err());
    }
}

#[test]
fn native_preflight_output_parsing_never_treats_inactive_as_free_or_missing_as_zero() {
    let memory = b"17179869184\n".to_vec();
    let vm = b"Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 10.\nPages inactive: 900.\n".to_vec();
    let host = super::native::host_from_outputs(
        1000,
        false,
        Ok(memory.clone()),
        Ok(b"1\n".to_vec()),
        Ok(vm.clone()),
    );
    assert_eq!(host.physical_bytes, Some(17179869184));
    assert_eq!(host.free_bytes, Some(163840));
    assert_eq!(host.pressure, Pressure::Normal);
    assert!(!host.external_runtime_clearance);
    for (raw, expected) in [
        (b"2\n".to_vec(), Pressure::Warning),
        (b"4\n".to_vec(), Pressure::Critical),
        (b"0\n".to_vec(), Pressure::Unknown),
        (b"1\nsecret\n".to_vec(), Pressure::Unknown),
    ] {
        assert_eq!(
            super::native::host_from_outputs(0, true, Ok(memory.clone()), Ok(raw), Ok(vm.clone()))
                .pressure,
            expected
        );
    }
    let missing = super::native::host_from_outputs(
        0,
        true,
        Err(PreparationError::TimedOut),
        Err(PreparationError::Io),
        Ok(b"malformed".to_vec()),
    );
    assert_eq!(missing.physical_bytes, None);
    assert_eq!(missing.free_bytes, None);
    assert_eq!(missing.pressure, Pressure::Unknown);
}

#[test]
fn mlx_arguments_and_resource_policy_match_the_checked_in_baseline_plan() {
    let args = recipe().runtime_arguments(55000).unwrap();
    let plan = crate::runner::plan();
    let settings = &plan["comparison"]["runtime_settings"];
    for (flag, key) in [
        ("--max-tokens", "max_output_tokens"),
        ("--decode-concurrency", "decode_concurrency"),
        ("--prompt-concurrency", "prompt_concurrency"),
        ("--prefill-step-size", "prefill_step_size"),
        ("--prompt-cache-size", "prompt_cache_size"),
    ] {
        let value = &args[args.iter().position(|a| a == flag).unwrap() + 1];
        assert_eq!(
            value.parse::<u64>().unwrap(),
            settings[key].as_u64().unwrap()
        );
    }
    assert_eq!(
        args[args
            .iter()
            .position(|a| a == "--prompt-cache-bytes")
            .unwrap()
            + 1],
        settings["prompt_cache_bytes"].as_str().unwrap()
    );
    let thinking: serde_json::Value = serde_json::from_str(
        &args[args
            .iter()
            .position(|a| a == "--chat-template-args")
            .unwrap()
            + 1],
    )
    .unwrap();
    assert_eq!(thinking["enable_thinking"], settings["enable_thinking"]);
    assert_eq!(plan["runner_implemented"], true);
    assert_eq!(plan["authorization_required"], true);
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "explicit native hash/version smoke on new synthetic files only; no MLX imports, weights or host-memory probes"]
async fn native_provenance_hashes_held_synthetic_files_without_loading_runtime() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .unwrap()
        .join(format!(
            "seiya-provenance-{}",
            uuid::Uuid::new_v4().simple()
        ));
    std::fs::create_dir(&directory).unwrap();
    let mut source = Source::valid();
    source.set_json(
        "tokenizer.json",
        serde_json::json!({"synthetic_padding":"x".repeat(3*1024*1024)}),
    );
    for (name, bytes) in &source.contents {
        std::fs::write(directory.join(name), bytes).unwrap();
    }
    std::fs::write(directory.join("model.safetensors"), b"abc").unwrap();
    let entrypoint = directory.join("entrypoint-not-executed");
    std::fs::write(&entrypoint, b"abc").unwrap();
    for name in ["mlx", "mlx-lm", "transformers"] {
        std::fs::write(
            directory.join(format!("{name}-METADATA")),
            format!("Name: {name}\nVersion: 0.0.fixture\n\n"),
        )
        .unwrap();
    }
    let packages = PackageMetadata {
        mlx: directory.join("mlx-METADATA"),
        mlx_lm: directory.join("mlx-lm-METADATA"),
        transformers: directory.join("transformers-METADATA"),
    };
    let mut recipe = recipe();
    recipe.checkpoint = directory.clone();
    recipe.runtime_entrypoint = entrypoint;
    recipe.python = "/opt/homebrew/bin/python3".into();
    let result = NativeProvenance::inspect(&recipe, &packages).await.unwrap();
    let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(result.entrypoint_sha256, expected);
    assert_eq!(
        result
            .checkpoint
            .assets
            .iter()
            .find(|a| a.name == "model.safetensors")
            .unwrap()
            .sha256,
        expected
    );
    assert_eq!(
        result.checkpoint.verification,
        "native-held-files-sha256-before-measurement"
    );
    // Replace only our synthetic selected asset with a symlink. Never read it.
    std::fs::remove_file(directory.join("tokenizer.json")).unwrap();
    std::os::unix::fs::symlink(
        directory.join("mlx-METADATA"),
        directory.join("tokenizer.json"),
    )
    .unwrap();
    assert!(NativeProvenance::inspect(&recipe, &packages).await.is_err());
    for name in source.contents.keys().map(String::as_str).chain([
        "entrypoint-not-executed",
        "mlx-METADATA",
        "mlx-lm-METADATA",
        "transformers-METADATA",
    ]) {
        std::fs::remove_file(directory.join(name)).unwrap();
    }
    std::fs::remove_dir(directory).unwrap();
}
