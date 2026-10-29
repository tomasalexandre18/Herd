//! `herd-hub report`: every figure it prints, asserted to the byte (ORCH-125).
//!
//! Three things are being held here, and they are not the same thing:
//!
//! 1. **The arithmetic.** A fixture with the awkward cases in it — a digest
//!    shared by three jobs, a digest two records give two lengths, a
//!    zero-byte output, a file written before records carried digests, a
//!    record whose bytes the sweeper took, a record this build cannot read,
//!    and a job directory with no record yet — and every figure the report
//!    derives from it, exact, followed by the whole of its rendered text.
//! 2. **That it disturbs nothing.** The report runs while the `flock(2)` on
//!    `hub.lock` is held by somebody else, which is the state a live hub
//!    leaves that file in, and the state root is byte-identical afterwards.
//!    A report that could block the serving hub, or write to its root, would
//!    be worse than no report at all.
//! 3. **That it reads real records.** The fixture above is hand-written, so
//!    on its own it would only prove the report agrees with the test. The
//!    last test drives the store's own writer and reports on what *it* left.
//! 4. **That it names the jobs behind figure 3 and not only their sum**
//!    (ORCH-166) — the middle state a defect leaves; that the ten it names
//!    out of a longer pile are the ten **largest** and not the ten lowest
//!    ids; the tie-break under that; that a sound root prints no empty
//!    list; and that each of those runs leaves the root byte-identical too.

use std::fs;
use std::path::Path;

use herd_hub::job::JobStore;
use herd_hub::{ClaimShortfall, JobDirStore, Mint, NewJob, StateRootReport, Timestamp};
use herd_proto::{LabelSet, Priority};

/// The moment the rendered text is pinned to, so that the assertion is about
/// the figures and not about when it ran: 2026-10-28T00:00:00Z.
const PINNED: i64 = 1_793_145_600;

fn digest(byte: &str) -> String {
    byte.repeat(32)
}

/// Write one job directory: its `job.json`, and the real bytes the test
/// wants in its `files/`.
fn job_dir(root: &Path, name: &str, record: Option<serde_json::Value>, kept: &[(&str, &[u8])]) {
    let dir = root.join("jobs").join(name);
    fs::create_dir_all(&dir).expect("the job directory is made");
    if let Some(record) = record {
        fs::write(
            dir.join("job.json"),
            serde_json::to_vec_pretty(&record).expect("the record serialises"),
        )
        .expect("the record is written");
    }
    if !kept.is_empty() {
        for (path, bytes) in kept {
            let at = dir.join("files").join(path);
            fs::create_dir_all(at.parent().expect("a parent")).expect("room for the file");
            fs::write(&at, bytes).expect("the kept file is written");
        }
    }
}

/// A record naming no output at all, which is what most jobs are.
fn silent(id: &str) -> serde_json::Value {
    serde_json::json!({
        "record_version": 1,
        "id": id,
        "command": ["true"],
        "state": { "type": "finished", "outcome": { "type": "exited", "code": 0 } },
    })
}

/// One `KeptFile`, as `job.json` carries it.
fn kept(path: &str, bytes: u64, sha256: &str) -> serde_json::Value {
    if sha256.is_empty() {
        // A record written before the field existed: the digest is absent
        // from the JSON, not present and empty.
        return serde_json::json!({ "path": path, "bytes": bytes });
    }
    serde_json::json!({ "path": path, "bytes": bytes, "sha256": sha256 })
}

