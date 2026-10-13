#![allow(dead_code)]
//! Scaffolding the hub's end-to-end tests share.
//!
//! Two shapes of hub live here. [`Harness`] runs one in the test's own
//! runtime, which is all a test of behaviour needs. [`HubProcess`] runs one on
//! its own thread and its own runtime, so that it can be *killed*: dropping
//! that runtime takes every connection it was serving with it, which is as
//! close to "the container stopped" as a test gets, and nothing of the old hub
//! is left to quietly finish the work.
//!
//! Everything binds port 0 and discovers the port it got, so the suite never
//! collides with a port in use. Every task is aborted and every temporary
//! directory removed when its owner drops, so no test leaves a process, a
//! bound port or a file behind.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use herd_hub::{Hub, HubHandle, JobDirStore, JobStore};
use herd_proto::JobId;
pub use herd_runner::testing::TempDir as DataRootDir;
use tempfile::TempDir;
use tokio::task::JoinHandle;

/// The confinement a runner in these tests gets: a throwaway data root, its
/// own, and M4's defaults for every cap.
///
/// A runner locks its data root for the life of the process (E-45), so every
/// runner a test starts needs one of its own; and the caps have to be out of
/// the way of the trivial commands these tests run, which M4's defaults are.
/// What the caps do when they are reached is asserted in
/// `herd-runner/tests/confinement.rs`.
pub fn confinement(data_root: &DataRootDir) -> herd_runner::Confinement {
    herd_runner::Confinement::with_data_root(data_root.path())
}

/// A data root and the confinement prepared on it, for a test that drives
/// [`herd_runner::serve_once`] itself rather than letting
/// [`herd_runner::run`] redial.
///
/// Prepared once and shared by every connection that test makes, because that
/// is what a real runner does: the data root is locked for the life of the
/// process, so preparing it again for the second connection would refuse
/// itself (E-45). The caller keeps the directory alive; dropping it removes
/// the root.
pub fn prepared_runner(label: &str) -> (DataRootDir, Arc<herd_runner::Confined>) {
    let data_root = DataRootDir::new(label);
    let confined = herd_runner::Confined::prepare(confinement(&data_root))
        .expect("a throwaway data root is confinable");
    (data_root, Arc::new(confined))
}

/// How long any wait in these tests may take before it is a failure. Generous
/// because CI is slow, never a substitute for waiting on a real condition.
pub const PATIENCE: Duration = Duration::from_secs(20);

/// A command that runs the same way everywhere a POSIX shell exists.
pub fn shell(script: &str) -> Vec<String> {
    vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()]
}

/// The capabilities a job needs, as a canonical set.
pub fn labels(names: &[&str]) -> herd_proto::LabelSet {
    herd_proto::LabelSet::new(names.iter().map(|n| (*n).to_owned())).expect("valid labels")
}

/// Submit a job that needs no capability in particular, and follow it.
///
/// Most of this suite is about things other than labels, so it goes through
/// the same `run_job` a caller uses with an empty label set and nothing
/// listening for notes. The tests that *are* about labels call `run_job`
/// themselves.
pub async fn run_job(
    config: &herdctl::Config,
    command: Vec<String>,
    on_log: impl FnMut(herd_proto::LogStream, &[u8]),
) -> anyhow::Result<herdctl::Finished> {
    herdctl::run_job(
        config,
        command,
        herd_proto::LabelSet::default(),
        on_log,
        |_| {},
    )
    .await
}

/// Submit a job that needs no capability in particular, and leave with its id.
pub async fn submit_job(
    config: &herdctl::Config,
    command: Vec<String>,
) -> anyhow::Result<herd_proto::JobId> {
    herdctl::submit_job(config, command, herd_proto::LabelSet::default()).await
}

/// A hub, optionally a runner, and the means to reach them.
pub struct Harness {
    pub hub_url: String,
    pub store: Arc<JobDirStore>,
    /// Removed when the harness drops, so the suite writes nothing lasting.
    pub state: TempDir,
    handle: HubHandle,
    tasks: Vec<JoinHandle<()>>,
    /// The data root of the runner this harness started, removed with it.
    _data_root: Option<DataRootDir>,
}

