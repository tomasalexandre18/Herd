//! `herd-hub retention-preview`: every figure it prints, and the four things
//! M12's retention had to settle before its engine is written (ORCH-146).
//!
//! Seven things are being held here, and they are not the same thing:
//!
//! 1. **The arithmetic.** A fixture with the awkward cases in it — a class
//!    whose span is `0`, a record that is not over, a quarantined record, a
//!    record from a layout this build does not read, a job directory with no
//!    record yet, and expired records in three classes — asserted figure by
//!    figure and then as the whole of its rendered text. Every `job.json` in
//!    it is padded to exactly 512 bytes, so every byte figure the preview
//!    prints is one a reader can check by arithmetic rather than one the
//!    test copied from a run.
//! 2. **That it disturbs nothing.** It runs while the `flock(2)` on
//!    `hub.lock` is held by somebody else, which is the state a live hub
//!    leaves that file in, and the state root is byte-identical afterwards.
//!    This tool's whole claim is that it removes nothing.
//! 3. **Settlement 1: `updated_at` is a stable end time.** The store's own
//!    writer ends a job, a requester fetches its outputs, a runner resends a
//!    file that is already whole, and the budget evicts its bytes — and
//!    `updated_at` does not move. If it did, a span measured from it would
//!    be an LRU, whose sign D152 says is wrong.
//! 4. **Settlement 2: a clock that cannot be used refuses.** A clock behind
//!    this root's own newest record, and a clock far enough forward that
//!    every record that can expire is expired: both refuse, name the
//!    figures, and print no list of what would go.
//! 5. **Settlement 3: a span of `0` means never**, printed in those words.
//! 6. **Settlement 4: the wrong population is never selected.** A record
//!    that is not over is the reaper's (ORCH-012, D134) and a quarantined
//!    record has no end time this build may trust (D52): neither can ever
//!    appear as expired, whatever the span.
//! 7. **That it reads real records.** The fixture is hand-written, so on its
//!    own it would only prove the preview agrees with the test. Two tests
//!    drive the store's own writer and preview what *it* left.
//! 8. **The three populations, apart** (ORCH-168). A state root holds three
//!    populations with three different owners — the collected files, the
//!    records and the logs — and the fixture gives each of them a different,
//!    arithmetically checkable size, on both sides of every span. Four
//!    things are held here that one undifferentiated figure could not hold:
//!    a record naming **no** collected file still reports its own bytes and
//!    its log (41 % of the first consumer's records name no file, so this is
//!    the common case); a **64 MiB** log — the runner's own default cap, and
//!    nothing on the hub bounds it at all — reports as 64 MiB rather than as
//!    "kilobytes"; a log this build could not measure is **UNKNOWN and
//!    named**, never nought; and a log nothing ever wrote is a measurement
//!    of its own and not the same thing as an unmeasured one.

use std::fs;
use std::path::Path;
use std::time::Duration;

use herd_hub::job::JobStore;
use herd_hub::retention::{self, Judged, LogWeight, Selection, Spans, Verdict, Weight};
use herd_hub::{Budget, JobDirStore, JobState, Mint, NewJob, RetentionPreview, Timestamp};
use herd_proto::{JobOutcome, LabelSet, Priority};

/// The clock reading the whole fixture is judged against:
/// 2026-10-28T00:00:00Z, the same instant `state_root_report.rs` pins to.
const PINNED: i64 = 1_793_145_600;

const DAY: i64 = 86_400;

/// Every `job.json` of the fixture, to the byte.
///
/// The record is padded with spaces, which `serde_json` accepts as trailing
/// whitespace, so no field of the fixture is a lie and every byte figure the
/// preview prints is `512 * records + the files`. A fixture whose sizes a
/// reader cannot do in their head is a fixture whose figures only the test
/// knows.
const RECORD_BYTES: usize = 512;

/// Every `log.seq` of the fixture, which is the length the store's own
/// writer gives one: two twenty-digit figures and a newline
/// (`LOG_SEQ_DIGITS`). It is part of the **record** population, not the log:
/// it is the hub's counter for the log and goes with the record.
const LOG_SEQ_BYTES: usize = 42;

/// A record plus its counter, which is what a job that was ever assigned
/// weighs in the record population.
const RECORD_AND_COUNTER: u64 = (RECORD_BYTES + LOG_SEQ_BYTES) as u64;

/// The runner's own default `--job-log-cap`, and the figure that makes the
/// point: **nothing on the hub bounds a log**, so one record can weigh this
/// much on its own. Written sparse, so the fixture costs no disk and every
/// figure still comes from `metadata().len()`, which is what the preview
/// reads.
const BIG_LOG: u64 = 64 << 20;

fn span(days: i64) -> Duration {
    Duration::from_secs((days * DAY) as u64)
}

/// The spans this fixture is read with. **Not a policy**: they are chosen so
/// that each class demonstrates a different thing, and `producer` is `0` so
/// that "never expires" is in the asserted text.
fn spans() -> Spans {
    Spans::new(span(90), span(14), span(14), Duration::ZERO)
}

/// A moment, as days before the pinned clock reading.
fn days_ago(days: i64) -> Timestamp {
    Timestamp::at_unix_seconds(PINNED - days * DAY)
}

/// One job directory: a `job.json` padded to [`RECORD_BYTES`], and the real
/// bytes the test wants beside it.
fn job_dir(
    root: &Path,
    name: &str,
    record: Option<serde_json::Value>,
    files: &[(&str, &str, &[u8])],
) {
    let dir = root.join("jobs").join(name);
    fs::create_dir_all(&dir).expect("the job directory is made");
    if let Some(record) = record {
        let mut text = serde_json::to_string(&record).expect("the record serialises");
        assert!(
            text.len() <= RECORD_BYTES,
            "{name}'s record is {} bytes, past the {RECORD_BYTES} this fixture pads to",
            text.len()
        );
        while text.len() < RECORD_BYTES {
            text.push(' ');
        }
        fs::write(dir.join("job.json"), text).expect("the record is written");
    }
    for (where_, path, bytes) in files {
        let at = dir.join(where_).join(path);
        fs::create_dir_all(at.parent().expect("a parent")).expect("room for the file");
        fs::write(&at, bytes).expect("the file is written");
    }
}

/// The counter the store writes the moment a job is assigned, at the length
/// it writes it. Its presence beside a missing `log` is what tells an
/// absent log from an unmeasurable one.
fn log_counter(root: &Path, name: &str) {
    let dir = root.join("jobs").join(name);
    fs::write(dir.join("log.seq"), vec![b'0'; LOG_SEQ_BYTES]).expect("the counter is written");
}

/// A log of exactly this many bytes, sparse: `set_len` gives the file the
/// length `metadata()` reports without writing a byte, so a 64 MiB log in
/// the fixture costs no disk and the figure the preview reads is still the
/// figure it would read on an operator's root.
fn log_of(root: &Path, name: &str, bytes: u64) {
    let dir = root.join("jobs").join(name);
    let log = fs::File::create(dir.join("log")).expect("the log is made");
    log.set_len(bytes).expect("the log is given its length");
}

