use super::*;
use crate::{Availability, Collector, Phase, Reason};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

#[derive(Default)]
struct State {
    alive: bool,
    metadata: BTreeMap<u32, Metadata>,
    children: BTreeMap<u32, Vec<u32>>,
    scanned: Vec<u32>,
}
fn meta(pid: u32, parent_pid: u32, start: &str) -> Metadata {
    Metadata {
        identity: ProcessIdentity::new(pid, start).unwrap(),
        parent_pid,
    }
}
struct Table(Arc<Mutex<State>>);
impl ProcessTable for Table {
    fn root_alive(&mut self) -> Result<bool, ProbeError> {
        Ok(self.0.lock().unwrap().alive)
    }
    fn metadata(&mut self, pid: u32) -> Result<Option<Metadata>, ProbeError> {
        Ok(self.0.lock().unwrap().metadata.get(&pid).cloned())
    }
    fn children(&mut self, pid: u32) -> Result<Vec<u32>, ProbeError> {
        let mut state = self.0.lock().unwrap();
        state.scanned.push(pid);
        Ok(state.children.get(&pid).cloned().unwrap_or_default())
    }
}
type Mutation = Box<dyn FnMut(Request) + Send>;
#[derive(Default)]
struct FakeExecutor {
    calls: Vec<Request>,
    replies: VecDeque<Result<Vec<u8>, ProbeError>>,
    mutation: Option<Mutation>,
    pending: bool,
    active: Arc<AtomicUsize>,
}
struct Active(Arc<AtomicUsize>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Executor for FakeExecutor {
    fn run(&mut self, request: Request) -> ProbeFuture<'_> {
        Box::pin(async move {
            self.calls.push(request);
            if let Some(mutation) = &mut self.mutation {
                mutation(request);
            }
            self.active.fetch_add(1, Ordering::SeqCst);
            let _guard = Active(self.active.clone());
            if self.pending {
                std::future::pending::<()>().await;
            }
            match self.replies.pop_front() {
                Some(reply) => reply,
                None => match request {
                    Request::Rss(pid) => Ok(format!("{pid} 12\n").into_bytes()),
                    _ => panic!("unprepared system probe"),
                },
            }
        })
    }
}
fn adapter() -> (Adapter<Table, FakeExecutor>, Arc<Mutex<State>>) {
    let root = meta(10, 1, "100-25");
    let state = Arc::new(Mutex::new(State {
        alive: true,
        metadata: BTreeMap::from([(10, root.clone())]),
        ..State::default()
    }));
    (
        Adapter::new(
            Table(state.clone()),
            FakeExecutor::default(),
            root,
            Role::Runtime,
        ),
        state,
    )
}
fn add_child(state: &Arc<Mutex<State>>, pid: u32, parent: u32) {
    let mut state = state.lock().unwrap();
    state.metadata.insert(pid, meta(pid, parent, "100-26"));
    state.children.entry(parent).or_default().push(pid);
}

#[test]
fn inspect_targets_verifies_tree_without_executor_and_fails_on_reused_root() {
    let (mut adapter, state) = adapter();
    add_child(&state, 11, 10);
    let targets = adapter.inspect_targets().unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[1].identity.pid(), 11);
    assert!(adapter.executor.calls.is_empty());
    state
        .lock()
        .unwrap()
        .metadata
        .insert(10, meta(10, 1, "reused"));
    assert!(matches!(
        adapter.inspect_targets(),
        Err(ProbeError::IdentityChanged)
    ));
    assert!(matches!(adapter.inspect_targets(), Err(ProbeError::Closed)));
    assert!(adapter.executor.calls.is_empty());
}