/// Everything in `jobs/`, and the real bytes beside it.
///
/// The names are deliberately in the hub's own zero-padded shape, because
/// the report prints them and an operator pastes them into a `ls`.
fn fixture(root: &Path) {
    let empty = herd_proto::sha256_hex(b"");
    let aa = digest("aa");
    let bb = digest("bb");
    let cc = digest("cc");

    for n in 1..=6 {
        let name = format!("j{n:0>10}");
        job_dir(root, &name, Some(silent(&name)), &[]);
    }
    // A sweep that was killed between the rename and the unlink (D152), and
    // a collection still arriving: real disk, outside the output budget,
    // and counted apart.
    fs::create_dir_all(root.join("jobs/j0000000005/files.gone")).expect("made");
    fs::write(root.join("jobs/j0000000005/files.gone/old.bin"), b"fifth").expect("written");
    fs::create_dir_all(root.join("jobs/j0000000006/files.part")).expect("made");
    fs::write(root.join("jobs/j0000000006/files.part/half.bin"), b"six").expect("written");

    // One zero-byte output. Its digest is the digest of nothing, which every
    // other empty file in the fleet shares, and it contributes nought bytes
    // to both sides of r_file: it moves the counts and not the ratio.
    let mut seven = silent("j0000000007");
    seven["outputs"] = serde_json::json!({ "files": [kept("files/empty", 0, &empty)] });
    job_dir(root, "j0000000007", Some(seven), &[("empty", b"")]);

    // The shared digest, three jobs over. `files/` holds seven bytes against
    // a manifest naming two million: the accounting gap figure 3 exists for.
    let mut eight = silent("j0000000008");
    eight["outputs"] = serde_json::json!({
        "files": [
            kept("files/a.bin", 2_000_000, &aa),
            kept("files/b.bin", 600, &bb),
        ]
    });
    job_dir(root, "j0000000008", Some(eight), &[("a.bin", b"a-short")]);

    // A record that outlived its bytes, and whose `files/` still holds some
    // all the same: a sweep that renamed and did not finish, which no other
    // figure here would show.
    let mut nine = silent("j0000000009");
    nine["outputs"] = serde_json::json!({
        "files": [
            kept("files/a.bin", 2_000_000, &aa),
            // The same digest as j0000000008's `b.bin`, with a different
            // length. The same bytes cannot be two lengths: an accounting
            // defect, named with both figures.
            kept("files/b.bin", 500, &bb),
        ],
        "removed": {
            "at": "2026-10-20T11:22:33Z",
            "why": "removed under the 200 GB output budget",
            "bytes": 2_000_500u64,
        },
    });
    job_dir(
        root,
        "j0000000009",
        Some(nine),
        &[("left.bin", b"eleven byte")],
    );

    // Two identical files inside one collection, a unique one, and a file
    // from before records carried digests. No `files/` at all: the record
    // claims four and a half megabytes the disk does not have.
    let mut ten = silent("j0000000010");
    ten["outputs"] = serde_json::json!({
        "files": [
            kept("files/a.bin", 2_000_000, &aa),
            kept("files/copy/a.bin", 2_000_000, &aa),
            kept("files/c.bin", 500_000, &cc),
            kept("files/legacy.bin", 777, ""),
        ]
    });
    job_dir(root, "j0000000010", Some(ten), &[]);

    // A record from a layout this build does not read. Named, counted, and
    // left out of every figure — never read as a job that kept nothing.
    let mut eleven = silent("j0000000011");
    eleven["record_version"] = serde_json::json!(2);
    job_dir(root, "j0000000011", Some(eleven), &[]);

    // A directory claimed for a job whose first record has not landed. A
    // state, not a fault.
    job_dir(root, "j0000000012", None, &[]);
}

/// Every file under `root`, by path and length, so that "it wrote nothing"
/// is an assertion and not a claim.
fn listing(root: &Path) -> Vec<(String, u64)> {
    fn walk(at: &Path, base: &Path, into: &mut Vec<(String, u64)>) {
        let Ok(entries) = fs::read_dir(at) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let kind = entry.file_type().expect("a file type");
            if kind.is_dir() {
                walk(&path, base, into);
            } else {
                let length = entry.metadata().expect("metadata").len();
                let name = path
                    .strip_prefix(base)
                    .expect("under the root")
                    .to_string_lossy()
                    .into_owned();
                into.push((name, length));
            }
        }
    }
    let mut found = Vec::new();
    walk(root, root, &mut found);
    found.sort();
    found
}