/// A record carrying only what retention judges it by, plus the two fields
/// every reader of a `job.json` expects to find.
fn record(
    id: &str,
    class: Priority,
    state: serde_json::Value,
    ended: Timestamp,
) -> serde_json::Value {
    serde_json::json!({
        "record_version": 1,
        "id": id,
        "command": ["true"],
        "labels": [],
        "priority": class.name(),
        "state": state,
        "submitted_at": ended.to_string(),
        "updated_at": ended.to_string(),
    })
}

fn finished() -> serde_json::Value {
    serde_json::json!({ "type": "finished", "outcome": { "type": "exited", "code": 0 } })
}

fn running() -> serde_json::Value {
    serde_json::json!({ "type": "running" })
}

/// Everything in `jobs/`, and the real bytes beside it.
///
/// The names are in the hub's own zero-padded shape, because the preview
/// prints them and an operator pastes them into an `ls`.
///
/// **Every one of the three populations has a different size in here, and
/// each of the log's three readings appears on both sides of a span:**
/// measured, UNKNOWN, and never written. The record population is
/// `RECORD_BYTES` plus `LOG_SEQ_BYTES` for a job that was ever assigned and
/// `RECORD_BYTES` alone for one that was not, so every record figure the
/// preview prints is 512 or 554 times a count.
fn fixture(root: &Path) {
    // merger, span 90d. One 10 days past it, one one day short of it.
    job_dir(
        root,
        "j0000000001",
        Some(record(
            "j0000000001",
            Priority::Merger,
            finished(),
            days_ago(100),
        )),
        &[("files", "a.bin", &[b'a'; 10])],
    );
    log_counter(root, "j0000000001");
    log_of(root, "j0000000001", 300);
    job_dir(
        root,
        "j0000000002",
        Some(record(
            "j0000000002",
            Priority::Merger,
            finished(),
            days_ago(89),
        )),
        &[],
    );
    // No `log` and no `log.seq`: this job never reached an attempt, so
    // nothing ever wrote a log byte for it. Nought is a measurement here and
    // the preview says so in its own line.
    //
    // step, span 14d. Two past it — one of them with a `files.gone/` a killed
    // sweep left behind (D152) — one with a collection still arriving, and
    // one a day old, which is the newest record on this root and therefore
    // the clock's own witness.
    job_dir(
        root,
        "j0000000003",
        Some(record(
            "j0000000003",
            Priority::Step,
            finished(),
            days_ago(30),
        )),
        &[
            ("files", "out.bin", &[b'o'; 100]),
            ("files.gone", "old.bin", b"fifth"),
        ],
    );
    // Its counter is there and its log is not, which is the one shape that
    // means **unknown**: this hub's own record says a log was being counted,
    // so the bytes removing this record would free cannot be measured and
    // are never printed as nought.
    log_counter(root, "j0000000003");
    job_dir(
        root,
        "j0000000004",
        Some(record(
            "j0000000004",
            Priority::Step,
            finished(),
            days_ago(20),
        )),
        &[("files.part", "half.bin", b"arrived")],
    );
    // Seven bytes of collection and a 64 MiB log. **The figure this whole
    // split exists for**: the output budget would see seven bytes here, and
    // what removing the record frees is sixty-four megabytes of log that
    // nothing on the hub bounds.
    log_counter(root, "j0000000004");
    log_of(root, "j0000000004", BIG_LOG);
    job_dir(
        root,
        "j0000000005",
        Some(record(
            "j0000000005",
            Priority::Step,
            finished(),
            days_ago(1),
        )),
        &[],
    );
    // Names no collected file at all and still weighs: its record, its
    // counter and 1234 bytes of log. The first consumer's common case — 41 %
    // of their records name no file — and it is on the **inside** of its
    // span, so the kept side carries it too.
    log_counter(root, "j0000000005");
    log_of(root, "j0000000005", 1234);
    // nightly, span 14d: the class that carries the volume. One well past the
    // span with a kilobyte of output, and one that is **not over** — a hub
    // killed mid-attempt, which is the reaper's and never retention's
    // (ORCH-012, D134).
    job_dir(
        root,
        "j0000000006",
        Some(record(
            "j0000000006",
            Priority::Nightly,
            finished(),
            days_ago(60),
        )),
        &[("files", "log.txt", &[b'n'; 1000])],
    );
    log_counter(root, "j0000000006");
    log_of(root, "j0000000006", 2000);
    job_dir(
        root,
        "j0000000007",
        Some(record(
            "j0000000007",
            Priority::Nightly,
            running(),
            days_ago(200),
        )),
        &[],
    );
    // producer, span 0: a record five hundred days old that never expires.
    job_dir(
        root,
        "j0000000008",
        Some(record(
            "j0000000008",
            Priority::Producer,
            finished(),
            days_ago(500),
        )),
        &[("files", "x.bin", b"xyz")],
    );
    // Readable and not actionable (D52): its labels are not labels this hub
    // accepts. Four hundred days old, and it must never be shown as expired.
    let mut quarantined = record("j0000000009", Priority::Step, finished(), days_ago(400));
    quarantined["labels"] = serde_json::json!(["Nope"]);
    job_dir(root, "j0000000009", Some(quarantined), &[]);
    // A record from a layout this build does not read. Named, counted, and
    // left out of every figure — and therefore never removable.
    let mut other_layout = record("j0000000010", Priority::Step, finished(), days_ago(300));
    other_layout["record_version"] = serde_json::json!(2);
    job_dir(root, "j0000000010", Some(other_layout), &[]);
    // A directory claimed for a job whose first record has not landed. A
    // state, not a fault.
    job_dir(root, "j0000000011", None, &[]);
}

/// Every file under `root`, by path, length **and modification time**, so
/// that "it wrote nothing" is an assertion and not a claim.
///
/// The modification time is here because a length is not a proof: a write
/// that replaced a byte without changing the count would pass a
/// length-only comparison. Reading a file never moves its `mtime`, so a
/// listing that is equal in all three is a root nothing wrote to — which is
/// the one claim this whole subcommand rests on.
fn listing(root: &Path) -> Vec<(String, u64, std::time::SystemTime)> {
    fn walk(at: &Path, base: &Path, into: &mut Vec<(String, u64, std::time::SystemTime)>) {
        let Ok(entries) = fs::read_dir(at) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let kind = entry.file_type().expect("a file type");
            if kind.is_dir() {
                walk(&path, base, into);
            } else {
                let at = entry.metadata().expect("metadata");
                let name = path
                    .strip_prefix(base)
                    .expect("under the root")
                    .to_string_lossy()
                    .into_owned();
                into.push((name, at.len(), at.modified().expect("a modified time")));
            }
        }
    }
    let mut found = Vec::new();
    walk(root, root, &mut found);
    found.sort();
    found
}

fn class(preview: &RetentionPreview, class: Priority) -> &herd_hub::ClassPreview {
    preview
        .classes
        .get(&class.rank())
        .expect("every class has an entry")
}

