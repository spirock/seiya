//! Explicit manual smoke on owned fixture processes, never part of normal CI.
#![cfg(target_os = "macos")]
use seiya_observe::{probes, Collector, Phase, Role};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.stdin.take(); // EOF asks the helper to stop/reap its own child.
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.0.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "helper only for explicitly invoked owned-tree smoke"]
fn owned_tree_fixture() {
    if std::env::var("SEIYA_PROBE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let mut sleep = Owned(
        Command::new("/bin/sleep")
            .arg("20")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    println!("SEIYA_FIXTURE_READY");
    let mut byte = [0];
    let _ = std::io::stdin().read(&mut byte);
    sleep.0.kill().unwrap();
    sleep.0.wait().unwrap();
}

#[tokio::test]
#[ignore = "manual native probe smoke: owns a helper and sleep; no model or network"]
async fn native_owned_tree_and_system_probe_smoke() {
    let mut owned = Owned(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "owned_tree_fixture", "--ignored", "--nocapture"])
            .env_clear()
            .env("SEIYA_PROBE_FIXTURE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let (ready, waiting) = std::sync::mpsc::sync_channel(1);
    let stdout = owned.0.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line.unwrap_or_default() == "SEIYA_FIXTURE_READY" {
                let _ = ready.send(());
            }
        }
    });
    waiting.recv_timeout(Duration::from_secs(3)).unwrap();
    let mut adapter =
        probes::from_child(&mut owned.0, Role::Coordinator, Duration::from_millis(500)).unwrap();
    let mut readings = adapter.sample().await.unwrap();
    assert_eq!(
        readings.targets().len(),
        2,
        "root and its own sleep descendant"
    );
    let mut collector = Collector::new(readings.targets(), 1).unwrap();
    let snapshot = collector.collect(0, Phase::Idle, &mut readings).unwrap();
    for process in &snapshot.processes {
        assert!(process.rss.value().is_some());
    }
    assert_eq!(snapshot.processes[1].role, Role::Descendant);
    let system = adapter.sample_system().await.unwrap();
    assert_eq!(system.scope, "whole-host");
    assert!(system.vm_stat.is_ok(), "{:?}", system.vm_stat);
    assert!(system.swap.is_ok(), "{:?}", system.swap);
    drop(adapter);
    owned.0.stdin.as_mut().unwrap().write_all(b"x").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while owned.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(owned.0.try_wait().unwrap().unwrap().success());
    reader.join().unwrap();
    let mut self_probe = probes::for_current_collector(Duration::from_millis(500)).unwrap();
    let mut self_readings = self_probe.sample().await.unwrap();
    let mut self_collector = Collector::new(self_readings.targets(), 1).unwrap();
    let self_sample = self_collector
        .collect(0, Phase::Idle, &mut self_readings)
        .unwrap();
    let own = self_sample
        .processes
        .iter()
        .find(|process| process.identity.pid() == std::process::id())
        .unwrap();
    assert_eq!(own.role, Role::Collector);
    assert!(own.rss.value().is_some());
    println!("native smoke: owned root/descendant RSS and explicit VM/swap parsed; helper reaped child, no model or service changes");
}