#[test]
fn every_figure_of_the_report_is_this_fixture_to_the_byte() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();
    fixture(root);

    // The state a live hub leaves this file in. The report must run anyway:
    // if it took this lock it would refuse, and if it waited for it it would
    // wait for the hub to stop.
    let held = herd_hub::durable::lock_exclusive(&root.join("hub.lock"))
        .expect("the test takes the lock a running hub holds");
    let before = listing(root);

    let mut report = StateRootReport::read(root).expect("the report reads this root");

    // Figure 1, over every record this root holds.
    assert_eq!(report.ever.files, 8, "every file whose record has a digest");
    assert_eq!(report.ever.bytes, 8_501_100);
    assert_eq!(report.ever.digests, 4, "the empty one, aa, bb and cc");
    assert_eq!(
        report.ever.distinct_bytes, 2_500_600,
        "0 + 2000000 + 600 + 500000, bb at the larger of its two lengths"
    );
    // Figure 1 again, over what the records still claim: j0000000009's two
    // files are gone, so neither its copy of aa nor its bb is counted.
    assert_eq!(report.held.files, 6);
    assert_eq!(report.held.bytes, 6_500_600);
    assert_eq!(report.held.digests, 4);
    assert_eq!(report.held.distinct_bytes, 2_500_600);
    assert_eq!(report.undigested_files, 1);
    assert_eq!(report.undigested_bytes, 777);
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(report.conflicts[0].digest, digest("bb"));
    assert_eq!(report.conflicts[0].smaller, 500);
    assert_eq!(report.conflicts[0].larger, 600);

    // Figure 2. Seven of the ten records name nought bytes, so seven deciles
    // are nought — the shape a mean would hide.
    assert_eq!(report.sizes.population, 10);
    assert_eq!(report.sizes.naming_nothing, 6, "j0000000007 names one file");
    assert_eq!(
        report.sizes.deciles,
        [0, 0, 0, 0, 0, 0, 0, 2_000_500, 2_000_600]
    );
    assert_eq!(report.sizes.max, 4_500_777);
    assert_eq!(report.sizes.bytes, 8_501_877);
    assert_eq!(report.sizes.top_decile_jobs, 1);
    assert_eq!(report.sizes.top_decile_bytes, 4_500_777);

    // Figure 3.
    assert_eq!(report.accounting.claiming_jobs, 3);
    assert_eq!(report.accounting.claimed_bytes, 6_501_377);
    assert_eq!(report.accounting.removed_jobs, 1);
    assert_eq!(report.accounting.removed_bytes, 2_000_500);
    assert_eq!(
        report.accounting.measured_bytes, 18,
        "seven bytes under j0000000008 and eleven under j0000000009"
    );
    assert_eq!(
        report.accounting.gap(),
        (std::cmp::Ordering::Greater, 6_501_359)
    );
    assert_eq!(report.accounting.claimed_but_absent, vec!["j0000000010"]);
    // The middle state, which neither list either side of it catches:
    // j0000000008's manifest names 2000600 bytes and its `files/` holds seven
    // of them (ORCH-166).
    assert_eq!(
        report.accounting.claiming_more_than_held,
        vec![ClaimShortfall {
            job: "j0000000008".to_owned(),
            claimed: 2_000_600,
            measured: 7,
        }]
    );
    let shortfall = &report.accounting.claiming_more_than_held[0];
    assert_eq!(
        shortfall.shortfall(),
        shortfall.claimed - shortfall.measured,
        "the shortfall is that subtraction and nothing else"
    );
    assert_eq!(shortfall.shortfall(), 2_000_593);
    assert_eq!(
        report.accounting.removed_but_present,
        vec![("j0000000009".to_owned(), 11)]
    );
    assert_eq!(report.accounting.parts_dirs, 1);
    assert_eq!(report.accounting.parts_bytes, 3);
    assert_eq!(
        report.accounting.parts_jobs,
        vec![("j0000000006".to_owned(), 3)],
        "an unfinished collection is named, not only counted"
    );
    assert_eq!(report.accounting.gone_dirs, 1);
    assert_eq!(report.accounting.gone_bytes, 5);

    // The census, which is what makes every figure above a floor and not a
    // total: one record was not read, and the report says so rather than
    // counting it as a job that kept nothing (D27's rule, applied to a
    // record).
    assert_eq!(report.census.directories, 12);
    assert_eq!(report.census.read, 10);
    assert_eq!(report.census.without_record, 1);
    assert_eq!(report.census.unread.len(), 1);
    assert_eq!(report.census.unread[0].path, "jobs/j0000000011/job.json");

    // And the whole of the text, because the figures only reach this project
    // as text somebody pasted: a line that silently stopped printing a
    // number would be a figure nobody noticed going missing.
    report.at = Timestamp::at_unix_seconds(PINNED);
    let rendered = report.to_string();
    let expected = EXPECTED.replace("{ROOT}", &root.display().to_string());
    assert_eq!(rendered, expected);

    // It disturbed nothing: not one byte of this root changed, and the lock
    // a hub holds was held throughout.
    assert_eq!(listing(root), before, "the report writes nothing");
    drop(held);
}