fn expired_ids(preview: &RetentionPreview, of: Priority) -> Vec<&str> {
    class(preview, of)
        .expired
        .iter()
        .map(|seen| seen.id.as_str())
        .collect()
}

#[test]
fn every_figure_of_the_preview_is_this_fixture_to_the_byte() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();
    fixture(root);

    // The state a live hub leaves this file in. The preview must run anyway:
    // if it took this lock it would refuse, and if it waited for it it would
    // wait for the hub to stop.
    let held = herd_hub::durable::lock_exclusive(&root.join("hub.lock"))
        .expect("the test takes the lock a running hub holds");
    let before = listing(root);

    let preview = RetentionPreview::read(root, spans(), Timestamp::at_unix_seconds(PINNED))
        .expect("the preview reads this root");

    // The walk. One record is quarantined and one is from another layout:
    // both are counted and named rather than skipped, which is what makes
    // every figure below a floor and not a total (D27's rule, applied to a
    // record).
    assert_eq!(preview.scanned.directories, 11);
    assert_eq!(preview.scanned.read, 8);
    assert_eq!(preview.scanned.without_record, 1, "j0000000011");
    assert_eq!(preview.scanned.quarantined.len(), 1);
    assert_eq!(preview.scanned.quarantined[0].0, "j0000000009");
    assert_eq!(preview.scanned.unread.len(), 1);
    assert_eq!(preview.scanned.unread[0].path, "jobs/j0000000010/job.json");

    // The clock's witness: the newest record on the whole root.
    assert_eq!(
        preview.newest,
        Some(("j0000000005".to_owned(), Priority::Step, days_ago(1))),
    );
    assert_eq!(preview.verdict, Verdict::Readable);

    // merger, span 90 days: one record 100 days old is past it, one 89 days
    // old has a day left.
    let merger = class(&preview, Priority::Merger);
    assert_eq!(merger.records, 2);
    assert_eq!(merger.not_over, 0);
    assert_eq!(merger.judged, 2);
    assert_eq!(expired_ids(&preview, Priority::Merger), vec!["j0000000001"]);
    // The three populations, apart: ten bytes a sweep could free on its own,
    // 554 of record and counter, and 300 bytes of log that nothing but
    // removing the record frees.
    assert_eq!(merger.past.collected_bytes, 10);
    assert_eq!(merger.past.collected_files, 1);
    assert_eq!(merger.past.without_collected_file, 0);
    assert_eq!(merger.past.record_bytes, RECORD_AND_COUNTER, "512 + 42");
    assert_eq!(merger.past.logs, 1);
    assert_eq!(merger.past.log_bytes, 300);
    assert!(merger.past.logs_unknown.is_empty());
    assert_eq!(merger.past.without_log, 0);
    assert_eq!(merger.past.measured_bytes(), 864, "10 + 554 + 300");
    assert_eq!(
        merger.expired[0].weighed.measured_bytes(),
        Some(864),
        "and the one record it is"
    );
    assert_eq!(merger.expired[0].weighed.log, LogWeight::Measured(300));
    assert_eq!(
        merger.oldest_and_newest(),
        Some((days_ago(100), days_ago(100))),
        "one record, so it is both"
    );
    assert_eq!(merger.inside.records, 1);
    assert_eq!(merger.inside.collected_bytes, 0);
    assert_eq!(merger.inside.without_collected_file, 1);
    assert_eq!(
        merger.inside.record_bytes, RECORD_BYTES as u64,
        "j0000000002 never reached an attempt, so it has no log.seq either"
    );
    assert_eq!(merger.inside.logs, 0);
    assert_eq!(
        merger.inside.without_log, 1,
        "and nothing ever wrote it a log byte: a measurement, not a gap"
    );
    assert_eq!(merger.inside.measured_bytes(), 512);
    assert_eq!(
        merger.next_to_expire,
        Some(("j0000000002".to_owned(), days_ago(89), span(1))),
    );

    // step, span 14 days: two past it, oldest first, and the newest record on
    // the root has 13 days left.
    let step = class(&preview, Priority::Step);
    assert_eq!(step.records, 3, "the quarantined one is not one of them");
    assert_eq!(step.judged, 3);
    assert_eq!(
        expired_ids(&preview, Priority::Step),
        vec!["j0000000003", "j0000000004"],
        "oldest first, by the end time and not by the id"
    );
    assert_eq!(
        step.past.collected_bytes, 112,
        "100 of collection, 5 left by a killed sweep and 7 still arriving"
    );
    assert_eq!(step.past.collected_files, 3);
    assert_eq!(step.past.without_collected_file, 0);
    assert_eq!(step.past.record_bytes, 2 * RECORD_AND_COUNTER, "1108");
    assert_eq!(
        step.past.logs, 1,
        "j0000000004's; j0000000003's could not be measured and is not counted as one"
    );
    assert_eq!(
        step.past.log_bytes, BIG_LOG,
        "the runner's own default cap, in one record, against 112 bytes of collection"
    );
    assert_eq!(step.past.logs_unknown.len(), 1);
    assert_eq!(step.past.logs_unknown[0].0, "j0000000003");
    assert!(
        step.past.logs_unknown[0]
            .1
            .contains("unknown and not nought"),
        "and the reason is named: {}",
        step.past.logs_unknown[0].1
    );
    assert_eq!(step.past.without_log, 0);
    assert_eq!(
        step.past.measured_bytes(),
        112 + 1108 + BIG_LOG,
        "a floor and not a total, because one log of the two could not be measured"
    );
    assert_eq!(step.oldest_and_newest(), Some((days_ago(30), days_ago(20))));
    assert_eq!(step.inside.records, 1);
    assert_eq!(step.inside.collected_bytes, 0);
    assert_eq!(step.inside.collected_files, 0);
    assert_eq!(
        step.inside.without_collected_file, 1,
        "j0000000005 names no file at all and still weighs"
    );
    assert_eq!(step.inside.record_bytes, RECORD_AND_COUNTER);
    assert_eq!(step.inside.logs, 1);
    assert_eq!(step.inside.log_bytes, 1234);
    assert_eq!(step.inside.measured_bytes(), 554 + 1234, "and no file byte");
    assert_eq!(
        step.next_to_expire,
        Some(("j0000000005".to_owned(), days_ago(1), span(13))),
    );

    // nightly, span 14 days: one past it, and one that is not over at all.
    let nightly = class(&preview, Priority::Nightly);
    assert_eq!(nightly.records, 2);
    assert_eq!(
        nightly.not_over, 1,
        "j0000000007 is running and belongs to the reaper, not here"
    );
    assert_eq!(nightly.judged, 1);
    assert_eq!(
        expired_ids(&preview, Priority::Nightly),
        vec!["j0000000006"]
    );
    assert_eq!(nightly.past.collected_bytes, 1000);
    assert_eq!(nightly.past.collected_files, 1);
    assert_eq!(nightly.past.record_bytes, RECORD_AND_COUNTER);
    assert_eq!(nightly.past.logs, 1);
    assert_eq!(nightly.past.log_bytes, 2000);
    assert_eq!(nightly.past.measured_bytes(), 3554, "1000 + 554 + 2000");
    assert_eq!(
        nightly.inside,
        Weight::default(),
        "nothing of this class is inside its span, in any population"
    );
    assert_eq!(nightly.next_to_expire, None);

    // producer, span 0: a record five hundred days old, and nothing of this
    // class is ever selected.
    let producer = class(&preview, Priority::Producer);
    assert_eq!(producer.records, 1);
    assert_eq!(producer.judged, 1);
    assert!(
        producer.expired.is_empty(),
        "a span of 0 means never, not now"
    );
    assert_eq!(producer.past, Weight::default());
    assert_eq!(producer.inside.records, 1);
    assert_eq!(producer.inside.collected_bytes, 3);
    assert_eq!(producer.inside.collected_files, 1);
    assert_eq!(producer.inside.record_bytes, RECORD_BYTES as u64);
    assert_eq!(producer.inside.without_log, 1);
    assert_eq!(producer.inside.measured_bytes(), 515, "3 + 512");
    assert_eq!(producer.next_to_expire, None);

    // And the totals, over the whole root, on both sides of every span.
    assert_eq!(preview.expired(), 4);
    assert_eq!(preview.judged(), 7);
    assert_eq!(
        preview.expirable(),
        6,
        "the producer's record can never expire, so it is not in this population"
    );
    let past = preview.past();
    assert_eq!(past.records, 4);
    assert_eq!(past.collected_bytes, 1122, "10 + 112 + 1000");
    assert_eq!(past.collected_files, 5, "1 + 3 + 1");
    assert_eq!(past.without_collected_file, 0);
    assert_eq!(
        past.record_bytes,
        4 * RECORD_AND_COUNTER,
        "2216: four records that each reached an attempt"
    );
    assert_eq!(past.logs, 3);
    assert_eq!(past.log_bytes, 300 + BIG_LOG + 2000);
    assert_eq!(past.logs_unknown.len(), 1);
    assert_eq!(past.without_log, 0);
    assert_eq!(
        past.measured_bytes(),
        1122 + 2216 + 300 + BIG_LOG + 2000,
        "67114502, of which 1122 bytes are all the output budget can ever see"
    );
    assert_eq!(
        past.logs + past.logs_unknown.len() + past.without_log,
        past.records,
        "every record past its span is in exactly one of the three log figures"
    );

    let inside = preview.inside();
    assert_eq!(inside.records, 3);
    assert_eq!(inside.collected_bytes, 3);
    assert_eq!(inside.collected_files, 1);
    assert_eq!(
        inside.without_collected_file, 2,
        "j0000000002 and j0000000005 name no file at all"
    );
    assert_eq!(
        inside.record_bytes,
        2 * RECORD_BYTES as u64 + RECORD_AND_COUNTER,
        "1578: two records with no counter and one with"
    );
    assert_eq!(inside.logs, 1);
    assert_eq!(inside.log_bytes, 1234);
    assert!(inside.logs_unknown.is_empty());
    assert_eq!(inside.without_log, 2);
    assert_eq!(inside.measured_bytes(), 3 + 1578 + 1234, "2815");
    assert_eq!(
        inside.logs + inside.logs_unknown.len() + inside.without_log,
        inside.records,
        "and so is every record inside its span"
    );

    // And the whole of the text, because these figures only reach an operator
    // as text: a line that silently stopped printing a number would be a
    // figure nobody noticed going missing.
    let rendered = preview.to_string();
    let expected = EXPECTED.replace("{ROOT}", &root.display().to_string());
    assert_eq!(rendered, expected);

    // It disturbed nothing: not one byte of this root changed, and the lock a
    // hub holds was held throughout. This is the claim the whole subcommand
    // rests on.
    assert_eq!(
        listing(root),
        before,
        "the preview removes and writes nothing"
    );
    drop(held);
}

