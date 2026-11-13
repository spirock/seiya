use super::*;
use serde_json::{json, Value};

const FIXTURE: &[u8] =
    include_bytes!("../../../../contracts/benchmarks/chat-session/v0.1.0/evidence.json");

fn fixture() -> Evidence {
    Evidence::parse(FIXTURE).unwrap()
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let base =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/session-evidence-tests");
        fs::create_dir_all(&base).unwrap();
        Self(base.join(Uuid::new_v4().to_string()))
    }
    fn store(&self) -> Store {
        Store::create(self.0.clone()).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        // Only this test's newly created flat fixture directory. Never recurse.
        if let Ok(entries) = fs::read_dir(&self.0) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
        let _ = fs::remove_dir(&self.0);
    }
}

#[test]
fn frozen_workload_has_ten_memory_turns_and_ten_distinct_synthesis_cases() {
    // Non-cryptographic change guard: fixture edits require a version review.
    // This is not model provenance or authentication of evidence producers.
    let checksum = WORKLOAD.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    assert_eq!(checksum, 0x9bcfd8282c71c02a);
    let work: Value = serde_json::from_str(WORKLOAD).unwrap();
    assert_eq!(work["version"], WORKLOAD_VERSION);
    assert_eq!(work["synthetic_only"], true);
    let turns = work["memory_dialogue"].as_array().unwrap();
    assert_eq!(turns.len(), 10);
    assert!(turns
        .iter()
        .all(|v| v.as_str().is_some_and(|s| !s.is_empty() && s.len() < 1024)));
    let cases = work["synthesis_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    let mut ids = HashSet::new();
    for case in cases {
        assert!(ids.insert(case["id"].as_str().unwrap()));
        assert!(!case["prompt"].as_str().unwrap().is_empty());
        assert!(case["sources"].is_array() && case["expected_facts"].is_array());
        for source in case["sources"].as_array().unwrap() {
            assert_eq!(source.as_object().unwrap().len(), 2);
            assert!(source["id"].is_string() && source["text"].is_string());
        }
    }
    assert!(!WORKLOAD.contains("http://") && !WORKLOAD.contains("https://"));
    assert!(cases
        .iter()
        .any(|c| c["sources"].as_array().unwrap().is_empty()));
    assert!(WORKLOAD.contains("INSTRUCCIÓN MALICIOSA"));
}

#[test]
fn strict_fixture_roundtrip_preserves_nulls_and_separates_generation_from_quality() {
    let record = fixture();
    let value = serde_json::to_value(&record).unwrap();
    assert_eq!(value, serde_json::from_slice::<Value>(FIXTURE).unwrap());
    assert!(value["events"][1]["metrics"][1]["value"].is_null());
    assert_eq!(
        value["events"][1]["metrics"][2]["missing_reason"],
        "buffered_transport"
    );
    assert_eq!(record.summary.task_resolved, None);
    assert!(record.summary.generation_completed);
    let mut record = record;
    record.summary.task_resolved = Some(false);
    assert!(record.validate().is_ok());
    // A valid citation to a supplied snippet is not proof of a page read.
    record.summary.sources_available = 1;
    record.summary.valid_citations = 1;
    assert!(record.validate().is_ok());
    assert_eq!(record.summary.pages_read, 0);
}

#[test]
fn free_text_unknown_fields_and_fake_real_modes_are_rejected_at_every_level() {
    for (pointer, key) in [
        ("", "prompt"),
        ("/summary", "answer"),
        ("/events/0", "url"),
        ("/events/1/metrics/0", "secret"),
    ] {
        let mut value: Value = serde_json::from_slice(FIXTURE).unwrap();
        value.pointer_mut(pointer).unwrap()[key] = json!("NEVER_STORE_THIS");
        assert_eq!(
            Evidence::parse(&serde_json::to_vec(&value).unwrap()).unwrap_err(),
            Error::Invalid
        );
    }
    for (key, value) in [
        ("mode", "model-campaign"),
        ("mode", "model-session"),
        ("schema_version", "future"),
        ("workload_version", "future"),
    ] {
        let mut document: Value = serde_json::from_slice(FIXTURE).unwrap();
        document[key] = json!(value);
        assert_eq!(
            Evidence::parse(&serde_json::to_vec(&document).unwrap()).unwrap_err(),
            Error::Invalid
        );
    }
}

#[test]
fn malformed_trailing_duplicate_and_negative_json_do_not_become_records() {
    for bytes in [b"{} {}".as_slice(), b"\xff", b"{", b"null", b"[]"] {
        assert_eq!(Evidence::parse(bytes).unwrap_err(), Error::Invalid);
    }
    let duplicate =
        String::from_utf8(FIXTURE.to_vec())
            .unwrap()
            .replacen("{", "{\"mode\":\"synthetic\",", 1);
    assert_eq!(
        Evidence::parse(duplicate.as_bytes()).unwrap_err(),
        Error::Invalid
    );
    let mut value: Value = serde_json::from_slice(FIXTURE).unwrap();
    value["events"][1]["metrics"][0]["value"] = json!(-1);
    assert_eq!(
        Evidence::parse(&serde_json::to_vec(&value).unwrap()).unwrap_err(),
        Error::Invalid
    );
}

#[test]
fn metric_absence_is_not_zero_and_values_are_bounded_without_ambiguous_origins() {
    let mut metric = fixture().events[1].metrics[0].clone();
    metric.value = Some(0);
    assert!(metric.validate().is_ok());
    metric.value = None;
    assert_eq!(metric.validate(), Err(Error::Invalid));
    metric.origin = Origin::Unavailable;
    metric.missing_reason = Some(MissingReason::NotObserved);
    assert!(metric.validate().is_ok());
    metric.value = Some(0);
    assert_eq!(metric.validate(), Err(Error::Invalid));
    metric.origin = Origin::Synthetic;
    metric.missing_reason = None;
    metric.kind = MetricKind::LoadMillis;
    metric.value = Some(MAX_MILLIS + 1);
    assert_eq!(metric.validate(), Err(Error::Limit));
    metric.kind = MetricKind::OutputTokens;
    metric.value = Some(1_000_001);
    assert_eq!(metric.validate(), Err(Error::Limit));
}

#[test]
fn identity_hashes_event_clock_counts_and_summary_invariants_fail_closed() {
    for id in [
        "../escape",
        "00000000-0000-0000-0000-000000000000",
        "not-an-id",
    ] {
        let mut record = fixture();
        record.experiment_id = id.into();
        assert_eq!(record.validate(), Err(Error::Invalid));
    }
    for hash in ["https://secret.invalid", "A".repeat(64).as_str(), "a"] {
        let mut record = fixture();
        record.profile_sha256 = hash.into();
        assert_eq!(record.validate(), Err(Error::Invalid));
    }
    let mut record = fixture();
    record.events[2].at_millis = 9;
    assert_eq!(record.validate(), Err(Error::Invalid));
    let mut record = fixture();
    record.events[2].at_millis = 21;
    assert_eq!(record.validate(), Err(Error::Invalid));
    let mut record = fixture();
    let duplicate = record.events[1].metrics[0].clone();
    record.events[1].metrics.push(duplicate);
    assert_eq!(record.validate(), Err(Error::Invalid));
    let mut record = fixture();
    record.events = vec![record.events[0].clone(); MAX_EVENTS + 1];
    assert_eq!(record.validate(), Err(Error::Limit));
    let mut record = fixture();
    record.summary.cleanup_confirmed = false;
    assert_eq!(record.validate(), Err(Error::Invalid));
    record.summary.outcome = Outcome::Aborted;
    record.summary.cause = Some(Cause::UnconfirmedCleanup);
    assert!(record.validate().is_ok());
    record.summary.task_resolved = Some(true);
    record.summary.generation_completed = false;
    assert_eq!(record.validate(), Err(Error::Invalid));
    let mut record = fixture();
    record.summary.pages_read = 1;
    assert_eq!(record.validate(), Err(Error::Invalid));
    assert_eq!(
        Evidence::parse(&vec![b' '; MAX_FILE_BYTES as usize + 1]).unwrap_err(),
        Error::Limit
    );
}

#[test]
fn publication_is_atomic_private_and_never_replaces_existing_evidence() {
    let directory = Directory::new();
    let mut store = directory.store();
    let record = fixture();
    let destination = directory.0.join(format!("{}.json", record.experiment_id));
    store
        .publish_before_commit(&record, |temporary| {
            assert!(temporary.exists());
            assert!(!destination.exists());
            assert_eq!(
                Evidence::parse(&fs::read(temporary).unwrap())
                    .unwrap()
                    .experiment_id,
                record.experiment_id
            );
            Ok(())
        })
        .unwrap();
    let original = fs::read(&destination).unwrap();
    assert_eq!(store.publish(&record), Err(Error::Conflict));
    assert_eq!(fs::read(&destination).unwrap(), original);
    assert_eq!(store.rebuild_index().unwrap().incomplete_files, 0);
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&directory.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(destination).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(matches!(
        Store::create(directory.0.clone()),
        Err(Error::Conflict)
    ));
}

#[test]
fn interrupted_publication_preserves_pending_evidence_without_visible_partial_record() {
    let directory = Directory::new();
    let mut store = directory.store();
    assert_eq!(
        store.publish_before_commit(&fixture(), |_| Err(Error::Io)),
        Err(Error::Io)
    );
    let index = store.rebuild_index().unwrap();
    assert!(index.entries.is_empty());
    assert_eq!(index.incomplete_files, 1);
    store.publish(&fixture()).unwrap();
    let index = store.rebuild_index().unwrap();
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.incomplete_files, 1);
}

