//! **A job is reported finished only once its output files are on the hub**
//! (D167).
//!
//! The defect this file exists for was measured on the live deployment by the
//! first consumer, who is now its only operator. Job `j1586` was recorded
//! finished at 14:11:43; its files went on arriving until 14:12:40; a
//! `herdctl wait --outputs-to` that fetched on that signal reported **195 of
//! 657** files as *"did NOT get … the hub does not have it"*, and a second
//! `herdctl wait` minutes later got all of them. `j1578` and `j1580` were
//! 268 and 273 files short in the same way. The gap between "finished" and
//! the last output was 55 to 130 seconds, and it was being worked around at
//! a cost per build.
//!
//! The cause is in the design rather than in a slip: a streamed collection
//! **follows** the verdict (D148), and D150 fixed `is_current` precisely so
//! that a file chunk arriving after the verdict is not dropped. So the system
//! expects late arrivals, and the hub relayed `JobFinished` with whatever was
//! on the disk at that instant. "This job finished successfully" and "this
//! job's evidence is on the hub" were two different facts reported as one.
//!
//! **Only a multi-file collection could have shown it**, which is why
//! everything looked green for days: `carriages.rs` streams one 32 MiB file
//! and asserts the bytes land, and with one file the requester's own stall
//! bound happens to behave. So the job here leaves thirty-three files, over
//! the inline threshold in total, and the requester fetches the **instant** it is
//! told the job finished — which is what the consumer's tooling does and what
//! no test did.

use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, ToHub};
use tokio_tungstenite::tungstenite::Message;

mod support;

use support::{shell, Harness, PATIENCE};

/// How many files the job leaves, and how big each one is.
///
/// Thirty-three of a megabyte each: over [`herd_proto::INLINE_OUTPUT_MAX`] in
/// all, so the runner chooses the streamed carriage rather than riding the verdict
/// (D148), and **many** rather than one, because one file is the shape that
/// hid this.
const FILES: usize = 33;
const EACH: u64 = 1 << 20;

const _: () = assert!(FILES as u64 * EACH > herd_proto::INLINE_OUTPUT_MAX);

/// A declared command whose `run` hook leaves `FILES` files of about `EACH`
/// bytes under `{out}`.
///
/// **Declared rather than ad hoc, because `{out}` is a declaration's.** A
/// bare command line gets no exchange directory at all (D107): the runner
/// opens `{in}` and `{out}` only for a planned job, which is why every other
/// test about outputs goes through a declaration too. Every hook runs under
/// the runner's own shell, so the script is the hook.
///
/// The bytes are made by doubling a seed to a megabyte and copying it: a
/// shell loop writing a megabyte a byte at a time would be the slowest part
/// of a test that is otherwise about a link. Nothing names an absolute path
/// but the shell the runner chooses — this host's `/bin` holds `sh` alone, so
/// a script naming `/bin/anything-else` would silently measure nothing.
fn a_command_leaving_files() -> herd_proto::Declaration {
    herd_proto::Declaration {
        command: "leave-evidence".to_owned(),
        commit: "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68".to_owned(),
        args: Vec::new(),
        executor: herd_proto::Executor {
            up: "true".to_owned(),
            fetch: "true".to_owned(),
            run: format!(
                r#"set -e
                   out="{{out}}"
                   seed="$out/.seed"
                   printf '0123456789abcdef' > "$seed"
                   while [ "$(wc -c < "$seed")" -lt {EACH} ]; do
                       cat "$seed" "$seed" > "$seed.next"
                       mv "$seed.next" "$seed"
                   done
                   i=1
                   while [ "$i" -le {FILES} ]; do
                       cp "$seed" "$out/evidence-$i.bin"
                       i=$((i + 1))
                   done
                   rm "$seed"
                   # The requester's own line, which a declared run hook must
                   # carry or the hub refuses the submission: the verdict has
                   # to be the command's and not the hook's.
                   {{line}}"#
            ),
            down: "true".to_owned(),
            env: std::collections::BTreeMap::new(),
        },
    }
}