/// A record claiming one file of `bytes` under `sha256`, and nothing else.
fn claiming(id: &str, bytes: u64, sha256: &str) -> serde_json::Value {
    let mut record = silent(id);
    record["outputs"] = serde_json::json!({ "files": [kept("files/a.bin", bytes, sha256)] });
    record
}

/// The defect state, named (ORCH-166).
///
/// Not a repeat of the fixture above: that one asserts the whole text, this
/// one asserts the *condition*, with the two neighbours of the middle state
/// in the same root so that the three lists are seen sorting a job each.
/// Before this, a job here was in no list at all and the operator was left
/// with a total.
#[test]
fn a_job_claiming_more_than_its_files_holds_is_named_with_both_figures() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();

    // The middle: 1000 claimed, 400 of them there.
    job_dir(
        root,
        "j0000000001",
        Some(claiming("j0000000001", 1_000, &digest("aa"))),
        &[("a.bin", &[b'x'; 400])],
    );
    // One extreme: claimed, and `files/` measures nought.
    job_dir(
        root,
        "j0000000002",
        Some(claiming("j0000000002", 1_000, &digest("bb"))),
        &[],
    );
    // Sound: the claim and the measurement agree to the byte.
    job_dir(
        root,
        "j0000000003",
        Some(claiming("j0000000003", 500, &digest("cc"))),
        &[("a.bin", &[b'y'; 500])],
    );

    let before = listing(root);
    let mut report = StateRootReport::read(root).expect("the report reads this root");

    assert_eq!(
        report.accounting.claiming_more_than_held,
        vec![ClaimShortfall {
            job: "j0000000001".to_owned(),
            claimed: 1_000,
            measured: 400,
        }],
        "the middle state is one job, and the two neighbours are not in it"
    );
    assert_eq!(report.accounting.claimed_but_absent, vec!["j0000000002"]);
    // The figures check by arithmetic, both inside the value and against the
    // totals: 1000 + 1000 + 500 claimed, 400 + 500 measured.
    let shortfall = &report.accounting.claiming_more_than_held[0];
    assert_eq!(shortfall.shortfall(), 600);
    assert_eq!(
        shortfall.shortfall(),
        shortfall.claimed - shortfall.measured
    );
    assert_eq!(report.accounting.claimed_bytes, 2_500);
    assert_eq!(report.accounting.measured_bytes, 900);
    assert_eq!(
        report.accounting.gap(),
        (std::cmp::Ordering::Greater, 1_600)
    );

    report.at = Timestamp::at_unix_seconds(PINNED);
    let rendered = report.to_string();
    assert!(
        rendered.contains("  claiming more than files/ holds     1 job(s)\n"),
        "the count line is there:\n{rendered}"
    );
    assert!(
        rendered.contains(
            "    jobs/j0000000001: claims 1000 bytes, files/ measures 400 bytes, \
             short by 600 bytes\n"
        ),
        "the job is named with its claim, its measurement and the shortfall:\n{rendered}"
    );

    assert_eq!(listing(root), before, "the report writes nothing");
}

