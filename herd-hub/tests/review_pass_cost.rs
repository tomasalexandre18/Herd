//! Review bench for `docs/remediation/05-experiments-review.md`.
//!
//! It measures what 04a's `junction` column leaves out, and it checks the
//! premise of 04a's recommendation 1 against the store. It asserts nothing
//! about a product behaviour and is `#[ignore]`d, like `cost_surface.rs`.
//!
//! ```sh
//! cargo test -p herd-hub --test review_pass_cost --release -- \
//!   --ignored --nocapture --test-threads=1
//! ```
//!
//! Three questions:
//!
//! 1. **The write side of a pass.** `dispatch_a_pass` ends in a loop that
//!    calls `say_it_waits` for every job it leaves queued
//!    (`herd-hub/src/server.rs:2619-2640`), and `say_it_waits` calls
//!    `store.record_waiting` whenever the *cause* changed (D171,
//!    `server.rs:3435`). `record_waiting` is a durable record rewrite plus an
//!    event append (`store.rs:2007-2023`), i.e. four `fsync`s, not an 11 µs
//!    read. 04a's `junction` column charges the two reads only.
//! 2. **Is `collections_owed` reducible to the open jobs?** 04a recommends
//!    "a `collections_owed` that reads only the open jobs the hub already
//!    holds". Measured here against the store.
//! 3. **What a directory walk costs at all**, even if each entry were
//!    answered by one `stat` instead of a record read: the floor under any
//!    fix that keeps the scan a scan.

mod support;

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use herd_hub::job::{JobState, NewJob};
use herd_hub::{JobDirStore, JobStore};
use herd_proto::{JobId, JobOutcome};

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// N open jobs nothing can run, in a throwaway state root.
fn queued(n: usize) -> (tempfile::TempDir, Arc<JobDirStore>, Vec<JobId>) {
    let dir = tempfile::tempdir().expect("a temporary state root");
    let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));
    let mut ids = Vec::with_capacity(n);
    for i in 0..n {
        let submission = store
            .submit(NewJob::needing(
                vec!["sleep".to_owned(), format!("review-{i}")],
                support::labels(&["review-unobtainable"]),
            ))
            .expect("an open job is minted");
        ids.push(submission.record().id.clone());
    }
    (dir, store, ids)
}

/// 1. What the write half of one pass costs, against what the read half does.
#[test]
#[ignore = "a bench; run it on purpose"]
fn the_write_half_of_a_pass() {
    println!("\n== the write half of one pass: record_waiting per queued job ==");
    println!("     N |   unfinished_jobs |  N x record_waiting | per record");
    for n in [50usize, 200, 1000] {
        let (_dir, store, ids) = queued(n);

        let at = Instant::now();
        let read = store.unfinished_jobs().expect("the queue reads");
        let reading = at.elapsed();
        assert_eq!(read.len(), n);

        // One pass's worth of news: a distinct cause per job, which is what a
        // pass that found a different fleet than the last one produces.
        let at = Instant::now();
        let mut written = 0usize;
        for (i, id) in ids.iter().enumerate() {
            if store
                .record_waiting(id, &format!("it is waiting because of review cause {i}"))
                .expect("the wait records")
            {
                written += 1;
            }
        }
        let writing = at.elapsed();
        assert_eq!(written, n, "every job's cause was news");

        println!(
            "{n:>6} | {:>17?} | {:>19?} | {:?}",
            reading,
            writing,
            writing / n as u32
        );
    }

    // And the same pass a second time, with nothing new to say: D171's half.
    println!("\n== the same N writes when no cause changed (D171) ==");
    for n in [1000usize] {
        let (_dir, store, ids) = queued(n);
        for id in &ids {
            store
                .record_waiting(id, "it is waiting because the fleet is away")
                .expect("recorded");
        }
        let at = Instant::now();
        for id in &ids {
            store
                .record_waiting(id, "it is waiting because the fleet is away")
                .expect("recorded");
        }
        println!("{n:>6} | second pass, same cause: {:?}", at.elapsed());
    }
}