/// Settlement 2, first half: a clock behind this root's own newest record.
#[test]
fn a_clock_behind_the_root_refuses_and_names_the_figures() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();
    fixture(root);
    let before = listing(root);

    // Ten days before the fixture's own newest record, which this hub wrote
    // with its own clock: the clock has gone backwards since.
    let now = Timestamp::at_unix_seconds(PINNED - 10 * DAY);
    let preview = RetentionPreview::read(root, spans(), now).expect("the preview reads this root");

    assert_eq!(
        preview.verdict,
        Verdict::ClockBehindTheRoot {
            newest: days_ago(1),
            ahead_by: span(9),
        }
    );
    assert!(!preview.verdict.is_readable());
    let said = preview.verdict.refusal().expect("a refusal has a sentence");
    assert!(
        said.contains("2026-10-27T00:00:00Z"),
        "it names the record: {said}"
    );
    assert!(said.contains("9 day(s)"), "and the gap: {said}");

    let rendered = preview.to_string();
    assert!(rendered.contains("REFUSED: THE CLOCK IS BEHIND THIS ROOT'S OWN RECORDS"));
    assert!(
        !rendered.contains("what retention would remove, by class"),
        "a refusal prints no list of what would go"
    );
    assert_eq!(listing(root), before, "and it removed nothing");
}

/// Settlement 2, second half: a clock far enough forward that a pass would
/// empty the root.
///
/// **The planner's proposed rule does not catch this, and that is the
/// finding.** `now` earlier than the newest record catches a clock that went
/// *backwards*; a clock stepped *forward* is ahead of every record and that
/// test passes it through. Nothing on a state root distinguishes a forward
/// step from a hub that has been idle longer than its shortest span — a
/// year-old newest record is what both look like. So what is refused is the
/// consequence the two share: a pass that would take every record there is.
#[test]
fn a_clock_a_year_forward_refuses_rather_than_reporting_everything_expired() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();
    fixture(root);
    let before = listing(root);

    let now = Timestamp::at_unix_seconds(PINNED + 365 * DAY);
    let preview = RetentionPreview::read(root, spans(), now).expect("the preview reads this root");

    match &preview.verdict {
        Verdict::WouldEmptyTheRoot {
            expirable,
            newest,
            newest_class,
            newest_at,
            newest_past_by,
        } => {
            assert_eq!(*expirable, 6);
            assert_eq!(newest, "j0000000005");
            assert_eq!(*newest_class, Priority::Step);
            assert_eq!(*newest_at, days_ago(1));
            assert_eq!(
                *newest_past_by,
                span(365 + 1 - 14),
                "even the newest record is 352 days past its 14-day span"
            );
        }
        other => panic!("a pass that would empty the root was not refused: {other:?}"),
    }
    // The figures were computed and deliberately not printed: an operator who
    // saw "every record is expired" would be reading the output this refusal
    // exists to prevent.
    assert_eq!(preview.expired(), 6);
    let rendered = preview.to_string();
    assert!(rendered.contains("REFUSED: A PASS WOULD EMPTY THIS ROOT OF RECORDS"));
    assert!(
        !rendered.contains("what retention would remove, by class"),
        "a refusal prints no list of what would go"
    );
    assert!(
        rendered.contains("a span typed in the wrong"),
        "and it names the other thing that lands here: {rendered}"
    );
    assert_eq!(listing(root), before, "and it removed nothing");
}

