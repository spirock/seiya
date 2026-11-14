//! Offline CLI only: owned ephemeral listener proves no inventory/inference call.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::Command,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("seiya-catalog-cli-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn check_catalog_never_contacts_an_owned_listener_or_changes_config() {
    let fixture = Fixture::new();
    let listener = loop {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        if !matches!(
            listener.local_addr().unwrap().port(),
            8015 | 8016 | 11434 | 49399 | 49400
        ) {
            break listener;
        }
    };
    listener.set_nonblocking(true).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../contracts/local-models/v1/catalog.example.json"
    ))
    .unwrap();
    for m in value["models"].as_array_mut().unwrap() {
        m["runtime"]["base_url"] =
            serde_json::json!(format!("http://{}/v1", listener.local_addr().unwrap()));
    }
    let file = fixture.0.join("catalog.json");
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let bytes = serde_json::to_vec(&value).unwrap();
    options.open(&file).unwrap().write_all(&bytes).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_seiya-desktop"))
        .args(["--check-catalog", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["network_used"], false);
    assert_eq!(report["inference_validated"], false);
    assert!(matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
    assert_eq!(fs::read(file).unwrap(), bytes);
    let help = Command::new(env!("CARGO_BIN_EXE_seiya-desktop"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&help.stdout).contains("--check-catalog"));
}