#[tokio::test]
async fn owns_only_root_and_verified_descendants_and_feeds_pure_collector() {
    let (mut adapter, state) = adapter();
    add_child(&state, 11, 10);
    add_child(&state, 12, 11);
    let mut readings = adapter.sample().await.unwrap();
    let mut collector = Collector::new(readings.targets(), 1).unwrap();
    let snapshot = collector
        .collect(0, Phase::ColdText, &mut readings)
        .unwrap();
    assert_eq!(snapshot.processes.len(), 3);
    for sample in &snapshot.processes {
        assert_eq!(sample.rss.value(), Some(12 * 1024));
    }
    assert_eq!(snapshot.processes[0].role, Role::Runtime);
    assert_eq!(snapshot.processes[2].role, Role::Descendant);
    assert_eq!(
        adapter.executor.calls,
        [Request::Rss(10), Request::Rss(11), Request::Rss(12)]
    );
    assert_eq!(state.lock().unwrap().scanned, [10, 11, 12]);
}

#[tokio::test]
async fn stopped_original_root_blocks_all_probes_and_does_not_adopt_reused_pid() {
    let (mut adapter, state) = adapter();
    state.lock().unwrap().alive = false;
    assert!(matches!(
        adapter.sample().await,
        Err(ProbeError::RootExited)
    ));
    state.lock().unwrap().alive = true;
    assert!(matches!(adapter.sample().await, Err(ProbeError::Closed)));
    assert!(adapter.executor.calls.is_empty());
}

#[tokio::test]
async fn root_start_or_parent_changes_are_terminal_before_any_command() {
    for changed in [meta(10, 1, "different"), meta(10, 9, "100-25")] {
        let (mut adapter, state) = adapter();
        state.lock().unwrap().metadata.insert(10, changed);
        assert!(matches!(
            adapter.sample().await,
            Err(ProbeError::IdentityChanged | ProbeError::OwnershipChanged)
        ));
        assert!(matches!(adapter.sample().await, Err(ProbeError::Closed)));
        assert!(adapter.executor.calls.is_empty());
    }
}

#[tokio::test]
async fn listed_foreign_child_is_not_registered_or_sampled() {
    let (mut adapter, state) = adapter();
    add_child(&state, 11, 10);
    state
        .lock()
        .unwrap()
        .metadata
        .insert(11, meta(11, 99, "foreign"));
    assert!(matches!(
        adapter.sample().await,
        Err(ProbeError::OwnershipChanged)
    ));
    assert!(adapter.executor.calls.is_empty());
}

#[tokio::test]
async fn identity_change_during_rss_discards_all_readings_and_closes_adapter() {
    let (mut adapter, state) = adapter();
    adapter.executor.mutation = Some(Box::new(move |_| {
        state
            .lock()
            .unwrap()
            .metadata
            .insert(10, meta(10, 1, "recycled"));
    }));
    assert!(matches!(
        adapter.sample().await,
        Err(ProbeError::IdentityChanged)
    ));
    assert!(matches!(adapter.sample().await, Err(ProbeError::Closed)));
    assert_eq!(adapter.executor.calls.len(), 1);
}

#[tokio::test]
async fn exited_child_is_missing_not_zero_and_never_queried_again() {
    let (mut adapter, state) = adapter();
    add_child(&state, 11, 10);
    let changed = state.clone();
    adapter.executor.mutation = Some(Box::new(move |request| {
        if request == Request::Rss(11) {
            let mut state = changed.lock().unwrap();
            state.metadata.remove(&11);
            state.children.remove(&10);
        }
    }));
    let mut readings = adapter.sample().await.unwrap();
    let mut collector = Collector::new(readings.targets(), 2).unwrap();
    let first = collector.collect(0, Phase::Idle, &mut readings).unwrap();
    assert_eq!(first.processes[1].rss.reason(), Some(Reason::Exited));
    assert_eq!(first.processes[1].rss.value(), None);
    let mut next = adapter.sample().await.unwrap();
    let second = collector.collect(1, Phase::Idle, &mut next).unwrap();
    assert_eq!(second.processes[1].rss.value(), None);
    assert_eq!(
        adapter
            .executor
            .calls
            .iter()
            .filter(|r| **r == Request::Rss(11))
            .count(),
        1
    );
}

