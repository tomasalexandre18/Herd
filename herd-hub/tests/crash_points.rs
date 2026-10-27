//! ORCH-080: every on-disk state a power cut can leave in one job
//! directory, **built by construction**, and what `JobDirStore::open` does
//! with each one.
//!
//! ## Why by construction and not by crashing
//!
//! Killing a hub produces *one* of the states below, chosen by the kernel's
//! write-back and by where the clock landed. It cannot be asked for the
//! other fifty-three, and a state it does produce cannot be reproduced on
//! demand. So nothing here crashes anything: each state is written byte for
//! byte into a temporary directory this file owns, the store is opened on
//! it, and what the store does is asserted. **No hub process is started, no
//! signal is sent, no process is killed, and no path outside
//! `tempfile::tempdir()` is written.**
//!
//! ## The write under study
//!
//! `JobDirStore::record_waiting` (`store.rs:2020`) is the write the
//! remediation dossier's steps A, B and C are about. It performs, in order:
//!
//! 1. `write_record` → `durable::write_file_with(job.json)`
//!    (`durable.rs:35-57`): create `.job.json.tmp-<pid>-<n>`, write, **R**
//!    `file.sync_all()`, `rename` onto `job.json`, **D** `sync_dir(parent)`.
//! 2. `append_event` → `durable::append_line(events.jsonl)`
//!    (`durable.rs:64-72`): one `write_all` of the line plus its terminator,
//!    **E** `file.sync_data()`.
//!
//! A power cut at an arbitrary instant in that sequence leaves exactly three
//! things to vary, and this file varies all three independently:
//!
//! * **`job.json`** — the older bytes (the rename is not durable), the newer
//!   bytes (it is), or none of the above: torn, empty, NUL-filled, absent, or
//!   a version this hub does not read. The last five are not reachable from
//!   `write_file_with` while **R** is kept — the contents are fsynced before
//!   the name — which is why they are enumerated separately and why the
//!   report attributes them to step **C** and to the filesystem, not to step
//!   **B**.
//! * **`events.jsonl`** — any byte prefix of the intended final content, plus
//!   the two shapes a journalling filesystem can hand back that a plain
//!   `truncate` cannot: a whole, newline-terminated line of NULs, and a tail
//!   that is not UTF-8.
//! * **the stale temporary** — `.job.json.tmp-<pid>-<n>`
//!   (`durable.rs:235-242`), absent, half-written, or whole; and the same
//!   thing left in `files/` by the inline-output write at `store.rs:1752`.
//!
//! ## Why the list is complete
//!
//! The hub's startup decision is a function of very little. `recover`
//! (`store.rs:860`) reads `job.json` and nothing else — `events.jsonl` is
//! read only by `job_events` (`store.rs:2310`), on a query, never at open —
//! and `job_events`' decision per line is a single test, `line.ends_with('\n')`
//! (`store.rs:2321`). So:
//!
//! * two prefixes of `events.jsonl` that end strictly inside the *same* line
//!   are indistinguishable to the reader, and one representative per line
//!   covers the class;
//! * a prefix that ends on a line boundary is its own class, and there are
//!   `lines + 1` of them;
//! * nothing in the tree reads a temporary at all, so a temporary's contents
//!   are one class and its *existence* is the only variable.
//!
//! Three lines in the seed therefore give 4 boundary classes + 3 mid-line
//! classes + 2 filesystem classes = 9 shapes of `events.jsonl`; 7 shapes of
//! `job.json`; 4 shapes of temporary. The cross-product is enumerated below.
//!
//! ## Running it
//!
//! Every case is `#[ignore]`d, so `./scripts/test.sh` does not run it.
//!
//! ```sh
//! nix develop --command cargo test -p herd-hub --test crash_points \
//!   -- --ignored --nocapture --test-threads=1
//! ```

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use herd_hub::job::NewJob;
use herd_hub::{Budget, JobDirStore, JobStore};
use herd_proto::JobId;

const JOBS_DIR: &str = "jobs";
const RECORD_FILE: &str = "job.json";
const EVENTS_FILE: &str = "events.jsonl";
const KEPT_FILES_DIR: &str = "files";

/// The reason carried by the first waiting write, which is the *older*
/// record's.
const OLDER_REASON: &str = "orch080: the older reason, written first";
/// The reason carried by the second waiting write, which is the one a power
/// cut lands in the middle of.
const NEWER_REASON: &str = "orch080: the newer reason, interrupted";