impl Harness {
    /// A hub on a free loopback port, with nothing connected to it yet.
    pub async fn with_hub() -> Self {
        let harness = Self::start().await;
        assert!(
            !harness.runner_joined(),
            "nothing is connected to a hub that has just started"
        );
        harness
    }

    /// A hub with one runner joined and ready to be dispatched to.
    pub async fn with_hub_and_runner() -> Self {
        let mut harness = Self::start().await;
        let data_root = DataRootDir::new("hub-end-to-end");
        let config = herd_runner::Config {
            hub_url: harness.hub_url.clone(),
            name: Some("test-runner".to_owned()),
            // Carried, not checked: M15 gives tokens meaning.
            token: Some("not-validated-yet".to_owned()),
            confinement: confinement(&data_root),
            ..herd_runner::Config::default()
        };
        harness.tasks.push(tokio::spawn(async move {
            let _ = herd_runner::run(config).await;
        }));
        harness._data_root = Some(data_root);
        // Wait on the hub's own state, not on a sleep: a submission is only
        // meaningful once the runner is registered.
        let deadline = Instant::now() + PATIENCE;
        while !harness.runner_joined() {
            assert!(Instant::now() < deadline, "the runner never joined the hub");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        harness
    }

    async fn start() -> Self {
        let state = tempfile::tempdir().expect("a temporary state directory");
        let store = Arc::new(JobDirStore::open(state.path()).expect("the state root opens"));
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::clone(&store) as Arc<dyn JobStore>,
        )
        .await
        .expect("the hub binds a free port");
        let hub_url = format!("ws://{}", hub.local_addr());
        let handle = hub.handle();
        let serving = tokio::spawn(async move {
            let _ = hub.serve().await;
        });
        Self {
            hub_url,
            store,
            state,
            handle,
            tasks: vec![serving],
            _data_root: None,
        }
    }

    /// Whether a runner has finished its handshake with this hub.
    pub fn runner_joined(&self) -> bool {
        self.handle.runner_connected()
    }