/// The bound holds — `NAMED_AT_MOST` named, the rest counted — and the ten
/// that fit are the ten **largest**, not the ten lowest ids.
///
/// Those two are one test on purpose. The bound alone is the rule the unread
/// and conflict lists already follow, and the reason there is no flag to
/// widen it: a list exists to be acted on, and one naming every job is a
/// number, which the count already is. But a *truncated* list in job order
/// would name an arbitrary ten and leave the operator re-sorting by hand,
/// which is the shell loop this list exists to stop. So the fixture here
/// gives every job a different shortfall, **ascending with its id**: under
/// the old job order the three left out would have been the three worst, and
/// this test would fail.
#[test]
fn the_shortfall_list_names_the_ten_largest_and_counts_the_rest() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();

    // Thirteen jobs in the middle state. Each claims 1000 * n and holds one
    // byte, so job n is short by 1000 * n - 1 and the shortfalls rise with
    // the id. A digest of its own per job, because one digest given two
    // claimed lengths is a conflict and this test is not about those.
    for n in 1..=13u64 {
        let name = format!("j{n:0>10}");
        job_dir(
            root,
            &name,
            Some(claiming(&name, 1_000 * n, &digest(&format!("{n:02}")))),
            &[("a.bin", b"x")],
        );
    }

    let before = listing(root);
    let mut report = StateRootReport::read(root).expect("the report reads this root");
    let held = &report.accounting.claiming_more_than_held;
    assert_eq!(held.len(), 13);
    assert!(report.conflicts.is_empty());

    // The value is ordered, not just the text: largest first, and every
    // shortfall is the subtraction it claims to be.
    for (index, short) in held.iter().enumerate() {
        let nth = 13 - index as u64;
        assert_eq!(
            short.job,
            format!("j{nth:0>10}"),
            "position {index} of the list"
        );
        assert_eq!(short.claimed, 1_000 * nth);
        assert_eq!(short.measured, 1);
        assert_eq!(short.shortfall(), 1_000 * nth - 1);
    }
    assert!(
        held.windows(2)
            .all(|pair| pair[0].shortfall() >= pair[1].shortfall()),
        "the list never rises"
    );

    report.at = Timestamp::at_unix_seconds(PINNED);
    let rendered = report.to_string();
    assert!(
        rendered.contains("  claiming more than files/ holds     13 job(s)\n"),
        "the count is the whole thirteen:\n{rendered}"
    );
    // `NAMED_AT_MOST` is 10 in `report.rs`. Written out here rather than
    // imported, so that raising it there fails this test and somebody has to
    // say the list got longer on purpose.
    for nth in 4..=13 {
        let name = format!("jobs/j{nth:0>10}: claims ");
        assert!(
            rendered.contains(&name),
            "{name} is one of the ten largest and must be named:\n{rendered}"
        );
    }
    for nth in 1..=3 {
        let name = format!("jobs/j{nth:0>10}: claims ");
        assert!(
            !rendered.contains(&name),
            "{name} is one of the three smallest and must not be:\n{rendered}"
        );
    }
    assert!(
        rendered.contains("\n    and 3 more\n"),
        "the three past the bound are counted:\n{rendered}"
    );
    // And in that order in the text, which is what an operator reads.
    let first = rendered
        .find("jobs/j0000000013: claims ")
        .expect("the worst is in the text");
    let last = rendered
        .find("jobs/j0000000004: claims ")
        .expect("the tenth-worst is in the text");
    assert!(
        first < last,
        "the worst offender is printed before the tenth:\n{rendered}"
    );

    assert_eq!(listing(root), before, "the report writes nothing");
}