/// Settlement 3, and the asymmetry with `--output-floor 0` that makes it the
/// right way round.
#[test]
fn a_span_of_zero_never_expires_and_the_preview_says_so_in_those_words() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();
    fixture(root);

    // Every class pinned to 0: nothing on this root can expire, however old.
    let never = Spans::new(
        Duration::ZERO,
        Duration::ZERO,
        Duration::ZERO,
        Duration::ZERO,
    );
    let preview = RetentionPreview::read(root, never, Timestamp::at_unix_seconds(PINNED))
        .expect("the preview reads this root");
    assert_eq!(preview.expired(), 0, "a span of 0 selects nothing, ever");
    assert_eq!(preview.expirable(), 0);
    assert_eq!(preview.judged(), 7);
    assert_eq!(
        preview.verdict,
        Verdict::Readable,
        "nothing can expire, so no pass would empty this root"
    );
    let rendered = preview.to_string();
    assert!(
        rendered.contains("0: these records NEVER EXPIRE"),
        "the words are in the output: {rendered}"
    );
    assert!(
        rendered.contains("not \"remove it now\""),
        "and so is why it is not the other reading: {rendered}"
    );
    assert!(rendered.contains("never: this class's span is 0"));
}

/// Settlement 4: neither population can ever be selected, at any span.
#[test]
fn a_job_that_is_not_over_and_a_quarantined_record_are_never_expired() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();
    fixture(root);

    // One second for every class: as aggressive as a span can be.
    let one_second = Duration::from_secs(1);
    let spans = Spans::new(one_second, one_second, one_second, one_second);
    // Far enough forward that the "would empty the root" refusal fires, so
    // the figures are read off the value rather than its text — which is the
    // point: the counts are right and the printing is what is withheld.
    let preview = RetentionPreview::read(root, spans, Timestamp::at_unix_seconds(PINNED))
        .expect("the preview reads this root");

    let every_id: Vec<&str> = Priority::ALL
        .iter()
        .flat_map(|of| expired_ids(&preview, *of))
        .collect();
    assert!(
        !every_id.contains(&"j0000000007"),
        "j0000000007 is running: it is the reaper's (ORCH-012, D134) and never retention's"
    );
    assert!(
        !every_id.contains(&"j0000000009"),
        "j0000000009 is quarantined: it has no end time this build may trust (D52)"
    );
    assert!(
        !every_id.contains(&"j0000000010"),
        "j0000000010 is from another layout: a record this build cannot read can never be \
         shown to be past its span"
    );
    assert!(
        !every_id.contains(&"j0000000011"),
        "j0000000011 has no record at all"
    );
    assert_eq!(
        every_id.len(),
        7,
        "the seven judgeable records and not one more"
    );
    assert_eq!(class(&preview, Priority::Nightly).not_over, 1);
    assert_eq!(preview.scanned.quarantined.len(), 1);

    // And the same thing at the selection function the engine will call, so
    // that it cannot be true of the preview and false of the engine.
    assert!(!retention::select(
        Judged {
            class: Priority::Nightly,
            terminal: false,
            ended_at: days_ago(10_000),
        },
        &spans,
        Timestamp::at_unix_seconds(PINNED),
    )
    .is_expired());
}

/// A record this build cannot judge is named and counted, never read as a
/// record that ended at the epoch.
#[test]
fn a_record_with_no_end_time_is_named_and_never_expired() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();
    let mut broken = record("j0000000001", Priority::Step, finished(), days_ago(99));
    broken
        .as_object_mut()
        .expect("an object")
        .remove("updated_at");
    job_dir(root, "j0000000001", Some(broken), &[]);

    let preview = RetentionPreview::read(root, spans(), Timestamp::at_unix_seconds(PINNED))
        .expect("the preview reads this root");
    assert_eq!(preview.scanned.read, 0);
    assert_eq!(preview.scanned.unread.len(), 1);
    assert_eq!(preview.scanned.unread[0].path, "jobs/j0000000001/job.json");
    assert!(
        preview.scanned.unread[0].why.contains("cannot judge it"),
        "the reason is named: {}",
        preview.scanned.unread[0].why
    );
    assert_eq!(preview.expired(), 0);
    assert_eq!(preview.newest, None);
    assert_eq!(preview.verdict, Verdict::Readable);
}

/// **Settlement 1.** `updated_at` is a stable end time and not a last-touch
/// clock, asserted against the store's own writer.
///
/// Four things happen to a finished job after it ends — a requester fetches
/// its outputs, a runner resends a file the hub already holds whole, the
/// budget evicts its bytes, and the record is read back — and the figure
/// retention measures a span from does not move. If it moved, retention
/// would be an LRU by the back door: a collection nobody had looked at would
/// expire **first**, which is the opposite of the order D152 chose when it
/// kept an unfetched collection longer than a fetched one.
#[test]
fn updated_at_is_an_end_time_and_not_a_last_touch_clock() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    // A budget of one collection, with no floor, so that the second
    // collection's arrival is what evicts the first (D152's one trigger).
    let store = JobDirStore::open_with(
        dir.path(),
        Budget {
            bytes: 1024,
            floor: Duration::ZERO,
        },
    )
    .expect("a state root");

    let data = vec![b'x'; 1024];
    let kept = |bytes: &[u8]| herd_proto::JobOutputs {
        files: vec![herd_proto::OutputFile::collected(
            "files/results.json".to_owned(),
            bytes.to_vec(),
        )],
        ..herd_proto::JobOutputs::default()
    };

    let job = store
        .create_job(NewJob::at(
            vec!["true".to_owned()],
            LabelSet::default(),
            Priority::Step,
        ))
        .expect("a job is minted");
    store
        .begin_attempt(&job.id, "test-runner", "the only runner", None)
        .expect("an attempt is recorded");
    store
        .record_outputs(&job.id, &kept(&data))
        .expect("the collection is recorded");
    store
        .record_state(
            &job.id,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 },
            },
        )
        .expect("the job ends");

    let ended = store
        .job(&job.id)
        .expect("reads")
        .expect("a record")
        .updated_at;

    // A requester fetches the bytes. `note_outputs_fetched` writes
    // `first_fetched_at` and nothing else.
    store
        .job_outputs(&job.id, true)
        .expect("the collection is handed over");
    store
        .note_outputs_fetched(&job.id)
        .expect("the fetch is noted");
    let fetched = store.job(&job.id).expect("reads").expect("a record");
    assert!(
        fetched.outputs.first_fetched_at.is_some(),
        "the fetch really was written down"
    );
    assert_eq!(
        fetched.updated_at, ended,
        "a fetch must not move the end time: retention is a span, not an LRU"
    );

    // A runner resends a file the hub already holds whole, which is ordinary
    // on a resumed transfer. `write_output_chunk` writes no record at all.
    store
        .write_output_chunk(&job.id, "files/results.json", 0, &data)
        .expect("the resend is answered");
    assert_eq!(
        store
            .job(&job.id)
            .expect("reads")
            .expect("a record")
            .updated_at,
        ended,
        "a resend must not move the end time"
    );

    // Something bigger arrives and the budget has to give: the first job's
    // bytes go, its manifest stays, and its end time is untouched — which the
    // eviction path says of itself and this asserts.
    let newcomer = store
        .create_job(NewJob::at(
            vec!["true".to_owned(), "second".to_owned()],
            LabelSet::default(),
            Priority::Step,
        ))
        .expect("a second job is minted");
    store
        .begin_attempt(&newcomer.id, "test-runner", "the only runner", None)
        .expect("an attempt is recorded");
    store
        .record_outputs(&newcomer.id, &kept(&vec![b'y'; 1024]))
        .expect("the second collection is recorded");

    let evicted = store.job(&job.id).expect("reads").expect("a record");
    assert!(
        evicted.outputs.removed.is_some(),
        "the first job's bytes really were evicted"
    );
    assert!(
        !evicted.outputs.files.is_empty(),
        "and its manifest outlived them (D148)"
    );
    assert_eq!(
        evicted.updated_at, ended,
        "an eviction must not move the end time: the record's age is what the floor and \
         the eviction order judge it by, and retention would inherit that figure"
    );

    // And the selection function reads the same figure, so the span the
    // engine measures is the one this test just pinned.
    assert_eq!(
        Judged::of(&evicted).ended_at,
        ended,
        "the selection function reads updated_at and nothing else"
    );
    drop(store);
}