#[tokio::test]
async fn timeout_permission_invalid_stdout_and_probe_errors_remain_explicit_and_recover() {
    for (reply, reason) in [
        (Err(ProbeError::TimedOut), Reason::TimedOut),
        (Err(ProbeError::PermissionDenied), Reason::PermissionDenied),
        (Err(ProbeError::Failed), Reason::ProbeFailed),
        (Err(ProbeError::Limit), Reason::InvalidData),
        (Ok(vec![255]), Reason::InvalidData),
        (Ok(b"99 123\n".to_vec()), Reason::InvalidData),
    ] {
        let (mut adapter, _) = adapter();
        adapter.executor.replies.push_back(reply);
        let mut readings = adapter.sample().await.unwrap();
        let mut collector = Collector::new(readings.targets(), 2).unwrap();
        let first = collector.collect(0, Phase::Idle, &mut readings).unwrap();
        assert_eq!(first.processes[0].rss.value(), None);
        assert_eq!(first.processes[0].rss.reason(), Some(reason));
        assert_eq!(
            first.processes[0].rss.availability(),
            Availability::Unavailable
        );
        let mut recovered = adapter.sample().await.unwrap();
        assert_eq!(
            collector
                .collect(1, Phase::Resume, &mut recovered)
                .unwrap()
                .processes[0]
                .rss
                .value(),
            Some(12288)
        );
    }
}

#[tokio::test]
async fn target_count_duplicate_children_and_cycles_stop_before_rss() {
    let (mut bounded, state) = adapter();
    for pid in 11..43 {
        add_child(&state, pid, 10);
    }
    assert!(matches!(bounded.sample().await, Err(ProbeError::Limit)));
    assert!(bounded.executor.calls.is_empty());
    for children in [vec![11, 11], vec![10], vec![0]] {
        let (mut adapter, state) = adapter();
        add_child(&state, 11, 10);
        state.lock().unwrap().children.insert(10, children);
        assert!(adapter.sample().await.is_err());
        assert!(adapter.executor.calls.is_empty());
    }
}

#[tokio::test]
async fn system_probes_are_explicit_and_do_not_become_process_memory() {
    let (mut adapter, _) = adapter();
    adapter.executor.replies.extend([
        Ok(include_bytes!("../../tests/fixtures/vm_stat.txt").to_vec()),
        Ok(include_bytes!("../../tests/fixtures/swap.txt").to_vec()),
    ]);
    let system = adapter.sample_system().await.unwrap();
    assert_eq!(system.scope, "whole-host");
    assert_eq!(system.vm_stat.unwrap().page_size_bytes, 16384);
    assert_eq!(system.swap.unwrap().used_bytes, 268697600);
    assert_eq!(adapter.executor.calls, [Request::VmStat, Request::Swap]);
}

#[tokio::test]
async fn malformed_system_probe_or_timeout_is_not_zero_or_disguised_success() {
    let (mut adapter, _) = adapter();
    adapter
        .executor
        .replies
        .extend([Ok(b"private-data".to_vec()), Err(ProbeError::TimedOut)]);
    let system = adapter.sample_system().await.unwrap();
    assert_eq!(system.vm_stat, Err(ProbeError::InvalidData));
    assert_eq!(system.swap, Err(ProbeError::TimedOut));
    assert!(!serde_json::to_string(&system)
        .unwrap()
        .contains("private-data"));
}

#[tokio::test]
async fn cancelling_a_pending_probe_drops_it_without_stopping_the_owned_root() {
    let (mut adapter, state) = adapter();
    adapter.executor.pending = true;
    let active = adapter.executor.active.clone();
    let mut future = Box::pin(adapter.sample());
    let mut context = Context::from_waker(Waker::noop());
    assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
    assert_eq!(active.load(Ordering::SeqCst), 1);
    drop(future);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert!(state.lock().unwrap().alive);
    adapter.executor.pending = false;
    assert!(adapter.sample().await.is_ok());
}