/// Two jobs equally short are named in job order.
///
/// The tie-break is what keeps the rendered text a function of its inputs
/// once the primary key stops being unique, and the whole-text assertion
/// above depends on there being one.
#[test]
fn jobs_equally_short_fall_back_to_job_order() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();

    // Two at 400 short and one at 900. The two ties are written in the
    // awkward order on purpose: `bb` before `aa` by digest, and the larger
    // id first, so neither insertion order nor the digest map can be what
    // the assertion below is reading.
    job_dir(
        root,
        "j0000000009",
        Some(claiming("j0000000009", 500, &digest("bb"))),
        &[("a.bin", &[b'y'; 100])],
    );
    job_dir(
        root,
        "j0000000002",
        Some(claiming("j0000000002", 500, &digest("aa"))),
        &[("a.bin", &[b'y'; 100])],
    );
    job_dir(
        root,
        "j0000000005",
        Some(claiming("j0000000005", 1_000, &digest("cc"))),
        &[("a.bin", &[b'y'; 100])],
    );

    let before = listing(root);
    let report = StateRootReport::read(root).expect("the report reads this root");
    assert_eq!(
        report
            .accounting
            .claiming_more_than_held
            .iter()
            .map(|short| (short.job.as_str(), short.shortfall()))
            .collect::<Vec<_>>(),
        vec![
            ("j0000000005", 900),
            ("j0000000002", 400),
            ("j0000000009", 400),
        ],
        "largest first, and the two equal ones in job order"
    );

    assert_eq!(listing(root), before, "the report writes nothing");
}

/// A root with neither a shortfall nor an unfinished collection prints the
/// two count lines at nought and not one line of a list under them.
///
/// The cheapest way for a report to become unreadable is a section that is
/// there whether or not it has anything in it.
#[test]
fn a_sound_root_prints_no_empty_list() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();

    job_dir(
        root,
        "j0000000001",
        Some(claiming("j0000000001", 500, &digest("aa"))),
        &[("a.bin", &[b'y'; 500])],
    );
    job_dir(root, "j0000000002", Some(silent("j0000000002")), &[]);

    let before = listing(root);
    let mut report = StateRootReport::read(root).expect("the report reads this root");
    assert!(report.accounting.claiming_more_than_held.is_empty());
    assert!(report.accounting.parts_jobs.is_empty());
    assert_eq!(report.accounting.parts_dirs, 0);
    assert_eq!(
        report.accounting.gap(),
        (std::cmp::Ordering::Equal, 0),
        "a sound accounting is the two figures agreeing to the byte"
    );

    report.at = Timestamp::at_unix_seconds(PINNED);
    let rendered = report.to_string();
    assert!(
        rendered.contains("  claiming more than files/ holds     0 job(s)\n"),
        "the count line is always printed, even at nought:\n{rendered}"
    );
    assert!(
        rendered.contains("  files.part/ unfinished collection   0 job(s), 0 bytes\n"),
        "and so is the unfinished-collection line:\n{rendered}"
    );
    assert!(
        !rendered.contains("Bytes were claimed and some of them are there"),
        "no shortfall, so no paragraph about shortfalls:\n{rendered}"
    );
    assert!(
        !rendered.contains("A collection that started and did not finish"),
        "no files.part/, so no paragraph about one:\n{rendered}"
    );
    assert!(
        !rendered.contains("    jobs/"),
        "no list has a line under it:\n{rendered}"
    );

    assert_eq!(listing(root), before, "the report writes nothing");
}