/// The preview on records the store itself wrote, while the store is still
/// open and therefore still holding its `flock`.
#[test]
fn the_preview_reads_what_the_store_itself_wrote() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let store = JobDirStore::open(dir.path()).expect("a state root");

    // One job that is over, and one that is not: the first is retention's and
    // the second is the reaper's.
    let over = store
        .create_job(NewJob::at(
            vec!["true".to_owned()],
            LabelSet::default(),
            Priority::Nightly,
        ))
        .expect("a job is minted");
    store
        .begin_attempt(&over.id, "test-runner", "the only runner", None)
        .expect("an attempt is recorded");
    // A real log, through the store's own writer. **This is what holds the
    // two file names the preview has to write out again** — `log` and
    // `log.seq` are private to `store.rs` and this task may not widen them,
    // so a rename there has to fail here rather than print nought for every
    // log on an operator's root.
    const LOG_LINE: &[u8] = b"a line of this job's output\n";
    store
        .append_log(&over.id, herd_proto::LogStream::Stdout, 0, LOG_LINE)
        .expect("the log line is appended");
    store
        .record_outputs(
            &over.id,
            &herd_proto::JobOutputs {
                files: vec![herd_proto::OutputFile::collected(
                    "files/out.bin".to_owned(),
                    b"four".to_vec(),
                )],
                ..herd_proto::JobOutputs::default()
            },
        )
        .expect("the collection is recorded");
    store
        .record_state(
            &over.id,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 },
            },
        )
        .expect("the job ends");
    let queued = store
        .create_job(NewJob::at(
            vec!["true".to_owned(), "queued".to_owned()],
            LabelSet::default(),
            Priority::Nightly,
        ))
        .expect("a second job is minted");
    // A third job, of a class with a long span, so that something survives
    // the pass: a preview in which everything that can expire is expired
    // refuses, and this test is about the figures rather than that refusal.
    let safe = store
        .create_job(NewJob::at(
            vec!["true".to_owned(), "safe".to_owned()],
            LabelSet::default(),
            Priority::Merger,
        ))
        .expect("a third job is minted");
    store
        .begin_attempt(&safe.id, "test-runner", "the only runner", None)
        .expect("an attempt is recorded");
    store
        .record_state(
            &safe.id,
            JobState::Finished {
                outcome: JobOutcome::Exited { code: 0 },
            },
        )
        .expect("the third job ends");

    let ended = store
        .job(&over.id)
        .expect("reads")
        .expect("a record")
        .updated_at;
    let before = listing(dir.path());

    // A span of one second, read an hour after the job ended: the record is
    // past it. The store is still open, so its `flock` is still held and its
    // index is still being written — exactly the hub this has to be safe
    // against.
    let one_second = Duration::from_secs(1);
    let preview = RetentionPreview::read(
        dir.path(),
        Spans::new(span(90), one_second, one_second, Duration::ZERO),
        ended.after(Duration::from_secs(3600)),
    )
    .expect("the preview reads a live root");

    assert_eq!(preview.scanned.directories, 3);
    assert_eq!(preview.scanned.read, 3);
    assert!(preview.scanned.unread.is_empty());
    assert!(preview.scanned.quarantined.is_empty());
    assert_eq!(
        preview.verdict,
        Verdict::Readable,
        "the merger record survives the pass, so it would not empty this root"
    );
    assert_eq!(
        class(&preview, Priority::Merger).inside.records,
        1,
        "{} is a merger and has ninety days",
        safe.id
    );
    let nightly = class(&preview, Priority::Nightly);
    assert_eq!(nightly.records, 2);
    assert_eq!(nightly.not_over, 1, "{} is queued", queued.id);
    assert_eq!(nightly.judged, 1);
    assert_eq!(
        expired_ids(&preview, Priority::Nightly),
        vec![over.id.as_str()]
    );
    // What removing that record would free: its own directory, measured the
    // way the sweeper measures one, and split the three ways the operator
    // needs it. Every figure is read off the listing rather than typed, so
    // the three add up to the directory and nothing is double-counted.
    let under = |what: &str| -> u64 {
        before
            .iter()
            .filter(|(path, _, _)| path.starts_with(&format!("jobs/{}/{what}", over.id)))
            .map(|(_, length, _)| *length)
            .sum()
    };
    let on_disk = under("");
    let weighed = &nightly.expired[0].weighed;
    assert_eq!(
        weighed.measured_bytes(),
        Some(on_disk),
        "the whole job directory, which is what retention removes"
    );
    assert_eq!(weighed.collected_bytes, 4, "files/out.bin");
    assert_eq!(weighed.collected_files, 1);
    assert_eq!(
        weighed.log,
        LogWeight::Measured(LOG_LINE.len() as u64),
        "the log the store's own writer appended, under the name store.rs gives it"
    );
    assert_eq!(weighed.record_bytes, on_disk - 4 - LOG_LINE.len() as u64);
    assert_eq!(
        weighed.record_bytes,
        under("job.json") + under("events.jsonl") + under("log.seq"),
        "the record population is the record, the events and the counter"
    );
    assert_eq!(nightly.past.logs, 1);
    assert_eq!(nightly.past.log_bytes, LOG_LINE.len() as u64);
    assert!(nightly.past.logs_unknown.is_empty());
    assert_eq!(nightly.past.without_log, 0);
    assert_eq!(nightly.expired[0].ended_at, ended);

    // And the selection the engine will call agrees, on the record the store
    // wrote, with no preview in between.
    let record = store.job(&over.id).expect("reads").expect("a record");
    assert!(matches!(
        retention::select(
            Judged::of(&record),
            &Spans::new(one_second, one_second, one_second, one_second),
            ended.after(Duration::from_secs(3600)),
        ),
        Selection::Expired { .. }
    ));

    assert_eq!(
        listing(dir.path()),
        before,
        "and the preview wrote nothing to a root a store has open"
    );
    drop(store);
}