// ---------------------------------------------------------------------------
// The seed: one job directory, photographed before and after one waiting write
// ---------------------------------------------------------------------------

/// A state root holding exactly one queued job, with the bytes of both sides
/// of the write under study kept so a case can put either on the disk.
struct Seed {
    dir: tempfile::TempDir,
    id: JobId,
    /// `job.json` as it was before the write under study.
    older_record: Vec<u8>,
    /// `job.json` as the write under study leaves it.
    newer_record: Vec<u8>,
    /// `events.jsonl` before the write: two whole lines.
    older_events: Vec<u8>,
    /// `events.jsonl` after it: three whole lines.
    newer_events: Vec<u8>,
}

impl Seed {
    /// Build the root, take both photographs, and **release the state root's
    /// lock** so a case can reopen the same root (D21: one hub per root).
    fn build() -> Self {
        let dir = tempfile::tempdir().expect("a temporary state root");
        let store = JobDirStore::open(dir.path()).expect("a fresh state root opens");
        let id = store
            .submit(NewJob::new(vec![
                "sleep".to_owned(),
                "orch080-never-runs".to_owned(),
            ]))
            .expect("an open job is minted")
            .record()
            .id
            .clone();

        assert!(
            store
                .record_waiting(&id, OLDER_REASON)
                .expect("the first waiting write lands"),
            "the first reason is news"
        );
        let job_dir = dir.path().join(JOBS_DIR).join(id.as_str());
        let older_record = read(&job_dir.join(RECORD_FILE));
        let older_events = read(&job_dir.join(EVENTS_FILE));

        assert!(
            store
                .record_waiting(&id, NEWER_REASON)
                .expect("the second waiting write lands"),
            "the second reason is news"
        );
        let newer_record = read(&job_dir.join(RECORD_FILE));
        let newer_events = read(&job_dir.join(EVENTS_FILE));

        // The store goes before anything else does: dropping it closes the
        // `flock`ed lock file, which is what lets a case reopen this root.
        drop(store);

        // `line_ends` yields one offset per whole line plus the leading 0.
        assert_eq!(
            line_ends(&older_events).len() - 1,
            2,
            "the older events are `queued` and the first `waiting`"
        );
        assert_eq!(
            line_ends(&newer_events).len() - 1,
            3,
            "the newer events add the second `waiting`"
        );
        assert_ne!(older_record, newer_record, "the two records differ");

        Self {
            dir,
            id,
            older_record,
            newer_record,
            older_events,
            newer_events,
        }
    }

    fn job_dir(&self) -> PathBuf {
        self.dir.path().join(JOBS_DIR).join(self.id.as_str())
    }
}

// ---------------------------------------------------------------------------
// The three axes
// ---------------------------------------------------------------------------

/// What `job.json` holds when the hub next opens the root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Record {
    /// The rename of the interrupted write never reached the disk.
    Older,
    /// The rename reached the disk; this is the write's own result.
    Newer,
    /// A prefix of the newer record. **Not** reachable from
    /// `write_file_with` while its `sync_all` is kept; reachable if the
    /// record `fsync` (step C) is dropped, or from a filesystem that
    /// publishes a rename ahead of the bytes it names.
    Torn,
    /// Zero bytes under the right name — the same hazard, at its limit.
    Empty,
    /// The right length, NUL bytes: what a journal replay can leave where a
    /// block was allocated and never written.
    Nulled,
    /// The directory was made and the first record never landed: an
    /// interrupted mint.
    Absent,
    /// Readable JSON, a `record_version` this hub does not read. Not a crash
    /// state; enumerated because it is the one *other* way `read_stored`
    /// returns `Err`, and it shares the refusal.
    WrongVersion,
}

/// What `events.jsonl` holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Events {
    /// A prefix ending exactly at the end of line `n` — `0` is an empty file.
    Boundary(usize),
    /// A prefix ending strictly inside line `n` (1-based).
    MidLine(usize),
    /// Lines 1 and 2 whole, then a third line of NULs **with** its newline:
    /// a whole line that is not JSON.
    NulWholeLine,
    /// Lines 1 and 2 whole, then NULs and no newline: a torn tail of NULs.
    NulTornTail,
    /// Lines 1 and 2 whole, then two bytes that are not UTF-8.
    NotUtf8,
    /// The file is not there at all.
    Absent,
}