/// 2. Can `collections_owed` be answered from the open jobs?
#[test]
#[ignore = "a bench; run it on purpose"]
fn a_collection_owed_is_not_an_open_job() {
    println!("\n== is an owed collection visible to unfinished_jobs()? ==");
    let dir = tempfile::tempdir().expect("a temporary state root");
    let store = JobDirStore::open(dir.path()).expect("the state root opens");

    let id = store
        .submit(NewJob::new(vec!["true".to_owned(), "owing".to_owned()]))
        .expect("minted")
        .record()
        .id
        .clone();
    store
        .begin_attempt(&id, "review-runner", "the only runner", None)
        .expect("an attempt starts");
    let bytes = b"{\"ok\":true}";
    store
        .record_outputs(
            &id,
            &herd_proto::JobOutputs {
                files: vec![herd_proto::OutputFile::announced(
                    "files/results.json".to_owned(),
                    bytes.len() as u64,
                    herd_proto::sha256_hex(bytes),
                )],
                streamed: true,
                ..Default::default()
            },
        )
        .expect("the manifest lands");
    store
        .record_state(
            &id,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 },
            },
        )
        .expect("the verdict lands, the bytes have not");

    let owed = store.collections_owed().expect("the scan reads");
    let open = store.unfinished_jobs().expect("the queue reads");
    println!("collections_owed -> {} entry(ies)", owed.len());
    println!("unfinished_jobs  -> {} entry(ies)", open.len());
    println!(
        "the owed job is in unfinished_jobs: {}",
        open.iter().any(|r| r.id == id)
    );
    println!(
        "=> a collections_owed restricted to the open jobs would return {} of {}",
        open.iter()
            .filter(|r| store.collection_owed(&r.id).ok().flatten().is_some())
            .count(),
        owed.len()
    );
}

