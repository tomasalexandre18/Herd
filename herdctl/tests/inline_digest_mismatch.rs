//! **A verdict whose inline bytes contradict their own digest never exits 0.**
//!
//! The defect this pins was measured, not supposed
//! (`docs/remediation/04b-integrity-and-containment.md` §1). `record_outputs`
//! used to drop the *manifest entry* of a file whose bytes disagreed with
//! their sha256, so the shortfall `herdctl` counts off the manifest
//! (`herdctl/src/main.rs`, `missing`) was zero, `code_for` had nothing to
//! raise, and a requester that asked for two files, got one, and read **0** —
//! the one code that means "everything you asked for happened".
//!
//! These were benches that only printed. They are expectations now (D-NEXT):
//! the figures below fail on the code of 2026-10-24 and pass on the code that
//! keeps the entry and refuses only the bytes.
//!
//! A real runner cannot produce the frame under test — it computes the length
//! and the digest from the same buffer it sends
//! (`herd-proto/src/outputs.rs`) — so the runner here is hand-written,
//! exactly as `herd-hub/tests/link.rs` does for the same reason. `herdctl` is
//! the **real binary**, run as a child, because the exit code is the thing
//! being asserted and only a process has one.

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

/// **The non-regression test.** A passing job whose verdict carries one
/// inline file that contradicts its own sha256, and the exit code the shell
/// would read from `herdctl run --outputs-to`.
///
/// Five things are asserted together because each on its own is a plausible
/// half-fix, and 04b §4.1 names them as the five:
///
/// 1. the record carries **two** manifest entries, not one — the file whose
///    bytes were refused is named, not erased;
/// 2. the `skipped` sentence naming both digests is beside them;
/// 3. `herdctl` exits **2**, because one of two promised files is not here;
/// 4. the honest file still arrives, byte for byte — a fix that loses the
///    good file to protect against the bad one is worse than the defect;
/// 5. the bytes that lied are **not** written anywhere.
///
/// And a sixth, which is this task's other half: the verdict document
/// carries the sentence, so a wrapper never has to parse stderr (D166).
#[tokio::test]
async fn inline_bytes_that_contradict_their_digest_never_exit_zero() {
    let bench = Bench::start().await;
    let mut puppet = Puppet::join(&bench).await;
    let kept = tempfile::tempdir().expect("a temporary output directory");
    let verdict_to = kept.path().join("verdict.json");

    let hub_url = bench.hub_url.clone();
    let kept_path = kept.path().join("outputs");
    let verdict_path = verdict_to.clone();
    let requester = tokio::spawn(async move {
        tokio::process::Command::new(env!("CARGO_BIN_EXE_herdctl"))
            .arg("--hub")
            .arg(&hub_url)
            .arg("run")
            .arg("--outputs-to")
            .arg(&kept_path)
            .arg("--verdict-to")
            .arg(&verdict_path)
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
    let outputs_dir = kept.path().join("outputs");

    assert!(
        matches!(record.state, herd_hub::JobState::Finished { .. }),
        "the hub recorded the verdict: {:?}",
        record.state
    );

    // 1. The manifest names both files. **This is the fix**: the entry is
    //    written down before the bytes are judged, so refusing the bytes
    //    leaves a file that is announced and not held instead of a file that
    //    was never mentioned. Before D-NEXT this was 1.
    let named: Vec<&str> = record
        .outputs
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(
        named,
        vec!["files/results.json", "files/honest.txt"],
        "a file whose bytes this hub refused is still named on the record, \
         or a requester has no way to count it short: {:?}",
        record.outputs.files
    );

    // 2. And the record says why it is not here, naming both digests.
    let why = record
        .outputs
        .skipped
        .iter()
        .find(|sentence| sentence.contains("files/results.json"))
        .unwrap_or_else(|| panic!("the record says why: {:?}", record.outputs.skipped));
    assert!(
        why.contains("a frame this hub will not believe") && why.contains("sha256"),
        "the sentence names the refusal and the two digests: {why}"
    );

    // 3. **The exit code.** One of two promised files is not here, so this
    //    run did part of what it says. Before D-NEXT this was 0.
    assert_eq!(
        code, 2,
        "a requester promised two files and handed one must never read 0; stderr was:\n{said}"
    );

    // 4. The honest file still arrives, byte for byte.
    let honest = std::fs::read(outputs_dir.join("files/honest.txt"))
        .expect("the honest file was written under --outputs-to");
    assert_eq!(
        honest, b"this one is true\n",
        "a fix that loses the good file to protect against the bad one is worse than the defect"
    );

    // 5. And the bytes that lied are nowhere.
    assert!(
        !outputs_dir.join("files/results.json").exists(),
        "bytes this hub refused must not reach the requester's directory"
    );

    // 6. The fact is machine-readable, not only prose on stderr (D166).
    let verdict: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&verdict_to).expect("the verdict document was written"),
    )
    .expect("the verdict document is JSON");
    let document = verdict["said"].to_string();
    assert!(
        document.contains("a frame this hub will not believe"),
        "the verdict document carries what was skipped: {document}"
    );
    assert!(
        document.contains("1 of 2 output file(s) are not here"),
        "the verdict document carries the shortfall: {document}"
    );
    assert_eq!(
        verdict["exit_code_from"], "herd",
        "a 2 is this command's own code and never the job's (D166)"
    );
}

/// The neighbouring case, pinned as it stands: a path the hub will not
/// write keeps **no** manifest entry, and its requester still reads 0.
///
/// **This is not the digest case and must not be fixed by the same change.**
/// The manifest is the list of paths this store joins onto its own
/// directory — a chunk is resolved against it, a file is read back by it —
/// so a path that must never be joined must never be on it. The `skipped`
/// sentence is where that file exists on the record, and that much *is*
/// decided (`herd-hub/src/job.rs`, D131).
///
/// **The exit code is not decided, and that is why it is asserted here.**
/// Nothing in `docs/decisions.md` ever ruled on it: the 0 falls out of the
/// dropped entry, the same way the digest case's 0 did. Pinning it makes a
/// change to it deliberate instead of accidental — if this assertion ever
/// fails, somebody decided something, and the decision belongs in
/// `docs/decisions.md` before the test is edited.
#[tokio::test]
async fn a_path_the_hub_will_not_write_is_named_and_today_costs_no_exit_code() {
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

    assert!(
        matches!(record.state, herd_hub::JobState::Finished { .. }),
        "the hub recorded the verdict: {:?}",
        record.state
    );
    assert!(
        record.outputs.files.is_empty(),
        "an unjoinable path is kept off the manifest on purpose: {:?}",
        record.outputs.files
    );
    assert!(
        record
            .outputs
            .skipped
            .iter()
            .any(|sentence| sentence.contains("a path this hub will not write")
                && sentence.contains("escape.json")),
        "and named on the record instead: {:?}",
        record.outputs.skipped
    );
    assert_eq!(
        status_code(&output),
        0,
        "today, and undecided: see this test's own comment before changing it. stderr was:\n{}",
        stderr(&output)
    );
}