/// `files.part/` says what it is, and names the job (ORCH-166).
///
/// The old wording was "still arriving", which on the first consumer's hub
/// was false: the directory had sat untouched for six days through a reboot.
/// A report that asserts something it cannot know is worse than one that
/// says less, and this was the line that would have pointed straight at the
/// problem.
#[test]
fn an_unfinished_collection_is_named_and_claims_nothing_about_arrival() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path();

    job_dir(root, "j0000000001", Some(silent("j0000000001")), &[]);
    job_dir(root, "j0000000002", Some(silent("j0000000002")), &[]);
    fs::create_dir_all(root.join("jobs/j0000000002/files.part")).expect("made");
    fs::write(
        root.join("jobs/j0000000002/files.part/half.bin"),
        b"halfway",
    )
    .expect("written");

    let before = listing(root);
    let mut report = StateRootReport::read(root).expect("the report reads this root");
    assert_eq!(
        report.accounting.parts_jobs,
        vec![("j0000000002".to_owned(), 7)]
    );

    report.at = Timestamp::at_unix_seconds(PINNED);
    let rendered = report.to_string();
    assert!(
        rendered.contains("  files.part/ unfinished collection   1 job(s), 7 bytes\n"),
        "{rendered}"
    );
    assert!(
        rendered.contains("    jobs/j0000000002: 7 bytes\n"),
        "the job is named, not only counted:\n{rendered}"
    );
    assert!(
        !rendered.contains("still arriving"),
        "the report no longer claims the bytes are on their way:\n{rendered}"
    );

    assert_eq!(listing(root), before, "the report writes nothing");
}

#[test]
fn the_report_reads_what_the_store_itself_wrote() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let store = JobDirStore::open(dir.path()).expect("a state root");
    let shared = b"the same bytes in two jobs".to_vec();
    let mine = b"only here".to_vec();
    // Two different pieces of work, because two identical submissions join
    // into one job (E-05) and this test needs two records.
    for (nth, payload) in [&shared, &mine].into_iter().enumerate() {
        let job = store
            .create_job(NewJob::at(
                vec!["true".to_owned(), format!("{nth}")],
                LabelSet::default(),
                Priority::Step,
            ))
            .expect("a job is minted");
        store
            .begin_attempt(&job.id, "test-runner", "the only runner", None)
            .expect("an attempt is recorded");
        store
            .record_outputs(
                &job.id,
                &herd_proto::JobOutputs {
                    files: vec![
                        herd_proto::OutputFile::collected(
                            "files/shared.bin".to_owned(),
                            shared.clone(),
                        ),
                        herd_proto::OutputFile::collected(
                            "files/own.bin".to_owned(),
                            payload.clone(),
                        ),
                    ],
                    ..herd_proto::JobOutputs::default()
                },
            )
            .expect("the collection is recorded");
    }

    // The store is still open, so its `flock` is still held and its index is
    // still being written: exactly the hub this has to be safe against.
    let report = StateRootReport::read(dir.path()).expect("the report reads a live root");

    assert_eq!(report.census.directories, 2);
    assert_eq!(report.census.read, 2);
    assert!(report.census.unread.is_empty());
    // Four files, three distinct digests: `shared.bin` is the same bytes in
    // both jobs and the second job's `own.bin` is that same content again.
    assert_eq!(report.ever.files, 4);
    assert_eq!(
        report.ever.bytes,
        3 * shared.len() as u64 + mine.len() as u64
    );
    assert_eq!(report.ever.digests, 2);
    assert_eq!(
        report.ever.distinct_bytes,
        shared.len() as u64 + mine.len() as u64
    );
    assert!(
        report.conflicts.is_empty(),
        "the store never gives one digest two lengths"
    );
    assert_eq!(report.undigested_files, 0);

    // The one case where the records and the disk must agree exactly: every
    // byte was carried inline, nothing was swept, and nothing is in flight.
    assert_eq!(report.accounting.claimed_bytes, report.ever.bytes);
    assert_eq!(report.accounting.measured_bytes, report.ever.bytes);
    assert_eq!(
        report.accounting.gap(),
        (std::cmp::Ordering::Equal, 0),
        "a sound accounting is the two figures agreeing to the byte"
    );
    assert!(report.accounting.claimed_but_absent.is_empty());
    assert!(report.accounting.removed_but_present.is_empty());
    assert_eq!(report.accounting.parts_dirs, 0);
    assert_eq!(report.accounting.gone_dirs, 0);
    drop(store);
}

/// The report's text, with the only thing that varies left as `{ROOT}`.
const EXPECTED: &str = "\
herd-hub state-root report
  state root  {ROOT}
  taken at    2026-10-28T00:00:00Z

  A point-in-time snapshot. It takes no lock of any kind, so a hub may be
  writing this root while it is read: each record is read whole or not at
  all, but the figures together are not one instant. It reads no output
  byte and no log byte -- every size and digest in sections 1 and 2 comes
  out of a record, and section 3's measurements are file lengths.