/// 3. The floor under any fix that keeps the scan a scan.
#[test]
#[ignore = "a bench; run it on purpose"]
fn what_a_bare_directory_walk_costs() {
    println!("\n== a walk of T closed job directories: record read vs one stat ==");
    println!("     T |   collections_owed |  read_dir + 1 stat | read_dir alone");
    for t in [1_000usize, 5_000, 20_000] {
        let dir = tempfile::tempdir().expect("a temporary state root");
        let store = JobDirStore::open(dir.path()).expect("the state root opens");
        // One real finished record, copied T times, exactly as cost_surface
        // builds its closed corpus.
        let template = store
            .submit(NewJob::new(vec!["true".to_owned(), "template".to_owned()]))
            .expect("minted")
            .record()
            .id
            .clone();
        store
            .record_state(
                &template,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("finished");
        let template_path = store.job_dir(&template).join("job.json");
        let text = fs::read_to_string(&template_path).expect("reads");
        let digits = template.as_str().len() - 1;
        let jobs = dir.path().join("jobs");
        for i in 0..t {
            let name = format!("j{:0>width$}", 900_000_000 + i, width = digits);
            let job_dir = jobs.join(&name);
            fs::create_dir_all(&job_dir).expect("a closed job directory");
            fs::write(
                job_dir.join("job.json"),
                text.replace(template.as_str(), &name),
            )
            .expect("a closed job record");
        }

        let mut owed = Vec::new();
        let mut stat = Vec::new();
        let mut bare = Vec::new();
        for _ in 0..5 {
            let at = Instant::now();
            let _ = store.collections_owed().expect("the scan reads");
            owed.push(at.elapsed());

            let at = Instant::now();
            let names = herd_hub::durable::names_in(&jobs).expect("lists");
            let mut owing = 0usize;
            for name in &names {
                // The cheapest per-entry question a scan could ask: does this
                // job have any part-written output at all?
                if jobs.join(name).join("parts").symlink_metadata().is_ok() {
                    owing += 1;
                }
            }
            stat.push(at.elapsed());
            assert_eq!(owing, 0);

            let at = Instant::now();
            let names = herd_hub::durable::names_in(&jobs).expect("lists");
            bare.push(at.elapsed());
            assert_eq!(names.len(), t + 1);
        }
        println!(
            "{t:>6} | {:>18?} | {:>18?} | {:?}",
            median(owed),
            median(stat),
            median(bare)
        );
    }
}

/// 4. Where the amplification comes from.
///
/// 04a attributes it to the `state.dispatching` std mutex held across the
/// pass (`server.rs:2493`). But `collections_owed` runs at
/// `server.rs:2415`, four lines *before* the pass takes that lock, so the
/// `T` term is not under it. This case runs C concurrent `collections_owed`
/// on plain threads — no tokio, no hub, no `dispatching` lock anywhere —
/// and reports the slowest. If C callers cost C x one call with the lock
/// absent, the lock is not the amplifier.
#[test]
#[ignore = "a bench; run it on purpose"]
fn where_the_amplification_comes_from() {
    use std::sync::Barrier;

    println!("\n== C concurrent collections_owed, no hub and no dispatching lock ==");
    for t in [20_000usize] {
        let dir = tempfile::tempdir().expect("a temporary state root");
        let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));
        let template = store
            .submit(NewJob::new(vec!["true".to_owned(), "template".to_owned()]))
            .expect("minted")
            .record()
            .id
            .clone();
        store
            .record_state(
                &template,
                JobState::Finished {
                    outcome: JobOutcome::Exited { code: 0 },
                },
            )
            .expect("finished");
        let template_path = store.job_dir(&template).join("job.json");
        let text = fs::read_to_string(&template_path).expect("reads");
        let digits = template.as_str().len() - 1;
        let jobs = dir.path().join("jobs");
        for i in 0..t {
            let name = format!("j{:0>width$}", 900_000_000 + i, width = digits);
            let job_dir = jobs.join(&name);
            fs::create_dir_all(&job_dir).expect("a closed job directory");
            fs::write(
                job_dir.join("job.json"),
                text.replace(template.as_str(), &name),
            )
            .expect("a closed job record");
        }
        // Warm.
        let _ = store.collections_owed().expect("reads");

        println!("     T  callers |   slowest caller | x one call");
        let mut one = Duration::ZERO;
        for callers in [1usize, 2, 4] {
            let mut rounds = Vec::new();
            for _ in 0..3 {
                let gate = Arc::new(Barrier::new(callers));
                let hands: Vec<_> = (0..callers)
                    .map(|_| {
                        let store = Arc::clone(&store);
                        let gate = Arc::clone(&gate);
                        std::thread::spawn(move || {
                            gate.wait();
                            let at = Instant::now();
                            let _ = store.collections_owed().expect("reads");
                            at.elapsed()
                        })
                    })
                    .collect();
                let slowest = hands
                    .into_iter()
                    .map(|h| h.join().expect("the caller returns"))
                    .max()
                    .expect("at least one caller");
                rounds.push(slowest);
            }
            let slowest = median(rounds);
            if callers == 1 {
                one = slowest;
            }
            println!(
                "{t:>6} {callers:>8} | {:>16?} | {:.2}",
                slowest,
                slowest.as_secs_f64() / one.as_secs_f64()
            );
        }
    }
}