/// **A record that names no collected file still reports its own bytes and
/// its log**, which is the common case and not the edge: 41 % of the first
/// consumer's records name no file at all, and a figure that could only see
/// `files/` would read every one of them as nought.
#[test]
fn a_record_naming_no_collected_file_still_weighs_its_record_and_its_log() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();
    // One record, past its span, with no `files/` at all and a log of 4096
    // bytes. Nothing the output budget governs; 4650 bytes that only
    // removing the record frees.
    job_dir(
        root,
        "j0000000001",
        Some(record(
            "j0000000001",
            Priority::Step,
            finished(),
            days_ago(30),
        )),
        &[],
    );
    log_counter(root, "j0000000001");
    log_of(root, "j0000000001", 4096);
    // A second record, inside its span, so that the pass would not empty the
    // root and the figures are printed rather than withheld.
    job_dir(
        root,
        "j0000000002",
        Some(record(
            "j0000000002",
            Priority::Merger,
            finished(),
            days_ago(1),
        )),
        &[],
    );
    let before = listing(root);

    let preview = RetentionPreview::read(root, spans(), Timestamp::at_unix_seconds(PINNED))
        .expect("the preview reads this root");
    assert_eq!(preview.verdict, Verdict::Readable);
    let past = preview.past();
    assert_eq!(past.records, 1);
    assert_eq!(past.collected_bytes, 0, "it names no file");
    assert_eq!(past.collected_files, 0);
    assert_eq!(past.without_collected_file, 1, "and that is counted");
    assert_eq!(past.record_bytes, RECORD_AND_COUNTER, "554");
    assert_eq!(past.logs, 1);
    assert_eq!(past.log_bytes, 4096);
    assert_eq!(
        past.measured_bytes(),
        4650,
        "554 + 4096, every byte of it outside anything the budget can see"
    );

    let rendered = preview.to_string();
    assert!(
        rendered.contains("collected files        0 bytes in 0 file(s)"),
        "the collection is nought and says so: {rendered}"
    );
    assert!(
        rendered.contains("naming no file       1 record(s)"),
        "and the record is counted as naming none: {rendered}"
    );
    assert!(
        rendered.contains("the logs               4096 bytes (4.00 KiB) in 1 log(s)"),
        "while its log is a real figure: {rendered}"
    );
    assert_eq!(listing(root), before, "and it removed nothing");
}

/// **A log that is large reports as large.** The runner's `--job-log-cap`
/// defaults to 64 MiB and the hub appends whatever arrives with no cap of
/// its own, so one record can weigh far more than the "kilobytes" the output
/// budget's own argument assumes of a record. A preview that implied
/// otherwise is the thing ORCH-168 fixes.
#[test]
fn a_log_the_size_of_the_runners_cap_reports_as_megabytes() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();
    fixture(root);
    let before = listing(root);

    let preview = RetentionPreview::read(root, spans(), Timestamp::at_unix_seconds(PINNED))
        .expect("the preview reads this root");
    let step = class(&preview, Priority::Step);
    assert_eq!(step.past.log_bytes, BIG_LOG);
    assert_eq!(
        step.past.collected_bytes, 112,
        "and the collection beside it is 112 bytes: the two figures differ by 600 000 times"
    );

    let rendered = preview.to_string();
    assert!(
        rendered.contains("the logs               67108864 bytes (64.00 MiB) in 1 log(s)"),
        "the exact figure and the approximation beside it: {rendered}"
    );
    assert!(
        rendered.contains("log 67108864 bytes (64.00 MiB)"),
        "and on the line that names the record it belongs to: {rendered}"
    );
    assert!(
        rendered.contains("--job-log-cap"),
        "and the output says where the only cap on it lives: {rendered}"
    );
    assert_eq!(listing(root), before, "and it removed nothing");
}

/// **A log this build could not measure is UNKNOWN and named, never nought.**
/// An unmeasured thing is printed as unmeasured in this project (D27's rule,
/// applied to a figure rather than to a test), and here it matters twice
/// over: a log read as empty is a record whose removal would free more than
/// the preview promised, which is the direction a sizing figure must never
/// err in.
#[test]
fn a_log_that_could_not_be_measured_is_named_and_never_counted_as_nought() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();
    fixture(root);
    let before = listing(root);

    let preview = RetentionPreview::read(root, spans(), Timestamp::at_unix_seconds(PINNED))
        .expect("the preview reads this root");
    let step = class(&preview, Priority::Step);
    assert_eq!(step.past.logs_unknown.len(), 1);
    assert_eq!(step.past.logs_unknown[0].0, "j0000000003");
    assert_eq!(
        step.past.logs, 1,
        "the unknown one is not counted among the measured"
    );
    assert_eq!(
        step.expired[0].weighed.measured_bytes(),
        None,
        "and nor is the record's own total a total any more"
    );
    assert_eq!(
        step.expired[0].weighed.record_bytes, RECORD_AND_COUNTER,
        "while the two populations that were measured still are"
    );

    let rendered = preview.to_string();
    assert!(
        rendered.contains("could not be read    1 log(s): UNKNOWN, never nought"),
        "the count is printed as unknown: {rendered}"
    );
    assert!(
        rendered.contains("jobs/j0000000003: there is no log"),
        "and the record is named with its reason: {rendered}"
    );
    assert!(
        rendered.contains("jobs/j0000000003  files 105 bytes, record 554 bytes, log UNKNOWN"),
        "and the line that names it does not print a figure it does not have: {rendered}"
    );
    assert!(
        rendered.contains("no log ever written  0 record(s)"),
        "which is not the same thing as a log nothing ever wrote: {rendered}"
    );
    assert_eq!(listing(root), before, "and it removed nothing");
}

/// The preview's text, with the only thing that varies left as `{ROOT}`.
const EXPECTED: &str = "\
herd-hub retention preview
  state root  {ROOT}
  clock read  2026-10-28T00:00:00Z
  newest      jobs/j0000000005, a step, last changed 2026-10-27T00:00:00Z

  IT REMOVES NOTHING. This subcommand has no deletion path and no flag that
  could be given one: it reads records and file lengths and exits. It takes
  no lock of any kind, so a hub may be writing this root while it is read --
  each record is read whole or not at all, but the figures together are not
  one instant. The clock reading above is the one every figure below was
  judged against.

spans, as this invocation was given them
  merger    90 day(s) (7776000 s)
  step      14 day(s) (1209600 s)
  nightly   14 day(s) (1209600 s)
  producer  0: these records NEVER EXPIRE
    Every one of these is this invocation's and nothing else's: no span is
    built into this binary and none is defaulted (D194). A span of 0 means
    that class NEVER EXPIRES -- not \"remove it now\" -- which is the
    opposite of what --output-floor 0 means, because the floor's zero loses
    bytes and this one would lose the records.