/// A requester's connection, speaking the protocol by hand.
///
/// `herdctl` cannot stand in for it here. Its fetch waits for a collection
/// that is still crossing and is right to (D150), so a test driving `herdctl`
/// would measure the requester's patience instead of the hub's promise. What
/// is under test is the promise: at the moment the hub says `JobFinished`,
/// is the collection there? So this asks **once**, with no retry, which is
/// also what a requester whose tooling keys off the verdict does.
type Link =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Requester {
    socket: Link,
}

impl Requester {
    async fn join(hub_url: &str) -> Self {
        let (mut socket, _response) = tokio_tungstenite::connect_async(hub_url)
            .await
            .expect("the hub accepts a requester");
        let hello = ToHub::Hello {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            peer_version: herd_runner::runner_version().to_owned(),
            role: Role::Requester,
            token: None,
            name: Some("a-requester-that-fetches-at-once".to_owned()),
            instance: None,
            state_root: None,
            health_every_ms: None,
            capabilities: Default::default(),
            host: herd_proto::HostSize::default(),
            // This stand-in holds no job and says so by not saying (D181).
            jobs_at_once: None,
            preference: 0,
            load_milli: None,
            switch: herd_proto::Switch::On,
            mesh_endpoint: None,
        };
        socket
            .send(Message::text(to_frame(&hello).expect("hello encodes")))
            .await
            .expect("hello is sent");
        let mut joined = Self { socket };
        match joined.next().await {
            Some(FromHub::Welcome { .. }) => joined,
            other => panic!("a requester on this hub's version is welcomed, got {other:?}"),
        }
    }

    async fn say(&mut self, message: ToHub) {
        self.socket
            .send(Message::text(
                to_frame(&message).expect("the frame encodes"),
            ))
            .await
            .expect("the frame is sent");
    }

    async fn next(&mut self) -> Option<FromHub> {
        loop {
            let frame = self.socket.next().await?.expect("the frame is readable");
            if frame.is_close() {
                return None;
            }
            let Ok(text) = frame.to_text() else { continue };
            if text.is_empty() {
                continue;
            }
            return Some(from_frame(text).expect("the hub speaks the protocol"));
        }
    }

    /// Wait for the verdict, collecting the notes said on the way.
    ///
    /// Three times the suite's usual patience, because this wait contains a
    /// real job writing tens of megabytes and a real transfer of them across
    /// a real link — and the whole point of the fix is that the verdict waits
    /// for the second of those. It is still a deadline and not a sleep: a
    /// held verdict that never arrives fails here rather than hanging.
    async fn verdict(&mut self) -> (FromHub, Vec<String>) {
        let deadline = Instant::now() + 3 * PATIENCE;
        let mut notes = Vec::new();
        loop {
            assert!(
                Instant::now() < deadline,
                "no verdict arrived; notes: {notes:?}"
            );
            match self.next().await {
                Some(FromHub::JobNote { note, .. }) => notes.push(note),
                Some(verdict @ FromHub::JobFinished { .. }) => return (verdict, notes),
                Some(_) => continue,
                None => panic!("the hub closed the link with no verdict; notes: {notes:?}"),
            }
        }
    }

    /// Ask for every file of a collection, **once**, and report what the hub
    /// said was absent.
    async fn fetch_once(&mut self, job_id: &JobId) -> Vec<herd_proto::AbsentFile> {
        self.say(ToHub::AskJobFiles {
            job_id: job_id.clone(),
            paths: Vec::new(),
        })
        .await;
        loop {
            match self.next().await {
                Some(FromHub::JobFile { .. }) => continue,
                Some(FromHub::JobFilesSent { absent, .. }) => return absent,
                Some(other) => panic!("a fetch is answered with files, got {other:?}"),
                None => panic!("the hub closed the link mid-fetch"),
            }
        }
    }
}