/// 5. 04b's additive claim, both ways, on real types.
///
/// 04b (§3.2, §3.5) says a `#[serde(default)]` field on `StepRun` needs no
/// protocol or record break, and that "a rollback works too". The first half
/// is checked here on the real `StepRun` and the real `JobRecord`; the second
/// half is checked by asking what an old binary *writes back*.
///
/// **`ran_commit` has since shipped (D176), so it is no longer an unknown
/// field to this build and cannot stand for one.** `a_field_from_later` took
/// its place here, which costs the bench nothing: what it measures is what
/// happens to *any* field this build has never heard of, and that was always
/// the general statement. The same two directions are now **asserted**
/// rather than printed, on `ran_commit` itself, in
/// `herd-hub/tests/ran_commit_across_versions.rs`.
#[test]
#[ignore = "a bench; run it on purpose"]
fn an_unknown_field_survives_a_read_and_not_a_write() {
    println!("\n== 04b (c): an added field, forwards and backwards ==");

    // Forwards: a frame from a newer peer, decoded by this build.
    let newer_step = r#"{
        "step": "fetch",
        "started_unix_ms": 1,
        "duration_ms": 2,
        "exit_code": 0,
        "a_field_from_later": "0a2cf73"
    }"#;
    let step: herd_proto::StepRun =
        serde_json::from_str(newer_step).expect("an older build decodes a newer step");
    println!(
        "StepRun with an unknown `a_field_from_later`: decoded, step = {:?}",
        step.step
    );

    // Forwards on the wire: the same, inside a frame.
    let newer_frame = r#"{"type":"set_switch","switch":{"position":"on"},"why":"a probe","a_field_from_later":"0a2cf73"}"#;
    let frame: herd_proto::FromHub =
        serde_json::from_str(newer_frame).expect("an older build decodes a newer frame");
    println!("FromHub with an unknown field: decoded as {frame:?}");

    // Backwards: an old binary reads a record a new one wrote, and writes it
    // back. This is the cell 04b's matrix calls "ignored; the record still
    // reads".
    let dir = tempfile::tempdir().expect("a temporary state root");
    let store = JobDirStore::open(dir.path()).expect("the state root opens");
    let id = store
        .submit(NewJob::new(vec!["true".to_owned(), "rollback".to_owned()]))
        .expect("minted")
        .record()
        .id
        .clone();
    store
        .begin_attempt(&id, "review-runner", "the only runner", None)
        .expect("an attempt starts");
    store
        .record_step(
            &id,
            1,
            herd_proto::StepRun {
                step: herd_proto::Step::Fetch,
                started_unix_ms: 1,
                duration_ms: 2,
                exit_code: Some(0),
                ended: None,
                ran_commit: None,
            },
        )
        .expect("the step records");

    let path = store.job_dir(&id).join("job.json");
    let text = fs::read_to_string(&path).expect("the record reads");
    // What a *newer* hub would have written: the same record, one field more.
    let newer = text.replacen(
        "\"duration_ms\":2",
        "\"duration_ms\":2,\"a_field_from_later\":\"0a2cf73\"",
        1,
    );
    assert_ne!(newer, text, "the injection landed");
    fs::write(&path, &newer).expect("the newer record is planted");

    // The old binary reads it...
    let read = store
        .job(&id)
        .expect("the record reads")
        .expect("it is there");
    println!(
        "old build read a record carrying a_field_from_later: ok, {} step(s)",
        read.attempts.last().map(|a| a.steps.len()).unwrap_or(0)
    );
    println!(
        "  the planted field is still on the disk: {}",
        fs::read_to_string(&path)
            .expect("reads")
            .contains("a_field_from_later")
    );

    // ...and then writes the same record back, which is what `record_waiting`
    // and `note_outputs_fetched` do on a job the old build touches again.
    store
        .record_state(&id, JobState::Running)
        .expect("the old build writes the record again");
    let after = fs::read_to_string(&path).expect("reads");
    println!(
        "  after one write by the old build, a_field_from_later is still there: {}",
        after.contains("a_field_from_later")
    );
}