records
  job directories                    12
  records read                       10
  directories with no record yet     1
  records this build could not read  1
    jobs/j0000000011/job.json: it carries record version 2, and this build reads version 1

1. file-level deduplication (r_file)

  The bytes the records name, over the bytes of their distinct sha256:
  what one copy per digest would weigh against what every copy weighs.
  Read it with the counts under it. A ratio on its own says nothing about
  how many files or how many bytes it is over.

  over every record this root holds, bytes still there or not
    files with a digest    8
    bytes they name        8501100 bytes (8.11 MiB)
    distinct sha256        4
    bytes over distinct    2500600 bytes (2.38 MiB)
    r_file                 3.40
  over the collections whose bytes these records still claim
    files with a digest    6
    bytes they name        6500600 bytes (6.20 MiB)
    distinct sha256        4
    bytes over distinct    2500600 bytes (2.38 MiB)
    r_file                 2.60
  files whose record carries no digest  1 (777 bytes)
    Written before a record carried one. Left out of both sides of both
    ratios: a byte with no digest can neither be matched against another
    nor be called distinct from it.
  digests given two different lengths   1
    An accounting defect: the same bytes cannot be two lengths. The
    larger is what the figures above use, which makes r_file the lower
    of the two claims it could have made.
    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb: 500 bytes and 600 bytes

2. per-job output size, by decile

  Deciles and the maximum, never a mean: this distribution is skewed far
  enough that a mean over it misreads a job by more than an order of
  magnitude. Decile k is the smallest size at least ceil(k * n / 10) of
  the jobs are at or under -- a size some job really has, not an
  interpolation -- and the population is every record read, including the
  ones that name no file.

  population                 10 records
    of those, naming no file 6
  decile 1                   0 bytes
  decile 2                   0 bytes
  decile 3                   0 bytes
  decile 4                   0 bytes
  decile 5                   0 bytes
  decile 6                   0 bytes
  decile 7                   0 bytes
  decile 8                   2000500 bytes (1.91 MiB)
  decile 9                   2000600 bytes (1.91 MiB)
  maximum                    4500777 bytes (4.29 MiB)
  over the whole population  8501877 bytes (8.11 MiB)
  the largest 1 record(s)    4500777 bytes (4.29 MiB), 52.94% of that

3. the records against the directories

  What the records say this root holds against what the directories
  measure. A manifest outliving its bytes is by design (D148): only the
  bytes are ever removed, and such a record is counted apart below rather
  than against the disk.

  records still claiming their bytes  3 job(s), 6501377 bytes (6.20 MiB)
  records whose bytes were removed    1 job(s), 2000500 bytes (1.91 MiB)
  every files/ directory measures     18 bytes
  THE RECORDS CLAIM 6501359 bytes (6.20 MiB) MORE THAN THE DIRECTORIES MEASURE
  claiming bytes, files/ measures 0   1 job(s)
    jobs/j0000000010
  claiming more than files/ holds     1 job(s)
    Bytes were claimed and some of them are there: neither extreme the
    lists either side of this one name. Largest shortfall first, so the
    names that fit are the ones that matter, and both figures are on
    each line, so the shortfall can be checked rather than trusted.
    jobs/j0000000008: claims 2000600 bytes (1.91 MiB), files/ measures 7 bytes, short by 2000593 bytes (1.91 MiB)
  bytes removed, files/ still holds   1 job(s)
    jobs/j0000000009: 11 bytes
  files.part/ unfinished collection   1 job(s), 3 bytes
    A collection that started and did not finish. This report cannot
    tell whether anything is still coming: a runner may be streaming
    into it right now, or it may be what one that died left behind.
    jobs/j0000000006: 3 bytes
  files.gone/ a sweep left behind     1 job(s), 5 bytes
";
