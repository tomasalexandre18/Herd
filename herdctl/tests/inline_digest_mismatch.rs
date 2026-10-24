//! **Experiment 6.3 (a): what a requester's exit code says when a verdict's
//! inline bytes contradict their own digest.**
//!
//! The audit (`docs/remediation/03a-identity-and-storage.md` §5.2) predicted
//! that `herdctl` exits **0** here: `record_outputs` drops the manifest entry
//! whose bytes disagree with their digest (`herd-hub/src/store.rs:1711-1716`)
//! instead of keeping the entry and naming the absence, so the shortfall
//! `herdctl` counts off the manifest (`herdctl/src/main.rs:1283`) is zero and
//! `code_for` (`herdctl/src/main.rs:591-596`) has nothing to raise.
//!
//! These are **bench files, not an expectation Herd commits to**. They
//! measure what the code does today and are named so a reader cannot mistake
//! them for a contract: `measure_*`. Nothing here is fixed, because fixing it
//! is a product change that is not this task's.
//!
//! A real runner cannot produce the frame under test — it computes the length
//! and the digest from the same buffer it sends
//! (`herd-proto/src/outputs.rs:213-220`) — so the runner here is hand-written,
//! exactly as `herd-hub/tests/link.rs` does for the same reason. `herdctl` is
//! the **real binary**, run as a child, because the exit code is the thing
//! being measured and only a process has one.

use std::process::Output;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use herd_hub::{Hub, JobDirStore, JobStore};
use herd_proto::{
    from_frame, to_frame, FromHub, JobId, JobMetrics, JobOutcome, JobOutputs, OutputFile, Role,
    ToHub, PROTOCOL_VERSION,
};
use tempfile::TempDir;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// How long any wait here may take before it is a failure.
const PATIENCE: Duration = Duration::from_secs(30);

/// The one name the hand-written runner dials under.
const RUNNER: &str = "digest-bench-runner";

/// A hub on loopback with no real runner, and the means to approve one.
struct Bench {
    hub_url: String,
    store: Arc<JobDirStore>,
    _state: TempDir,
    serving: tokio::task::JoinHandle<()>,
}