/// **The defect, and the whole point of this file.** A requester that fetches
/// the moment it is told its job finished gets every file.
///
/// Against the unfixed hub this reports the collection as absent — the
/// verdict goes out before a single chunk of it has been written, so the
/// first fetch finds an empty `files/` and the assertion names how many of
/// the thirty-three were missing. It is the field failure reproduced:
/// `195 of 657` with smaller figures.
///
/// Three things are asserted, because a fix that bought one by losing
/// another would be worse than the defect:
///
/// * **nothing is absent** on the first fetch — the promise itself;
/// * the job's **record is terminal** by the time the verdict arrives, which
///   is what frees its slot on the runner (D79: a dispatch pass reads
///   occupancy off the records and from nothing else). Holding the record
///   back instead would have kept a one-slot machine idle for the minute the
///   bytes take, once per job;
/// * the wait **says why it is waiting** while it waits (E-04's precedent),
///   so sixty seconds of files is sixty seconds of explanation rather than
///   silence after a job the requester watched finish.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_job_is_finished_only_once_its_collection_is_whole() {
    let harness = Harness::with_hub_and_runner().await;
    let job_id = herdctl::submit_job(
        &harness.requester(),
        vec!["true".to_owned()],
        herd_proto::LabelSet::default(),
        herd_proto::Priority::Step,
        herdctl::Declared {
            declaration: Some(a_command_leaving_files()),
            ..herdctl::Declared::default()
        },
    )
    .await
    .expect("the submission is accepted");

    let mut requester = Requester::join(&harness.hub_url).await;
    requester
        .say(ToHub::WaitForJob {
            job_id: job_id.clone(),
            // This requester is going to fetch, which is what asks the hub
            // to hold the verdict for it (D167).
            wants_files: true,
        })
        .await;
    let (verdict, notes) = requester.verdict().await;
    let FromHub::JobFinished {
        outcome, outputs, ..
    } = &verdict
    else {
        unreachable!("verdict() returns only a verdict");
    };
    assert_eq!(
        outcome,
        &herd_proto::JobOutcome::Exited { code: 0 },
        "the job itself passed; notes: {notes:?}"
    );
    assert!(
        outputs.streamed,
        "{FILES} files of {EACH} bytes are over the inline threshold, so this is the \
         streamed carriage and not the one that rides the verdict"
    );
    assert_eq!(
        outputs.files.len(),
        FILES,
        "the manifest names every file the job left"
    );

    // The record is already over, which is what releases the runner's slot.
    assert!(
        matches!(
            harness.state_of(&job_id),
            herd_hub::JobState::Finished { .. }
        ),
        "the end is recorded before the verdict is handed over, not after: a held verdict \
         must never hold a build slot (D167)"
    );

    // **The assertion.** One fetch, no retry, right now.
    let absent = requester.fetch_once(&job_id).await;
    assert!(
        absent.is_empty(),
        "a requester that fetched the moment it was told {job_id} finished did NOT get {} of \
         {FILES} file(s): {:?}. That is the defect — a verdict reported while the \
         collection was still crossing.",
        absent.len(),
        absent
            .iter()
            .map(|gone| format!("{} ({})", gone.path, gone.why))
            .collect::<Vec<_>>(),
    );

    // And the wait explained itself rather than going quiet. One line is
    // enough to assert the shape: the cadence itself is E-04's and is tested
    // where that lives.
    assert!(
        notes.iter().any(|note| note.contains("still arriving")
            && note.contains("output file(s)")
            && note.contains("test-runner")),
        "a requester waiting on files is told what it is waiting for and from where: \
         {notes:?}"
    );
}