#[test]
fn destination_created_during_commit_is_preserved_and_not_replaced() {
    let directory = Directory::new();
    let mut store = directory.store();
    let record = fixture();
    let destination = directory.0.join(format!("{}.json", record.experiment_id));
    let result = store.publish_before_commit(&record, |_| {
        fs::write(&destination, b"previous-fixture").unwrap();
        Ok(())
    });
    assert_eq!(result, Err(Error::Conflict));
    assert_eq!(fs::read(&destination).unwrap(), b"previous-fixture");
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
}

#[test]
fn index_rebuild_is_deterministic_and_recipe_changes_block_comparison() {
    let directory = Directory::new();
    let mut store = directory.store();
    let mut a = fixture();
    let mut b = fixture();
    a.experiment_id = "00000000-0000-4000-8000-000000000002".into();
    assert!(a.comparable_with(&b));
    b.profile_sha256 = "e".repeat(64);
    assert!(!a.comparable_with(&b));
    b = a.clone();
    b.checkpoint_sha256 = "e".repeat(64);
    assert!(!a.comparable_with(&b));
    b = a.clone();
    b.runtime_recipe_sha256 = "e".repeat(64);
    assert!(!a.comparable_with(&b));
    b = a.clone();
    b.host_profile_sha256 = "e".repeat(64);
    assert!(!a.comparable_with(&b));
    b = fixture();
    store.publish(&a).unwrap();
    store.publish(&b).unwrap();
    let one = serde_json::to_value(store.rebuild_index().unwrap()).unwrap();
    let two = serde_json::to_value(store.rebuild_index().unwrap()).unwrap();
    assert_eq!(one, two);
    drop(store);
    assert_eq!(
        one,
        serde_json::to_value(Store::read_index(directory.0.clone()).unwrap()).unwrap()
    );
    assert_eq!(one["entries"][0]["experiment_id"], b.experiment_id);
    assert!(one["entries"][0].get("events").is_none());
}