impl Bench {
    async fn start() -> Self {
        let state = tempfile::tempdir().expect("a temporary state directory");
        let store = Arc::new(open_when_free(state.path()));
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::clone(&store) as Arc<dyn JobStore>,
        )
        .await
        .expect("the hub binds a free port");
        let hub_url = format!("ws://{}", hub.local_addr());
        let serving = tokio::spawn(async move {
            let _ = hub.serve().await;
        });
        Self {
            hub_url,
            store,
            _state: state,
            serving,
        }
    }

    fn token(&self) -> String {
        self.store
            .approve_runner(RUNNER)
            .expect("the state root mints a token")
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

/// Open a state root, waiting for whoever holds it to let go: the reason is
/// written out in `herdctl/tests/exit_code.rs`, and it is the same here.
fn open_when_free(root: &std::path::Path) -> JobDirStore {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match JobDirStore::open(root) {
            Ok(store) => return store,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "{} never came free: {error}",
                    root.display()
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A runner that says exactly what this file tells it to.
struct Puppet {
    socket: Socket,
}

impl Puppet {
    async fn join(bench: &Bench) -> Self {
        let (mut socket, _response) = tokio_tungstenite::connect_async(&bench.hub_url)
            .await
            .expect("the hub accepts a connection");
        let hello = ToHub::Hello {
            protocol_version: PROTOCOL_VERSION,
            peer_version: herd_runner::runner_version().to_owned(),
            role: Role::Runner,
            token: Some(bench.token()),
            name: Some(RUNNER.to_owned()),
            instance: Some(format!("run-{RUNNER}")),
            state_root: None,
            health_every_ms: None,
            capabilities: Default::default(),
            host: herd_proto::HostSize::default(),
            preference: 0,
            load_milli: None,
            switch: herd_proto::Switch::On,
        };
        socket
            .send(Message::text(to_frame(&hello).expect("encodes")))
            .await
            .expect("hello goes out");
        let mut puppet = Self { socket };
        match puppet.next().await {
            Some(FromHub::Welcome { .. }) => puppet,
            other => panic!("the hub answered hello with {other:?}"),
        }
    }

    async fn next(&mut self) -> Option<FromHub> {
        loop {
            match tokio::time::timeout(PATIENCE, self.socket.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    return Some(from_frame(text.as_str()).expect("the hub speaks the protocol"))
                }
                Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
                _ => return None,
            }
        }
    }

    async fn say(&mut self, message: ToHub) {
        self.socket
            .send(Message::text(to_frame(&message).expect("encodes")))
            .await
            .expect("the frame goes out");
    }

    async fn take_a_job(&mut self) -> (JobId, u32) {
        loop {
            match self.next().await {
                Some(FromHub::AssignJob {
                    job_id, attempt, ..
                }) => return (job_id, attempt),
                Some(_) => continue,
                None => panic!("the hub never handed this runner a job"),
            }
        }
    }
}

/// One output file whose bytes do **not** hash to the digest beside them.
///
/// Built by taking an honest file and swapping its bytes for others of the
/// same length, so the only thing wrong with the frame is the digest — the
/// length check in `OutputFile::disagreement` is not what trips.
fn a_file_that_lies_about_itself() -> OutputFile {
    let honest = OutputFile::collected("files/results.json".to_owned(), b"{\"ok\":true}".to_vec());
    OutputFile {
        data: Some(b"{\"ok\":fals}".to_vec()),
        ..honest
    }
}

/// The same collection with an honest file beside the lying one, so the
/// measurement can say whether the honest one still arrives.
fn a_collection_with_one_lie() -> JobOutputs {
    JobOutputs {
        files: vec![
            a_file_that_lies_about_itself(),
            OutputFile::collected(
                "files/honest.txt".to_owned(),
                b"this one is true\n".to_vec(),
            ),
        ],
        refused: None,
        skipped: Vec::new(),
        streamed: false,
    }
}

fn status_code(output: &Output) -> i32 {
    output
        .status
        .code()
        .unwrap_or_else(|| panic!("herdctl itself was killed: {:?}", output.status))
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// **The measurement.** A passing job whose verdict carries one inline file
/// that contradicts its own sha256, and the exit code the shell would read
/// from `herdctl run --outputs-to`.
#[tokio::test]
async fn measure_the_exit_code_when_inline_bytes_contradict_their_digest() {
    let bench = Bench::start().await;
    let mut puppet = Puppet::join(&bench).await;
    let kept = tempfile::tempdir().expect("a temporary output directory");

    let hub_url = bench.hub_url.clone();
    let kept_path = kept.path().to_path_buf();
    let requester = tokio::spawn(async move {
        tokio::process::Command::new(env!("CARGO_BIN_EXE_herdctl"))
            .arg("--hub")
            .arg(&hub_url)
            .arg("run")
            .arg("--outputs-to")
            .arg(&kept_path)
            .arg("--")
            .arg("/bin/sh")
            .arg("-c")
            .arg("exit 0")
            .output()
            .await
            .expect("herdctl runs")
    });

    let (job_id, attempt) = puppet.take_a_job().await;
    puppet
        .say(ToHub::JobStarted {
            job_id: job_id.clone(),
            attempt,
        })
        .await;
    puppet
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: JobOutcome::Exited { code: 0 },
            chunks_sent: 0,
            metrics: JobMetrics::default(),
            steps: Vec::new(),
            outputs: a_collection_with_one_lie(),
        })
        .await;

    let output = tokio::time::timeout(PATIENCE, requester)
        .await
        .expect("herdctl answers inside the patience")
        .expect("the requester task did not panic");

    let code = status_code(&output);
    let said = stderr(&output);
    let record = bench
        .store
        .job(&job_id)
        .expect("the store reads")
        .expect("the hub knows this job");
    let on_disk: Vec<String> = std::fs::read_dir(kept.path())
        .expect("the output directory reads")
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
        .collect();

    // Printed rather than asserted on: this file measures, it does not pin.
    // The figures a reader of 04b needs are all here, in one place.
    println!("--- experiment 6.3 (a) ---");
    println!("herdctl exit code: {code}");
    println!(
        "manifest entries on the record: {}",
        record.outputs.files.len()
    );
    println!("record manifest: {:?}", record.outputs.files);
    println!("record skipped: {:?}", record.outputs.skipped);
    println!("top level of --outputs-to: {on_disk:?}");
    println!("herdctl stderr:\n{said}");

    // The one thing this asserts is that the measurement happened at all:
    // the hub took the verdict and the requester came back. Everything the
    // experiment is about is in the printed figures, which 04b quotes.
    assert!(
        matches!(record.state, herd_hub::JobState::Finished { .. }),
        "the hub recorded the verdict: {:?}",
        record.state
    );
}

/// The control: the **path** case, which the design states as intentional
/// (`herd-hub/src/job.rs:1823-1826`). Measured beside the digest case so 04b
/// can say whether the two really are the same code path and the same code.
#[tokio::test]
async fn measure_the_exit_code_when_a_path_is_one_the_hub_will_not_write() {
    let bench = Bench::start().await;
    let mut puppet = Puppet::join(&bench).await;
    let kept = tempfile::tempdir().expect("a temporary output directory");

    let hub_url = bench.hub_url.clone();
    let kept_path = kept.path().to_path_buf();
    let requester = tokio::spawn(async move {
        tokio::process::Command::new(env!("CARGO_BIN_EXE_herdctl"))
            .arg("--hub")
            .arg(&hub_url)
            .arg("run")
            .arg("--outputs-to")
            .arg(&kept_path)
            .arg("--")
            .arg("/bin/sh")
            .arg("-c")
            .arg("exit 0")
            .output()
            .await
            .expect("herdctl runs")
    });

    let (job_id, attempt) = puppet.take_a_job().await;
    puppet
        .say(ToHub::JobStarted {
            job_id: job_id.clone(),
            attempt,
        })
        .await;
    puppet
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: JobOutcome::Exited { code: 0 },
            chunks_sent: 0,
            metrics: JobMetrics::default(),
            steps: Vec::new(),
            outputs: JobOutputs {
                files: vec![OutputFile::collected(
                    "../escape.json".to_owned(),
                    b"{\"ok\":true}".to_vec(),
                )],
                refused: None,
                skipped: Vec::new(),
                streamed: false,
            },
        })
        .await;

    let output = tokio::time::timeout(PATIENCE, requester)
        .await
        .expect("herdctl answers inside the patience")
        .expect("the requester task did not panic");

    let record = bench
        .store
        .job(&job_id)
        .expect("the store reads")
        .expect("the hub knows this job");

    println!("--- experiment 6.3 (a), control: a refused path ---");
    println!("herdctl exit code: {}", status_code(&output));
    println!(
        "manifest entries on the record: {}",
        record.outputs.files.len()
    );
    println!("record skipped: {:?}", record.outputs.skipped);
    println!("herdctl stderr:\n{}", stderr(&output));

    assert!(
        matches!(record.state, herd_hub::JobState::Finished { .. }),
        "the hub recorded the verdict: {:?}",
        record.state
    );
}