/// 6. One whole junction on a live hub, timed instead of counted.
///
/// 04a's case 4 reads its counters the moment `handle.runner_connected()`
/// turns true (`cost_surface.rs:802-813`), which is **before** the pass at
/// `server.rs:2419` has finished writing. Its "5 103 record accesses, 99 % of
/// them `T`" therefore ends one line before the write half of the same
/// junction. Here the same junction is timed end to end: from the runner
/// dialling to the last queued job carrying its `waiting_for` on the disk.
/// `T` is varied and `N` is not, so the two halves separate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a bench; run it on purpose"]
async fn one_whole_junction_timed() {
    println!("\n== one junction, end to end, N = 50 ==");
    println!("     T |  runner joined in |  queue settled in | T-walk's share");
    for t in [0usize, 5_000, 20_000] {
        let mut rounds: Vec<Duration> = Vec::new();
        for _round in 0..3 {
            let dir = tempfile::tempdir().expect("a temporary state root");
            let store = Arc::new(JobDirStore::open(dir.path()).expect("the state root opens"));

            let mut ids = Vec::new();
            for i in 0..50 {
                ids.push(
                    store
                        .submit(NewJob::needing(
                            vec!["sleep".to_owned(), format!("junction-{i}")],
                            support::labels(&["review-unobtainable"]),
                        ))
                        .expect("minted")
                        .record()
                        .id
                        .clone(),
                );
            }
            // T closed directories, as cost_surface builds them.
            if t > 0 {
                let template = store
                    .submit(NewJob::new(vec!["true".to_owned(), "template".to_owned()]))
                    .expect("minted")
                    .record()
                    .id
                    .clone();
                store
                    .record_state(
                        &template,
                        JobState::Finished {
                            outcome: JobOutcome::Exited { code: 0 },
                        },
                    )
                    .expect("finished");
                let text = fs::read_to_string(store.job_dir(&template).join("job.json"))
                    .expect("the template reads");
                let digits = template.as_str().len() - 1;
                let jobs = dir.path().join("jobs");
                for i in 0..t {
                    let name = format!("j{:0>width$}", 900_000_000 + i, width = digits);
                    let job_dir = jobs.join(&name);
                    fs::create_dir_all(&job_dir).expect("a closed job directory");
                    fs::write(
                        job_dir.join("job.json"),
                        text.replace(template.as_str(), &name),
                    )
                    .expect("a closed job record");
                }
            }

            let token = store
                .approve_runner("review-runner")
                .expect("the state root mints a token");
            let hub = herd_hub::Hub::bind_with_policy(
                "127.0.0.1:0".parse().expect("a literal loopback address"),
                Arc::clone(&store) as Arc<dyn JobStore>,
                herd_hub::Policy::default(),
            )
            .await
            .expect("the hub binds a free port");
            let hub_url = format!("ws://{}", hub.local_addr());
            let handle = hub.handle();
            let serving = tokio::spawn(async move {
                let _ = hub.serve().await;
            });

            let data_root = support::DataRootDir::new("review-junction");
            let config = herd_runner::Config {
                hub_url,
                name: Some("review-runner".to_owned()),
                token: Some(token),
                confinement: support::confinement(&data_root),
                ..herd_runner::Config::default()
            };
            let at = Instant::now();
            let running = tokio::spawn(async move {
                let _ = herd_runner::run(config).await;
            });

            let deadline = Instant::now() + support::PATIENCE;
            while !handle.runner_connected() {
                assert!(Instant::now() < deadline, "the runner never joined");
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            let joined = at.elapsed();

            // The pass is done when the last job it judged carries its reason on
            // the disk. That write is `say_it_waits` -> `record_waiting`
            // (`server.rs:2640`, `store.rs:2007`), the half 04a's counters miss.
            let last = ids.last().expect("fifty jobs").clone();
            let deadline = Instant::now() + support::PATIENCE;
            loop {
                let wrote = store
                    .job(&last)
                    .ok()
                    .flatten()
                    .and_then(|r| r.waiting_for)
                    .is_some_and(|why| why.contains("review-runner"));
                if wrote {
                    break;
                }
                assert!(Instant::now() < deadline, "the pass never settled");
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            let settled = at.elapsed();

            println!(
                "{t:>6} | {:>17?} | {:>17?} | {}",
                joined,
                settled,
                if t == 0 { "-" } else { "see T = 0 row" }
            );
            rounds.push(settled);

            running.abort();
            serving.abort();
            drop(data_root);
        }
        println!(
            "{t:>6} | median of 3 settled: {:?}  (rounds {:?})",
            median(rounds.clone()),
            rounds
        );
    }
}