#[test]
fn record_capacity_and_pending_byte_budget_are_enforced_without_deleting_prior_data() {
    let directory = Directory::new();
    let mut store = directory.store();
    for _ in 0..MAX_RECORDS {
        let mut record = fixture();
        record.experiment_id = Uuid::new_v4().to_string();
        store.publish(&record).unwrap();
    }
    assert_eq!(store.publish(&fixture()), Err(Error::Limit));
    assert_eq!(store.rebuild_index().unwrap().entries.len(), MAX_RECORDS);
    let directory = Directory::new();
    let mut store = directory.store();
    for _ in 0..MAX_RECORDS {
        let path = directory.0.join(format!(".pending-{}", Uuid::new_v4()));
        File::create(path).unwrap().set_len(MAX_FILE_BYTES).unwrap();
    }
    assert_eq!(store.publish(&fixture()), Err(Error::Limit));
    assert_eq!(store.rebuild_index().unwrap().incomplete_files, MAX_RECORDS);
}

#[test]
fn corrupt_renamed_foreign_or_oversized_records_are_not_silently_indexed() {
    let directory = Directory::new();
    let store = directory.store();
    let path = directory
        .0
        .join(format!("{}.json", fixture().experiment_id));
    fs::write(&path, b"{").unwrap();
    assert_eq!(store.rebuild_index().unwrap_err(), Error::Invalid);
    let mut record = fixture();
    record.experiment_id = Uuid::new_v4().to_string();
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(store.rebuild_index().unwrap_err(), Error::Invalid);
    File::create(&path)
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    assert_eq!(store.rebuild_index().unwrap_err(), Error::Limit);
    fs::remove_file(path).unwrap();
    fs::write(directory.0.join("foreign.json"), b"{}").unwrap();
    assert_eq!(store.rebuild_index().unwrap_err(), Error::Invalid);
}

#[cfg(unix)]
#[test]
fn symlink_children_are_rejected_before_reading_or_writing() {
    use std::os::unix::fs::symlink;
    let directory = Directory::new();
    let mut store = directory.store();
    symlink(
        "nonexistent-fixture",
        directory
            .0
            .join(format!("{}.json", fixture().experiment_id)),
    )
    .unwrap();
    assert_eq!(store.publish(&fixture()), Err(Error::Invalid));
    assert_eq!(store.rebuild_index().unwrap_err(), Error::Invalid);
    let link = directory.0.with_extension("link");
    symlink(&directory.0, &link).unwrap();
    let result = Store::read_index(link.clone());
    fs::remove_file(link).unwrap();
    assert_eq!(result.unwrap_err(), Error::Invalid);
}

#[cfg(unix)]
#[test]
fn shared_directory_and_relative_store_paths_are_rejected() {
    use std::os::unix::fs::PermissionsExt;
    assert!(matches!(
        Store::create(PathBuf::from("relative")),
        Err(Error::Invalid)
    ));
    assert_eq!(
        Store::read_index(PathBuf::from("relative")).unwrap_err(),
        Error::Invalid
    );
    let directory = Directory::new();
    let store = directory.store();
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(store.rebuild_index().unwrap_err(), Error::Invalid);
}