/// What the interrupted write left beside the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Temporary {
    /// Nothing: the rename completed, or the crash came before `File::create`.
    None,
    /// `.job.json.tmp-…` holding half the record's bytes.
    HalfWritten,
    /// `.job.json.tmp-…` holding the whole record, the crash landing between
    /// the `sync_all` and the `rename`.
    Whole,
    /// The same leftover from the *other* atomic writer — the inline-output
    /// write at `store.rs:1752` — inside `files/`.
    InKeptFiles,
}

/// The name `durable::temporary_beside` would have given it
/// (`durable.rs:235-242`): a leading dot, the destination's name, `.tmp-`,
/// a pid and a counter.
fn temporary_name(destination: &str) -> String {
    format!(".{destination}.tmp-424242-0")
}

// ---------------------------------------------------------------------------
// One case
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Case {
    record: Record,
    events: Events,
    temporary: Temporary,
}

impl fmt::Display for Case {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "job.json={:?} events.jsonl={:?} temporary={:?}",
            self.record, self.events, self.temporary
        )
    }
}

/// What the hub did when it opened the root this case built.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// `JobDirStore::open` returned `Err`: **the whole state root is
    /// refused**, every other job on it with it.
    Refused(String),
    /// It opened. The job is in `unfinished_jobs()` with this reason, or it
    /// is not there at all (`None`), or it is quarantined.
    Opened {
        queued_with: Option<String>,
        quarantined: bool,
        /// What `job_events` answered: how many events, or the error.
        events: std::result::Result<usize, String>,
        /// Whether the stale temporary is still on the disk afterwards.
        temporary_left: bool,
        /// What the opening sweep counted against the output budget.
        held_bytes: u64,
    },
}

impl Verdict {
    fn headline(&self) -> &'static str {
        match self {
            Verdict::Refused(_) => "REFUSED",
            Verdict::Opened {
                quarantined: true, ..
            } => "quarantined",
            Verdict::Opened {
                queued_with: None, ..
            } => "opened, job gone",
            Verdict::Opened { .. } => "opened",
        }
    }
}