/// **And a collection that cannot complete ends the wait honestly** (D167).
///
/// The hold must not be able to last for ever. A runner that dies
/// mid-collection, a link that drops, a file that never arrives — none of
/// them may leave a `wait` hanging with nothing said, which would be a worse
/// defect than the one being fixed: the consumer would have traded a short
/// directory for a build that never returns.
///
/// So this is a runner that announces a streamed collection, sends the
/// verdict, and then **goes away without sending a byte of it**. The hub
/// holds the verdict, finds the machine that owes the bytes gone, and hands
/// the verdict over with the shortfall named — and the fetch that follows
/// names the file off the manifest that outlives the bytes (D148).
///
/// Driven by hand rather than by the real runner because a real runner
/// delivers its collection: the failure under test is the machine not coming
/// back, and the honest way to stage that is a peer that does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_collection_that_cannot_complete_ends_the_wait_and_names_what_is_missing() {
    // Short enough that the test does not sit out a deployment's figure, and
    // through the hub's own clock rather than a copy of it.
    let harness = Harness::with_hub_timed(herd_hub::Policy {
        still_there_every: Duration::from_millis(100),
        collection_silence: Duration::from_millis(200),
        ..herd_hub::Policy::default()
    })
    .await;
    let token = harness.approve("a-runner-that-goes-away");
    let mut runner = Requester {
        socket: {
            let (mut socket, _response) = tokio_tungstenite::connect_async(&harness.hub_url)
                .await
                .expect("the hub accepts a runner");
            let hello = ToHub::Hello {
                protocol_version: herd_proto::PROTOCOL_VERSION,
                peer_version: herd_runner::runner_version().to_owned(),
                role: Role::Runner,
                token: Some(token),
                name: Some("a-runner-that-goes-away".to_owned()),
                instance: Some("one-run".to_owned()),
                state_root: None,
                health_every_ms: None,
                capabilities: Default::default(),
                host: herd_proto::HostSize {
                    memory_mib: Some(8192),
                    threads: Some(4),
                    memory_floor_mib: Some(1024),
                    tmpfs_mib: Some(4096),
                },
                // This stand-in holds no job and says so by not saying (D181).
                jobs_at_once: None,
                preference: 0,
                load_milli: None,
                switch: herd_proto::Switch::On,
                mesh_endpoint: None,
            };
            socket
                .send(Message::text(to_frame(&hello).expect("hello encodes")))
                .await
                .expect("hello is sent");
            socket
        },
    };
    match runner.next().await {
        Some(FromHub::Welcome { .. }) => {}
        other => panic!("an approved runner is welcomed, got {other:?}"),
    }

    let job_id = support::submit_job(&harness.requester(), shell("true"))
        .await
        .expect("the submission is accepted");
    let mut requester = Requester::join(&harness.hub_url).await;
    requester
        .say(ToHub::WaitForJob {
            job_id: job_id.clone(),
            // This requester is going to fetch, which is what asks the hub
            // to hold the verdict for it (D167).
            wants_files: true,
        })
        .await;

    let attempt = loop {
        match runner.next().await {
            Some(FromHub::AssignJob { attempt, .. }) => break attempt,
            Some(_) => continue,
            None => panic!("the hub never handed this runner the job"),
        }
    };
    // A collection announced and never sent. The digest is never checked,
    // because no file ever reaches its declared length — which is exactly
    // the state a runner that died mid-transfer leaves behind.
    const MISSING: &str = "evidence/screenshot-1.png";
    runner
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt,
            outcome: herd_proto::JobOutcome::Exited { code: 0 },
            chunks_sent: 0,
            metrics: herd_proto::JobMetrics::default(),
            steps: Vec::new(),
            outputs: herd_proto::JobOutputs {
                files: vec![herd_proto::OutputFile::announced(
                    MISSING.to_owned(),
                    4096,
                    "0".repeat(64),
                )],
                refused: None,
                skipped: Vec::new(),
                streamed: true,
            },
        })
        .await;
    // The hub acknowledges the result; then the machine goes away, holding
    // every byte of the collection it promised.
    loop {
        match runner.next().await {
            Some(FromHub::JobResultAck { .. }) => break,
            Some(_) => continue,
            None => panic!("the hub never acknowledged the result"),
        }
    }
    drop(runner);

    // **The wait ends**, inside this test's patience rather than never.
    let (verdict, notes) = requester.verdict().await;
    let FromHub::JobFinished { outputs, .. } = &verdict else {
        unreachable!("verdict() returns only a verdict");
    };
    assert_eq!(
        outputs.files.len(),
        1,
        "the manifest outlives the bytes: a record that had a file says so (D148)"
    );
    assert!(
        notes
            .iter()
            .any(|note| note.contains("did not all arrive")
                && note.contains("a-runner-that-goes-away")),
        "and the requester is told why the wait ended, naming the machine that owed the \
         bytes: {notes:?}"
    );

    // And the fetch names the file rather than handing over a short
    // collection with no word about it.
    let absent = requester.fetch_once(&job_id).await;
    assert_eq!(absent.len(), 1, "{absent:?}");
    assert_eq!(absent[0].path, MISSING);
    assert_eq!(
        absent[0].len, 4096,
        "with the size the record promised, so a reader knows what is gone"
    );
}