records
  job directories                    11
  records read                       8
  directories with no record yet     1
  quarantined, never expired         1
    jobs/j0000000009: its labels [\"Nope\"] are not labels this hub accepts: the label \"Nope\" carries 'N', and a label is lower-case letters, digits, '.', '_' and '-'
  records this build could not read  1
    jobs/j0000000010/job.json: it carries record version 2, and this build reads version 1
    A record this build cannot read has no readable end time, so it can
    never be shown to be past its span -- and therefore retention may never
    remove it. This list is a bound on the engine.

what retention would remove, by class

  THREE POPULATIONS, THREE OWNERS, kept apart below because sizing a policy
  against their sum is sizing it against nothing. The collected files are the
  output budget's (D148, D152): a sweep frees them and leaves the record. The
  record and its log are nobody's -- the only thing in Herd that frees one of
  their bytes is removing the record -- and nothing on this hub bounds a log
  at all: the only cap is the runner's --job-log-cap, 64 MiB a job by default,
  on the runner's own machine. A record is not kilobytes because its job.json
  is. A log this build could not measure is printed UNKNOWN and named, never
  as nought.

  merger
    span                     90 day(s) (7776000 s)
    records of this class    2
      not over, so not ours  0 (the reaper's, ORCH-012 and D134)
      judged                 2
    past the span            1 record(s)
      collected files        10 bytes in 1 file(s)
        naming no file       0 record(s)
      the records            554 bytes in 1 record(s)
      the logs               300 bytes in 1 log(s)
        could not be read    0 log(s): UNKNOWN, never nought
        no log ever written  0 record(s)
      measured in all        864 bytes
      oldest                 2026-07-20T00:00:00Z
      newest                 2026-07-20T00:00:00Z
      jobs/j0000000001  files 10 bytes, record 554 bytes, log 300 bytes  ended 2026-07-20T00:00:00Z
    inside the span          1 record(s)
      collected files        0 bytes in 0 file(s)
        naming no file       1 record(s)
      the records            512 bytes in 1 record(s)
      the logs               0 bytes in 0 log(s)
        could not be read    0 log(s): UNKNOWN, never nought
        no log ever written  1 record(s)
      measured in all        512 bytes
      next to expire         jobs/j0000000002, ended 2026-07-31T00:00:00Z, 1 day(s) (86400 s) left

  step
    span                     14 day(s) (1209600 s)
    records of this class    3
      not over, so not ours  0 (the reaper's, ORCH-012 and D134)
      judged                 3
    past the span            2 record(s)
      collected files        112 bytes in 3 file(s)
        naming no file       0 record(s)
      the records            1108 bytes (1.08 KiB) in 2 record(s)
      the logs               67108864 bytes (64.00 MiB) in 1 log(s)
        could not be read    1 log(s): UNKNOWN, never nought
          jobs/j0000000003: there is no log, and the log.seq beside it is this hub's own record that a log was being counted, so its bytes are unknown and not nought
        no log ever written  0 record(s)
      measured in all        67110084 bytes (64.00 MiB)
      oldest                 2026-09-28T00:00:00Z
      newest                 2026-10-08T00:00:00Z
      jobs/j0000000003  files 105 bytes, record 554 bytes, log UNKNOWN  ended 2026-09-28T00:00:00Z
      jobs/j0000000004  files 7 bytes, record 554 bytes, log 67108864 bytes (64.00 MiB)  ended 2026-10-08T00:00:00Z
    inside the span          1 record(s)
      collected files        0 bytes in 0 file(s)
        naming no file       1 record(s)
      the records            554 bytes in 1 record(s)
      the logs               1234 bytes (1.21 KiB) in 1 log(s)
        could not be read    0 log(s): UNKNOWN, never nought
        no log ever written  0 record(s)
      measured in all        1788 bytes (1.75 KiB)
      next to expire         jobs/j0000000005, ended 2026-10-27T00:00:00Z, 13 day(s) (1123200 s) left

  nightly
    span                     14 day(s) (1209600 s)
    records of this class    2
      not over, so not ours  1 (the reaper's, ORCH-012 and D134)
      judged                 1
    past the span            1 record(s)
      collected files        1000 bytes in 1 file(s)
        naming no file       0 record(s)
      the records            554 bytes in 1 record(s)
      the logs               2000 bytes (1.95 KiB) in 1 log(s)
        could not be read    0 log(s): UNKNOWN, never nought
        no log ever written  0 record(s)
      measured in all        3554 bytes (3.47 KiB)
      oldest                 2026-08-29T00:00:00Z
      newest                 2026-08-29T00:00:00Z
      jobs/j0000000006  files 1000 bytes, record 554 bytes, log 2000 bytes (1.95 KiB)  ended 2026-08-29T00:00:00Z
    inside the span          0 record(s)
      collected files        0 bytes in 0 file(s)
        naming no file       0 record(s)
      the records            0 bytes in 0 record(s)
      the logs               0 bytes in 0 log(s)
        could not be read    0 log(s): UNKNOWN, never nought
        no log ever written  0 record(s)
      measured in all        0 bytes
      next to expire         none: no record of this class is inside its span

  producer
    span                     0: these records NEVER EXPIRE
    records of this class    1
      not over, so not ours  0 (the reaper's, ORCH-012 and D134)
      judged                 1
    past the span            0 record(s)
      collected files        0 bytes in 0 file(s)
        naming no file       0 record(s)
      the records            0 bytes in 0 record(s)
      the logs               0 bytes in 0 log(s)
        could not be read    0 log(s): UNKNOWN, never nought
        no log ever written  0 record(s)
      measured in all        0 bytes
      oldest                 none: no record of this class is past its span
      newest                 none
    inside the span          1 record(s)
      collected files        3 bytes in 1 file(s)
        naming no file       0 record(s)
      the records            512 bytes in 1 record(s)
      the logs               0 bytes in 0 log(s)
        could not be read    0 log(s): UNKNOWN, never nought
        no log ever written  1 record(s)
      measured in all        515 bytes
      next to expire         never: this class's span is 0

  in all                   4 of the 6 record(s) that can expire are past their span
    records judged         7 (a class whose span is 0 is judged and never expires)
  what that would free, over the whole root
    collected files        1122 bytes (1.10 KiB) in 5 file(s)
      naming no file       0 record(s)
    the records            2216 bytes (2.16 KiB) in 4 record(s)
    the logs               67111164 bytes (64.00 MiB) in 3 log(s)
      could not be read    1 log(s): UNKNOWN, never nought
        jobs/j0000000003: there is no log, and the log.seq beside it is this hub's own record that a log was being counted, so its bytes are unknown and not nought
      no log ever written  0 record(s)
    measured in all        67114502 bytes (64.01 MiB)
  what it would keep, over the whole root
    collected files        3 bytes in 1 file(s)
      naming no file       2 record(s)
    the records            1578 bytes (1.54 KiB) in 3 record(s)
    the logs               1234 bytes (1.21 KiB) in 1 log(s)
      could not be read    0 log(s): UNKNOWN, never nought
      no log ever written  2 record(s)
    measured in all        2815 bytes (2.75 KiB)
  Nothing above was removed, and this subcommand cannot remove it.
";