    /// A hub with one runner joined, offering what `capabilities` says and
    /// reading its probes on the interval `probes` gives (M5).
    pub async fn with_hub_and_runner_offering(
        capabilities: herd_proto::CapabilityOffer,
        probes: herd_runner::capability::ProbeSettings,
    ) -> Self {
        let mut harness = Self::start().await;
        let data_root = DataRootDir::new("hub-capabilities");
        let config = herd_runner::Config {
            hub_url: harness.hub_url.clone(),
            name: Some("test-runner".to_owned()),
            confinement: confinement(&data_root),
            capabilities,
            probes,
            ..herd_runner::Config::default()
        };
        harness.tasks.push(tokio::spawn(async move {
            let _ = herd_runner::run(config).await;
        }));
        harness._data_root = Some(data_root);
        let deadline = Instant::now() + PATIENCE;
        while !harness.runner_joined() {
            assert!(Instant::now() < deadline, "the runner never joined the hub");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        harness
    }

    /// What the hub recorded about the runner's capabilities (M5, E-24).
    pub fn runner_capacity(&self) -> Option<herd_hub::RunnerCapacity> {
        self.handle.runner_capacity()
    }

    /// What the hub itself believes became of a job.
    pub fn state_of(&self, job_id: &JobId) -> herd_hub::JobState {
        self.store
            .job(job_id)
            .expect("the store reads")
            .expect("the hub knows this job")
            .state
    }

    pub fn requester(&self) -> herdctl::Config {
        herdctl::Config {
            hub_url: self.hub_url.clone(),
            token: Some("not-validated-yet".to_owned()),
            name: Some("test-requester".to_owned()),
        }
    }

    /// Where the hub's files are, for a test that reads them as the first
    /// consumer would.
    pub fn state_dir(&self) -> &Path {
        self.state.path()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// A hub on its own runtime, which can be killed outright.
pub struct HubProcess {
    pub hub_url: String,
    pub addr: SocketAddr,
    handle: HubHandle,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl HubProcess {
    /// Start a hub on the state root at `state_dir`, recovering whatever is
    /// already there.
    ///
    /// The port is whatever is free: a restarted hub keeps its address in
    /// reality, but nothing about durability depends on the number, and a test
    /// that re-binds a port it has just released is a test that fails for
    /// reasons of its own.
    pub fn start(state_dir: &Path) -> Self {
        let (ready, started) = mpsc::channel();
        let (stop, stopping) = mpsc::channel::<()>();
        let state_dir: PathBuf = state_dir.to_path_buf();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime for the hub");
            runtime.block_on(async move {
                let store = open_when_free(&state_dir);
                let hub = Hub::bind(
                    "127.0.0.1:0".parse().expect("a literal loopback address"),
                    Arc::new(store),
                )
                .await
                .expect("the hub binds a free port");
                ready
                    .send((hub.local_addr(), hub.handle()))
                    .expect("the test is still waiting");
                let serving = tokio::spawn(async move {
                    let _ = hub.serve().await;
                });
                let asked_to_stop = tokio::task::spawn_blocking(move || {
                    let _ = stopping.recv();
                });
                tokio::select! {
                    _ = serving => {}
                    _ = asked_to_stop => {}
                }
            });
            // Every task this hub spawned — the listener and every connection
            // it was serving — goes with the runtime. Nothing of this hub
            // survives to record anything after this point.
            runtime.shutdown_background();
        });
        let (addr, handle) = started
            .recv_timeout(PATIENCE)
            .expect("the hub reports the port it bound");
        Self {
            hub_url: format!("ws://{addr}"),
            addr,
            handle,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    /// Wait until a runner has joined, so a submission has somewhere to go.
    pub async fn wait_for_runner(&self) {
        let deadline = Instant::now() + PATIENCE;
        while !self.handle.runner_connected() {
            assert!(Instant::now() < deadline, "the runner never joined the hub");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    pub fn requester(&self) -> herdctl::Config {
        herdctl::Config {
            hub_url: self.hub_url.clone(),
            token: None,
            name: Some("test-requester".to_owned()),
        }
    }

    /// Kill the hub and wait for it to be gone.
    pub fn kill(mut self) {
        self.stop_now();
    }

    fn stop_now(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for HubProcess {
    fn drop(&mut self) {
        self.stop_now();
    }
}

/// Open a state root, waiting for whoever held it to let go.
///
/// One hub owns a state root at a time (D21). A real restart is a new process
/// and the kernel has already dropped the dead one's lock; two hubs on one
/// runtime in one test process are not that, and the lock is given back only
/// once the tasks of the hub that was killed have finished being dropped. So
/// this waits for it, rather than racing it.
///
/// **Every** `JobDirStore::open` in the suite goes through here, not only the
/// restarts. One test opened a root directly and failed once in CI with "the
/// state root is already in use by herd-hub, process <itself>" — its own
/// lock, not yet handed back. A flaky suite erodes invariant 5 faster than a
/// missing test does, so the fix is the wait, never a rerun.
pub fn open_when_free(state_dir: &Path) -> JobDirStore {
    let deadline = Instant::now() + PATIENCE;
    loop {
        match JobDirStore::open(state_dir) {
            Ok(store) => return store,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "{} never became free: {error}",
                    state_dir.display()
                );
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

/// Poll `condition` until it holds, or fail saying what was being waited for.
pub async fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A job's directory, as a reader outside Herd would find it.
pub fn job_dir(state_dir: &Path, job_id: &JobId) -> PathBuf {
    state_dir.join("jobs").join(job_id.as_str())
}

/// A job's record, read from its file and not from the hub. This is the first
/// consumer's view (E-15, E-53): plain JSON, no tool but a file read.
pub fn record_of(state_dir: &Path, job_id: &JobId) -> serde_json::Value {
    let path = job_dir(state_dir, job_id).join("job.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} reads: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is json: {e}", path.display()))
}

/// The state in a record read from the file, as the file spells it.
pub fn file_state_of(state_dir: &Path, job_id: &JobId) -> String {
    record_of(state_dir, job_id)["state"]["type"]
        .as_str()
        .expect("a state has a type")
        .to_owned()
}

/// A job's log, read from its file.
pub fn log_of(state_dir: &Path, job_id: &JobId) -> String {
    let path = job_dir(state_dir, job_id).join("log");
    std::fs::read_to_string(&path).unwrap_or_default()
}

/// A job's transitions, read from its file, in the order they are written.
pub fn events_of(state_dir: &Path, job_id: &JobId) -> Vec<serde_json::Value> {
    let path = job_dir(state_dir, job_id).join("events.jsonl");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{line} is json: {e}")))
        .collect()
}