/// Build the case's state root, open a hub store on it, and report.
fn run(case: Case) -> Verdict {
    let seed = Seed::build();
    let job_dir = seed.job_dir();

    // --- job.json ---------------------------------------------------------
    let record_path = job_dir.join(RECORD_FILE);
    match case.record {
        Record::Older => fs::write(&record_path, &seed.older_record).expect("written"),
        Record::Newer => fs::write(&record_path, &seed.newer_record).expect("written"),
        Record::Torn => {
            let cut = seed.newer_record.len() * 2 / 3;
            fs::write(&record_path, &seed.newer_record[..cut]).expect("written");
        }
        Record::Empty => fs::write(&record_path, b"").expect("written"),
        Record::Nulled => {
            fs::write(&record_path, vec![0u8; seed.newer_record.len()]).expect("written")
        }
        Record::Absent => fs::remove_file(&record_path).expect("removed"),
        Record::WrongVersion => {
            fs::write(&record_path, br#"{"record_version":99,"record":{}}"#).expect("written")
        }
    }

    // --- events.jsonl -----------------------------------------------------
    let events_path = job_dir.join(EVENTS_FILE);
    let ends = line_ends(&seed.newer_events);
    let bytes: Option<Vec<u8>> = match case.events {
        Events::Boundary(n) => Some(seed.newer_events[..ends[n]].to_vec()),
        Events::MidLine(n) => {
            let from = ends[n - 1];
            let to = ends[n];
            // Strictly inside: at least one byte of the line, never its
            // terminator.
            let cut = from + (to - from) / 2;
            assert!(cut > from && cut < to, "a mid-line byte exists");
            Some(seed.newer_events[..cut].to_vec())
        }
        Events::NulWholeLine => {
            let mut out = seed.newer_events[..ends[2]].to_vec();
            out.extend(vec![0u8; ends[3] - ends[2] - 1]);
            out.push(b'\n');
            Some(out)
        }
        Events::NulTornTail => {
            let mut out = seed.newer_events[..ends[2]].to_vec();
            out.extend(vec![0u8; (ends[3] - ends[2]) / 2]);
            Some(out)
        }
        Events::NotUtf8 => {
            let mut out = seed.newer_events[..ends[2]].to_vec();
            out.extend_from_slice(&[0xff, 0xfe]);
            Some(out)
        }
        Events::Absent => None,
    };
    match bytes {
        Some(bytes) => fs::write(&events_path, bytes).expect("written"),
        None => fs::remove_file(&events_path).expect("removed"),
    }

    // --- the stale temporary ----------------------------------------------
    let temporary = match case.temporary {
        Temporary::None => None,
        Temporary::HalfWritten => {
            let path = job_dir.join(temporary_name(RECORD_FILE));
            let cut = seed.newer_record.len() / 2;
            fs::write(&path, &seed.newer_record[..cut]).expect("written");
            Some(path)
        }
        Temporary::Whole => {
            let path = job_dir.join(temporary_name(RECORD_FILE));
            fs::write(&path, &seed.newer_record).expect("written");
            Some(path)
        }
        Temporary::InKeptFiles => {
            let path = job_dir
                .join(KEPT_FILES_DIR)
                .join(temporary_name("results.json"));
            fs::write(&path, vec![b'x'; 4096]).expect("written");
            Some(path)
        }
    };

    // --- open it ----------------------------------------------------------
    // A budget far above anything here, so the opening sweep measures and
    // evicts nothing: what is wanted is its figure, not its judgement.
    let budget = Budget {
        bytes: 1 << 30,
        ..Budget::default()
    };
    let store = match JobDirStore::open_with(seed.dir.path(), budget) {
        Ok(store) => store,
        Err(error) => return Verdict::Refused(error.to_string()),
    };

    let queued_with = store
        .unfinished_jobs()
        .expect("the queue lists")
        .into_iter()
        .find(|record| record.id == seed.id)
        .and_then(|record| record.waiting_for);
    let quarantined = store
        .quarantined()
        .expect("the quarantine lists")
        .iter()
        .any(|(id, _)| *id == seed.id);
    let events = store
        .job_events(&seed.id)
        .map(|found| found.map(|events| events.len()).unwrap_or(0))
        .map_err(|error| error.to_string());
    let held_bytes = store.sweep_outputs().expect("the sweep runs").held;
    let temporary_left = temporary.as_ref().is_some_and(|path| path.exists());

    Verdict::Opened {
        queued_with,
        quarantined,
        events,
        temporary_left,
        held_bytes,
    }
}

// ---------------------------------------------------------------------------
// What each axis is expected to produce, independently of the others
// ---------------------------------------------------------------------------

/// How many events `job_events` should read back, or `None` when it should
/// refuse the file.
fn events_expected(shape: Events) -> Option<usize> {
    match shape {
        Events::Boundary(n) => Some(n),
        // `job_events` stops at the first line with no terminator
        // (`store.rs:2320-2331`), so a prefix ending inside line `n` yields
        // the `n - 1` lines before it.
        Events::MidLine(n) => Some(n - 1),
        // A whole line that is not JSON is an error and not a torn tail:
        // `store.rs:2336-2337`.
        Events::NulWholeLine => None,
        Events::NulTornTail => Some(2),
        // `read_optional` is `read_to_string` (`store.rs:2653-2658`), so a
        // tail that is not UTF-8 refuses the whole file, not its last line.
        Events::NotUtf8 => None,
        Events::Absent => Some(0),
    }
}

/// Whether `JobDirStore::open` should refuse the whole state root.
fn open_refused(record: Record) -> bool {
    matches!(
        record,
        Record::Torn | Record::Empty | Record::Nulled | Record::WrongVersion
    )
}

// ---------------------------------------------------------------------------
// The enumeration
// ---------------------------------------------------------------------------

fn every_case() -> Vec<Case> {
    let records = [
        Record::Older,
        Record::Newer,
        Record::Torn,
        Record::Empty,
        Record::Nulled,
        Record::Absent,
        Record::WrongVersion,
    ];
    let events = [
        Events::Boundary(0),
        Events::Boundary(1),
        Events::Boundary(2),
        Events::Boundary(3),
        Events::MidLine(1),
        Events::MidLine(2),
        Events::MidLine(3),
        Events::NulWholeLine,
        Events::NulTornTail,
        Events::NotUtf8,
        Events::Absent,
    ];
    let temporaries = [
        Temporary::None,
        Temporary::HalfWritten,
        Temporary::Whole,
        Temporary::InKeptFiles,
    ];
    let mut cases = Vec::new();
    for record in records {
        for events in events {
            for temporary in temporaries {
                cases.push(Case {
                    record,
                    events,
                    temporary,
                });
            }
        }
    }
    cases
}

/// The whole cross-product, run and tabulated.
///
/// This is the evidence ORCH-084 (step **B**, dropping the directory
/// `fsync`) is to be judged on. It asserts the behaviour rather than only
/// printing it, so the table cannot drift from the code without the case
/// going red.
#[test]
#[ignore = "ORCH-080's enumeration; run with --ignored"]
fn every_crash_point_a_waiting_write_can_leave() {
    let cases = every_case();
    println!(
        "\nORCH-080 — {} constructed states, one job directory each\n",
        cases.len()
    );
    println!(
        "{:<14} {:<16} {:<13} {:<16} {:>7} {:>5} {:>7}",
        "job.json", "events.jsonl", "temporary", "open", "events", "temp", "held"
    );
    println!("{}", "-".repeat(84));

    let mut refusals = 0usize;
    let mut vanished = 0usize;
    let mut ahead = 0usize;
    let mut temporaries_left = 0usize;

    for case in &cases {
        let verdict = run(*case);
        match &verdict {
            Verdict::Refused(why) => {
                refusals += 1;
                assert!(
                    open_refused(case.record),
                    "{case} refused the whole state root and was not expected to: {why}"
                );
                assert!(
                    why.contains(RECORD_FILE),
                    "a refusal names the file it could not read: {why}"
                );
                println!(
                    "{:<14} {:<16} {:<13} {:<16} {:>7} {:>5} {:>7}",
                    format!("{:?}", case.record),
                    format!("{:?}", case.events),
                    format!("{:?}", case.temporary),
                    verdict.headline(),
                    "-",
                    "-",
                    "-"
                );
            }
            Verdict::Opened {
                queued_with,
                quarantined,
                events,
                temporary_left,
                held_bytes,
            } => {
                assert!(
                    !open_refused(case.record),
                    "{case} opened and was expected to be refused"
                );
                assert!(
                    !quarantined,
                    "{case}: D52's quarantine is for labels only and must not \
                     claim a crash state (store.rs:1150-1157)"
                );

                // The record axis, on its own.
                match case.record {
                    Record::Older => assert_eq!(
                        queued_with.as_deref(),
                        Some(OLDER_REASON),
                        "{case}: a lost rename leaves the older record in the queue"
                    ),
                    Record::Newer => assert_eq!(
                        queued_with.as_deref(),
                        Some(NEWER_REASON),
                        "{case}: a durable rename leaves the newer record in the queue"
                    ),
                    Record::Absent => {
                        assert!(
                            queued_with.is_none(),
                            "{case}: a job directory with no record holds no job"
                        );
                        vanished += 1;
                    }
                    other => unreachable!("{other:?} does not open"),
                }

                // The events axis, on its own — and it really is independent:
                // the assertion does not look at the record.
                match (events_expected(case.events), events) {
                    (Some(want), Ok(got)) => assert_eq!(
                        *got, want,
                        "{case}: `job_events` reads the whole lines and stops"
                    ),
                    (None, Err(why)) => assert!(
                        why.contains(EVENTS_FILE),
                        "{case}: a refused events file is named: {why}"
                    ),
                    (want, got) => panic!("{case}: expected {want:?}, got {got:?}"),
                }

                // D103: `events.jsonl` is never *ahead* of `job.json`.
                // Falling behind is permitted; this counts the states where
                // it is in front.
                if case.record == Record::Older && case.events == Events::Boundary(3) {
                    ahead += 1;
                }

                // The temporary axis, on its own. Nothing removes one.
                match case.temporary {
                    Temporary::None => assert!(!temporary_left, "{case}: there was none"),
                    _ => {
                        assert!(
                            *temporary_left,
                            "{case}: nothing in the tree removes a stale temporary at \
                             `JobDirStore::open` (ORCH-086)"
                        );
                        temporaries_left += 1;
                    }
                }

                // And one of the four is not merely left behind: it is
                // counted against the output budget, for ever.
                let want_held = if case.temporary == Temporary::InKeptFiles {
                    4096
                } else {
                    0
                };
                assert_eq!(
                    *held_bytes, want_held,
                    "{case}: `directory_bytes` (store.rs:2733-2749) has no dot-filter, \
                     so a temporary left in `files/` is held output"
                );

                println!(
                    "{:<14} {:<16} {:<13} {:<16} {:>7} {:>5} {:>7}",
                    format!("{:?}", case.record),
                    format!("{:?}", case.events),
                    format!("{:?}", case.temporary),
                    verdict.headline(),
                    match events {
                        Ok(n) => n.to_string(),
                        Err(_) => "ERR".to_owned(),
                    },
                    if *temporary_left { "left" } else { "-" },
                    held_bytes
                );
            }
        }
    }

    println!("\n{} states built", cases.len());
    println!("  {refusals} refuse the whole state root at `JobDirStore::open`");
    println!("  {vanished} open with the job silently gone from the queue");
    println!("  {ahead} leave `events.jsonl` AHEAD of `job.json` (D103's one rule)");
    println!("  {temporaries_left} leave a stale temporary that nothing removes");
    println!("  0 are quarantined (D52 covers labels only, store.rs:1150-1157)\n");

    // 4 of the 7 record shapes refuse, in every combination of the other two
    // axes: 4 x 11 x 4.
    assert_eq!(
        refusals,
        4 * 11 * 4,
        "the refusal is a property of the record alone"
    );
    assert_eq!(vanished, 11 * 4, "so is the vanishing");
    assert_eq!(ahead, 4, "one event shape runs ahead of the older record");
    assert_eq!(
        temporaries_left,
        3 * 11 * 3,
        "three shapes of temporary, three records that open"
    );
}

// ---------------------------------------------------------------------------
// The findings, each on its own, so a reader can run one
// ---------------------------------------------------------------------------

/// **The headline.** A `job.json` this hub cannot parse does not quarantine
/// one job: it refuses the whole state root, and the hub does not start.
///
/// The chain, at `2ca6dc7`: `store.rs:1141-1142` (the `?` on
/// `serde_json::from_str`) → `store.rs:899` (`self.read_stored(&id)?` in
/// `recover`) → `store.rs:436` (`store.recover()?` in `open_with`).
///
/// It is not avoidable by the index, either. `usable_index` reads every
/// record the index leaves out, and an unreadable one there sets the index
/// aside (`store.rs:1052-1054`) so that `recover` reads it again and refuses.
#[test]
#[ignore = "ORCH-080's enumeration; run with --ignored"]
fn a_torn_record_refuses_the_whole_state_root() {
    for shape in [
        Record::Torn,
        Record::Empty,
        Record::Nulled,
        Record::WrongVersion,
    ] {
        let verdict = run(Case {
            record: shape,
            events: Events::Boundary(3),
            temporary: Temporary::None,
        });
        match verdict {
            Verdict::Refused(why) => {
                println!("{shape:?}: the hub will not start — {why}");
                assert!(why.contains(RECORD_FILE));
            }
            other => panic!("{shape:?} was expected to refuse, got {}", other.headline()),
        }
    }
}

/// A second job on the same root goes down with the first. This is the
/// operational fact: the blast radius of one unreadable record is the whole
/// hub, and the consumer runs it in docker compose and cannot log into
/// anything of ours (invariant 9, D161).
#[test]
#[ignore = "ORCH-080's enumeration; run with --ignored"]
fn one_torn_record_takes_every_other_job_with_it() {
    let dir = tempfile::tempdir().expect("a temporary state root");
    let store = JobDirStore::open(dir.path()).expect("a fresh state root opens");
    let mut ids = Vec::new();
    for i in 0..3 {
        ids.push(
            store
                .submit(NewJob::new(vec![
                    "sleep".to_owned(),
                    format!("orch080-{i}"),
                ]))
                .expect("minted")
                .record()
                .id
                .clone(),
        );
    }
    drop(store);

    // The middle one only.
    let victim = dir
        .path()
        .join(JOBS_DIR)
        .join(ids[1].as_str())
        .join(RECORD_FILE);
    let whole = read(&victim);
    fs::write(&victim, &whole[..whole.len() / 2]).expect("torn");

    let error = JobDirStore::open(dir.path()).expect_err("the hub refuses the root");
    println!("three jobs, one torn record: {error}");
    assert!(error.to_string().contains(ids[1].as_str()));
}

/// **Step B's hazard, named.** The rename is lost and the event is not: the
/// record says the older reason while `events.jsonl` already carries the
/// newer one. `events.jsonl` is *ahead* of `job.json`, which is the one
/// ordering D103 forbids — and the hub opens, says nothing, and repairs
/// nothing.
///
/// Today this state is unreachable: `write_file_with`'s `sync_dir`
/// (`durable.rs:56`) returns before `append_line` is issued, so the rename is
/// on the platter before the event exists. **Dropping that directory `fsync`
/// is what makes this state reachable**, which is why ORCH-084 is held behind
/// this enumeration.
#[test]
#[ignore = "ORCH-080's enumeration; run with --ignored"]
fn an_event_ahead_of_the_record_is_neither_seen_nor_repaired() {
    let verdict = run(Case {
        record: Record::Older,
        events: Events::Boundary(3),
        temporary: Temporary::None,
    });
    let Verdict::Opened {
        queued_with,
        events,
        quarantined,
        ..
    } = verdict
    else {
        panic!("the hub opens on this state");
    };
    assert_eq!(queued_with.as_deref(), Some(OLDER_REASON));
    assert_eq!(
        events,
        Ok(3),
        "three whole lines, one more than the record knows"
    );
    assert!(!quarantined, "nothing notices");
    println!(
        "job.json says {OLDER_REASON:?}; events.jsonl's third line says {NEWER_REASON:?}; \
         the hub opens and reports neither"
    );

    // And the disagreement is readable, which is what makes it a diagnostic
    // hazard rather than a harmless one: a reader following the history is
    // told something the record denies.
    let seed = Seed::build();
    let text = String::from_utf8(seed.newer_events.clone()).expect("utf-8");
    let last: serde_json::Value =
        serde_json::from_str(text.lines().last().expect("a third line")).expect("json");
    assert_eq!(last["reason"], NEWER_REASON);
}

/// **Step A's hazard, named, and it is the permitted one.** The record is the
/// new one and the event never landed: `events.jsonl` is *behind*
/// `job.json`. D103 permits exactly this, and the hub opens with the job
/// queued under the reason the record carries.
#[test]
#[ignore = "ORCH-080's enumeration; run with --ignored"]
fn a_record_renamed_with_no_event_appended_is_permitted_and_silent() {
    for events in [Events::Boundary(2), Events::MidLine(3), Events::NulTornTail] {
        let verdict = run(Case {
            record: Record::Newer,
            events,
            temporary: Temporary::None,
        });
        let Verdict::Opened {
            queued_with,
            events: read_back,
            ..
        } = verdict
        else {
            panic!("the hub opens on {events:?}");
        };
        assert_eq!(queued_with.as_deref(), Some(NEWER_REASON));
        assert_eq!(
            read_back,
            Ok(2),
            "{events:?}: the torn third line is not an event"
        );
    }
}

/// **ORCH-086, verified.** Nothing in the tree removes a stale temporary at
/// `JobDirStore::open`, and `names_in`'s dot-filter (`durable.rs:216`) is
/// what keeps it out of sight rather than off the disk.
///
/// One leftover *is* removed at open — `files.gone`, by `sweep_now`
/// (`store.rs:654-670`) — so the absence is a gap in a mechanism that
/// exists, not a mechanism that was never built.
#[test]
#[ignore = "ORCH-080's enumeration; run with --ignored"]
fn nothing_removes_a_stale_temporary_at_open() {
    for temporary in [
        Temporary::HalfWritten,
        Temporary::Whole,
        Temporary::InKeptFiles,
    ] {
        let verdict = run(Case {
            record: Record::Newer,
            events: Events::Boundary(3),
            temporary,
        });
        let Verdict::Opened {
            temporary_left,
            held_bytes,
            ..
        } = verdict
        else {
            panic!("the hub opens");
        };
        assert!(temporary_left, "{temporary:?} survives `JobDirStore::open`");
        println!("{temporary:?}: still on the disk after open, held = {held_bytes} bytes");
    }
}

/// And the one that is not merely untidy: a temporary left in `files/` by the
/// inline-output write (`store.rs:1752`) is counted against the output budget
/// by `directory_bytes` (`store.rs:2733-2749`, which has no dot-filter) and is
/// never swept, because the sweeper only ever removes a whole job's `files/`
/// through the records it ranks — and this job holds no collection at all.
#[test]
#[ignore = "ORCH-080's enumeration; run with --ignored"]
fn a_temporary_in_files_is_held_output_for_ever() {
    let verdict = run(Case {
        record: Record::Newer,
        events: Events::Boundary(3),
        temporary: Temporary::InKeptFiles,
    });
    let Verdict::Opened {
        held_bytes,
        temporary_left,
        ..
    } = verdict
    else {
        panic!("the hub opens");
    };
    assert_eq!(
        held_bytes, 4096,
        "the sweep counts the temporary as held output"
    );
    assert!(temporary_left, "and does not take it");
}

/// A job directory whose record never landed is not an error and not a
/// quarantine: the job is gone, the id stays spent, and the only trace is a
/// line on stderr (`store.rs:925-928`).
#[test]
#[ignore = "ORCH-080's enumeration; run with --ignored"]
fn an_interrupted_mint_loses_the_job_quietly() {
    let verdict = run(Case {
        record: Record::Absent,
        events: Events::Boundary(3),
        temporary: Temporary::None,
    });
    let Verdict::Opened {
        queued_with,
        quarantined,
        events,
        ..
    } = verdict
    else {
        panic!("the hub opens");
    };
    assert!(queued_with.is_none(), "the job is not in the queue");
    assert!(!quarantined, "and it is not on the quarantine list either");
    assert_eq!(
        events,
        Ok(3),
        "its `events.jsonl` is still there and still readable, naming a job the hub no longer has"
    );
}

/// `job_events`' two refusals, separated. A torn *tail* is dropped and is not
/// an error (`store.rs:2320-2331`); a whole line that is not JSON, and a file
/// that is not UTF-8, are errors (`store.rs:2336-2337` and `store.rs:2653`).
/// Neither reaches `JobDirStore::open`, which never reads the file — so both
/// are a failed query on one job, not a hub that will not start.
#[test]
#[ignore = "ORCH-080's enumeration; run with --ignored"]
fn a_whole_line_that_is_not_json_refuses_the_query_and_not_the_hub() {
    for shape in [Events::NulWholeLine, Events::NotUtf8] {
        let verdict = run(Case {
            record: Record::Newer,
            events: shape,
            temporary: Temporary::None,
        });
        let Verdict::Opened {
            queued_with,
            events,
            ..
        } = verdict
        else {
            panic!("{shape:?}: the hub still opens — `recover` never reads `events.jsonl`");
        };
        assert_eq!(queued_with.as_deref(), Some(NEWER_REASON));
        let why = events.expect_err("the query refuses");
        println!("{shape:?}: open succeeds, `job_events` says — {why}");
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The offset just past each `\n`, so `ends[n]` is the length of the first
/// `n` whole lines and `ends[0]` is 0.
fn line_ends(bytes: &[u8]) -> Vec<usize> {
    let mut ends = vec![0usize];
    for (at, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            ends.push(at + 1);
        }
    }
    ends
}

/// A guard against the harness drifting from the record it photographs: the
/// seed's two records really are the two sides of one waiting write, and the
/// events really do grow by one line.
#[test]
#[ignore = "ORCH-080's enumeration; run with --ignored"]
fn the_seed_is_the_write_under_study() {
    let seed = Seed::build();
    assert!(seed.older_events.len() < seed.newer_events.len());
    assert!(
        seed.newer_events.starts_with(&seed.older_events),
        "an append only ever adds to the end"
    );
    let older: serde_json::Value =
        serde_json::from_slice(&seed.older_record).expect("the older record is json");
    let newer: serde_json::Value =
        serde_json::from_slice(&seed.newer_record).expect("the newer record is json");
    // `RecordFile` flattens the record under its version (`store.rs:246-250`),
    // so the record's own fields are at the top level of `job.json`.
    assert_eq!(older["waiting_for"], OLDER_REASON);
    assert_eq!(newer["waiting_for"], NEWER_REASON);
    assert_eq!(
        newer["state"], older["state"],
        "the write under study changes the reason and not the state"
    );
    assert_eq!(newer["record_version"], older["record_version"]);

    // The third event line is the second `waiting`, and it is what a state
    // carrying the older record would be ahead of.
    let text = String::from_utf8(seed.newer_events).expect("utf-8");
    let line: serde_json::Value =
        serde_json::from_str(text.lines().last().expect("a third line")).expect("json");
    assert_eq!(line["event"], "waiting");
    assert_eq!(line["reason"], NEWER_REASON);
}