/// **And a requester that wants no files does not pay for the wait** (D167).
///
/// The first shape of this fix held every verdict, whoever was following,
/// because the hub cannot guess which requester is going to fetch. That put
/// up to the minute or two the bytes take onto a `herdctl run` with no
/// `--outputs-to` — a requester that receives no byte of the collection and
/// never asked about it — and broke D148's promise that such a run "exits on
/// the verdict and waits for nothing". So the requester says which it is, and
/// the hub holds the verdict only for the followers that answered yes.
///
/// Two followers on one job, answering differently, is the assertion that
/// could not be made any other way: the one that wants files is still
/// waiting at the moment the other one has its verdict. Both of them are
/// right, which is the whole point of the field.
///
/// The frame's default is the other half, and it is asserted where it lives,
/// in `herd-proto`'s round trip: a requester that does not mention the field
/// is read as wanting them, so an older `herdctl` keeps the safe behaviour
/// rather than being handed the early verdict by the upgrade that fixed the
/// defect.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_requester_that_wants_no_files_is_told_at_once() {
    let harness = Harness::with_hub_and_runner().await;
    let job_id = herdctl::submit_job(
        &harness.requester(),
        vec!["true".to_owned()],
        herd_proto::LabelSet::default(),
        herd_proto::Priority::Step,
        herdctl::Declared {
            declaration: Some(a_command_leaving_files()),
            ..herdctl::Declared::default()
        },
    )
    .await
    .expect("the submission is accepted");

    // The one that will fetch, and the one that will not.
    let mut fetching = Requester::join(&harness.hub_url).await;
    fetching
        .say(ToHub::WaitForJob {
            job_id: job_id.clone(),
            wants_files: true,
        })
        .await;
    let mut only_the_verdict = Requester::join(&harness.hub_url).await;
    only_the_verdict
        .say(ToHub::WaitForJob {
            job_id: job_id.clone(),
            wants_files: false,
        })
        .await;

    // The verdict reaches the one that asked for no files…
    let (verdict, _) = only_the_verdict.verdict().await;
    let FromHub::JobFinished { outcome, .. } = &verdict else {
        unreachable!("verdict() returns only a verdict");
    };
    assert_eq!(outcome, &herd_proto::JobOutcome::Exited { code: 0 });

    // …while the collection is still crossing, which is the measurement: the
    // other requester has had no verdict yet, and the hub says what it is
    // waiting for. Judged off the hub's own state rather than a sleep — if
    // the collection had already finished there would be nothing to prove,
    // so a run that gets here with it complete says so instead of passing
    // quietly.
    let record = herd_hub::JobStore::job(&*harness.store, &job_id)
        .expect("the store reads")
        .expect("the hub knows this job");
    let whole = herd_hub::JobStore::collection_owed(&*harness.store, &job_id)
        .expect("the store reads")
        .is_none();
    assert!(
        record.outputs.streamed,
        "the collection has to be the streamed carriage for this to mean anything"
    );
    assert!(
        !whole,
        "this test needs the collection to be still crossing when the early \
         verdict lands, and it was already whole: nothing was measured"
    );

    // And the fetching requester is answered in the end, with everything.
    let (_, notes) = fetching.verdict().await;
    assert!(
        notes.iter().any(|note| note.contains("still arriving")),
        "the one that waited was told what it was waiting for: {notes:?}"
    );
    let absent = fetching.fetch_once(&job_id).await;
    assert!(absent.is_empty(), "{absent:?}");
}
