//! The two carriages a job's output comes home by, and the property that
//! they are the same journey (D148).
//!
//! A collection at or under [`herd_proto::INLINE_OUTPUT_MAX`] rides the
//! verdict frame, exactly as it did before streaming existed. Above it the
//! verdict carries the **manifest** — every path, its length, its sha256 —
//! and the bytes follow file by file. The runner measures and chooses,
//! because nobody knows in advance whether a run leaves 1 MiB or 400 MiB, so
//! a requester that had to declare which it wanted would have to know.
//!
//! **So the thing to test is not that streaming works. It is that a
//! requester cannot tell.** Everything observable has to match: the exit
//! code, the bytes on the disk, the line on stderr saying what was written,
//! and stdout carrying no word of either (E-02). The one place the difference
//! is allowed to show is `job.json`'s `streamed` flag, which an operator
//! reads and a requester does not.
//!
//! These run the real binaries against the real hub, the real runner and a
//! real job writing a real file: the figures here are the shipped ones, not a
//! test's own, so a change that put the threshold somewhere else fails them.

use std::process::Output;

mod fixture;

use fixture::{Fixture, Harness, ASSEMBLE, BRANCH};

/// Where the fixture leaves its result, shaped like the first consumer's.
const RESULTS: &str = "files/results.json";

/// A file big enough that a real collection really does take the streamed
/// carriage.
///
/// **Exactly the threshold, which is over it**, and the arithmetic is worth
/// naming because it is the mechanism: the carriage is chosen on a
/// collection's *weight*, not on its bytes, and weight is bytes plus the path
/// plus [`herd_proto::PER_FILE_OVERHEAD`]. So a file of exactly
/// [`herd_proto::INLINE_OUTPUT_MAX`] weighs more than the threshold and
/// streams.
///
/// Asking for exactly a power of two also keeps the test cheap: the fixture
/// writes by doubling from sixteen bytes, so any figure above 32 MiB would
/// overshoot to 64 and double what this test writes and transfers.
fn over_the_threshold() -> u64 {
    herd_proto::INLINE_OUTPUT_MAX
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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// What one run of the fixture, leaving `bytes` bytes, is observable as.
struct Journey {
    code: i32,
    out: String,
    err: String,
    /// The bytes that reached the requester's own directory.
    landed: Vec<u8>,
    /// Whether the record says the collection streamed.
    streamed: bool,
}

/// Run the fixture leaving a file of about `bytes`, fetch it, and report
/// everything a requester could observe about the journey.
async fn travel(fixture: &Fixture, label: &str, bytes: u64) -> Journey {
    let harness = Harness::new().await;
    let into = herd_runner::testing::TempDir::new(label);
    let size = bytes.to_string();
    let output = harness
        .herdctl_in(
            fixture,
            &[
                "run",
                "--command",
                ASSEMBLE,
                "--commit",
                BRANCH,
                "--outputs-to",
                &into.path().to_string_lossy(),
                "--",
                "--leave",
                RESULTS,
                "--leave-bytes",
                &size,
                "--verdict",
                "0",
            ],
        )
        .await;
    let landed = std::fs::read(into.path().join(RESULTS)).unwrap_or_else(|why| {
        panic!(
            "{} was not written: {why}\nstderr: {}",
            into.path().join(RESULTS).display(),
            stderr(&output)
        )
    });
    let record = harness.only_record();
    let streamed = record["outputs"]["streamed"].as_bool().unwrap_or(false);
    Journey {
        code: status_code(&output),
        out: stdout(&output),
        err: stderr(&output),
        landed,
        streamed,
    }
}

/// **The property.** The same job, once small enough to ride the verdict and
/// once too big, is indistinguishable to the requester except on the record.
///
/// If this fails, the carriage has become visible — which is the one thing
/// D148 promised it would not be, because a requester declares nothing about
/// size and so must be told nothing about it either.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_requester_cannot_tell_which_carriage_its_output_came_by() {
    let Some(fixture) = Fixture::new("a_requester_cannot_tell_which_carriage_its_output_came_by")
    else {
        return;
    };

    let small = travel(&fixture, "carriage-inline", 4096).await;
    let large = travel(&fixture, "carriage-streamed", over_the_threshold()).await;

    // The carriages really were different ones. Asserted first, because
    // everything below it would pass trivially if both had gone inline.
    assert!(
        !small.streamed,
        "4 KiB must ride the verdict frame: {}",
        small.err
    );
    assert!(
        large.streamed,
        "{} must stream: {}",
        over_the_threshold(),
        large.err
    );

    // And now every observable thing matches.
    assert_eq!(
        small.code, large.code,
        "the exit code is the job's, and the job is the same job"
    );
    assert_eq!(small.code, 0, "both passed: {}", small.err);

    // The bytes arrived whole, which for the streamed one means every chunk
    // landed in order and the digest held.
    assert_eq!(
        small.landed.len(),
        4096,
        "the small file is the file the job wrote"
    );
    // The fixture doubles from sixteen bytes, so it overshoots to the next
    // power of two above what was asked for. What is pinned here is only
    // that the file really is over the threshold; that it is *exactly* what
    // the job wrote is what the digest check in the next test establishes.
    assert!(
        large.landed.len() as u64 >= over_the_threshold(),
        "the large file really is over the threshold: {} bytes",
        large.landed.len()
    );
    // Same generator, so the first bytes are the same bytes: a file that
    // arrived with its chunks reordered or spliced would not match here.
    assert_eq!(
        &large.landed[..4096],
        &small.landed[..],
        "the streamed file's bytes are the bytes the job wrote, in order"
    );

    // One line per file written, on **stderr**, in the same words. The size
    // differs and the sentence does not.
    for (what, journey) in [("inline", &small), ("streamed", &large)] {
        assert!(
            journey.err.contains(&format!(
                "wrote {}",
                std::path::Path::new(RESULTS).display()
            )) || journey.err.contains("wrote "),
            "the {what} journey must say what it wrote: {}",
            journey.err
        );
        assert!(
            journey
                .err
                .contains(&format!("({} bytes)", journey.landed.len())),
            "and how many bytes, in the {what} journey: {}",
            journey.err
        );
        // E-02: **Herd** says nothing about output files on stdout, either
        // way. The job's own bytes are stdout's, and this fixture's tool
        // echoes its own arguments there — `results.json` among them — which
        // is the job talking and not Herd. So what is asserted is that no
        // line of Herd's is in that stream at all: every one of them carries
        // this prefix and belongs on stderr.
        assert!(
            !journey.out.contains("herdctl:"),
            "the {what} journey must keep Herd's own lines out of E-02's stream: {}",
            journey.out
        );
        assert!(
            !journey.out.contains("wrote "),
            "the {what} journey must keep its file lines on stderr: {}",
            journey.out
        );
    }
}

/// A streamed collection reaches a requester that was never connected while
/// the job ran — which is the first consumer's actual flow.
///
/// `herdctl wait <id> --outputs-to <dir>` is the case D137 built the whole
/// mechanism around, and it is the one that would have been easiest to leave
/// broken: the live follower has the verdict frame in hand, while a late
/// attach has nothing but the record. If the manifest or the fetch depended
/// on having followed the job, this is where it shows.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_streamed_collection_reaches_a_requester_that_attaches_afterwards() {
    let Some(fixture) =
        Fixture::new("a_streamed_collection_reaches_a_requester_that_attaches_afterwards")
    else {
        return;
    };
    let harness = Harness::new().await;
    let size = over_the_threshold().to_string();

    // Submitted and followed to its end, with no `--outputs-to`: so nothing
    // is fetched, and the requester pays nothing for bytes it did not ask
    // for.
    let ran = harness
        .herdctl_in(
            &fixture,
            &[
                "run",
                "--command",
                ASSEMBLE,
                "--commit",
                BRANCH,
                "--",
                "--leave",
                RESULTS,
                "--leave-bytes",
                &size,
                "--verdict",
                "0",
            ],
        )
        .await;
    assert_eq!(status_code(&ran), 0, "{}", stderr(&ran));
    // Told the files exist, so a requester that forgot the option can find
    // them — and told it on stderr.
    assert!(
        stderr(&ran).contains("--outputs-to"),
        "a collection nobody asked for is still announced: {}",
        stderr(&ran)
    );

    let record = harness.only_record();
    assert!(
        record["outputs"]["streamed"].as_bool().unwrap_or(false),
        "this collection streamed: {record}"
    );
    let job_id = record["id"].as_str().expect("the record carries its id");

    // And now the late attach, on a job that is long over.
    let into = herd_runner::testing::TempDir::new("carriage-late");
    let waited = harness
        .herdctl_in(
            &fixture,
            &[
                "wait",
                job_id,
                "--outputs-to",
                &into.path().to_string_lossy(),
            ],
        )
        .await;
    assert_eq!(status_code(&waited), 0, "{}", stderr(&waited));
    let landed = std::fs::read(into.path().join(RESULTS)).unwrap_or_else(|why| {
        panic!(
            "a late attach must get the files too: {why}\nstderr: {}",
            stderr(&waited)
        )
    });
    assert!(
        landed.len() as u64 >= over_the_threshold(),
        "and all of them: {} bytes",
        landed.len()
    );
    // The digest the record carries is the digest of what arrived. This is
    // the end-to-end check that the manifest and the bytes agree across
    // three processes and two carriages.
    let announced = record["outputs"]["files"]
        .as_array()
        .expect("the record lists its files")
        .iter()
        .find(|file| file["path"] == RESULTS)
        .expect("the results file is on the record")["sha256"]
        .as_str()
        .expect("with its digest")
        .to_owned();
    assert_eq!(
        herd_proto::sha256_hex(&landed),
        announced,
        "what reached the requester is what the verdict promised"
    );
}

/// **The sweeper, end to end: a collection written, evicted, and then asked
/// for** (D148, D152).
///
/// Two real jobs through a real runner, a hub whose output budget only has
/// room for one of them, and then a late `herdctl wait --outputs-to` on the
/// one that went. Everything about eviction that reaches a requester is
/// here, and most of it is about what is **kept**:
///
/// * the second job's files are untouched, because a sweeper that freed
///   more than it had to would be a worse defect than one that freed
///   nothing at all;
/// * the first job's **manifest** is whole — path, length and digest —
///   because that is what makes an absent collection answerable instead of
///   empty;
/// * the answer names the figures and says `removed`, so the reader's next
///   question is not "did Herd lose it";
/// * the fetch writes **no file**, and the directory it was pointed at is
///   empty rather than short;
/// * and the job's exit code is still its own (D20): a side channel never
///   becomes a verdict, not even when the bytes are gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_collection_the_sweeper_took_is_answered_with_its_figures() {
    let Some(fixture) = Fixture::new("a_collection_the_sweeper_took_is_answered_with_its_figures")
    else {
        return;
    };
    // Room for one collection of this size and not two. Both jobs pass and
    // neither is fetched before the sweep, so what decides between them is
    // age — the tie-break doing its job, which is the only key two
    // identical jobs can differ on.
    const LEAVES: u64 = 4096;
    let harness = Harness::with_output_budget(LEAVES + 1024).await;

    for tag in ["first", "second"] {
        let size = LEAVES.to_string();
        let output = harness
            .herdctl_in(
                &fixture,
                &[
                    "run",
                    "--command",
                    ASSEMBLE,
                    "--commit",
                    BRANCH,
                    "--",
                    "--leave",
                    RESULTS,
                    "--leave-bytes",
                    &size,
                    "--verdict",
                    "0",
                    // Two different argument lines, so these are two jobs
                    // and not one submission joining another (E-05): the
                    // fixture's `dedup_key` is keyed on the arguments.
                    "--tag",
                    tag,
                ],
            )
            .await;
        assert_eq!(status_code(&output), 0, "{}", stderr(&output));
    }
    let records = harness.records();
    assert_eq!(records.len(), 2, "two jobs ran: {records:?}");
    let first = records[0]["id"].as_str().expect("an id").to_owned();
    let second = records[1]["id"].as_str().expect("an id").to_owned();

    // **What is kept.** The second job's bytes are where it left them.
    let survivor = harness
        .kept(&second, RESULTS)
        .expect("the newer collection is untouched");
    assert!(survivor.len() as u64 >= LEAVES, "{} bytes", survivor.len());

    // **What went**, and the record that still describes it.
    assert!(
        harness.kept(&first, RESULTS).is_none(),
        "the older collection's bytes made room for the newer one"
    );
    let record = harness.records().remove(0);
    let manifest = record["outputs"]["files"]
        .as_array()
        .expect("the manifest is still there");
    assert_eq!(
        manifest.len(),
        1,
        "the manifest outlives the bytes: {record}"
    );
    assert!(
        manifest[0]["bytes"].as_u64().unwrap_or(0) >= LEAVES
            && !manifest[0]["sha256"].as_str().unwrap_or("").is_empty(),
        "with its size and its digest: {record}"
    );
    let why = record["outputs"]["removed"]["why"]
        .as_str()
        .expect("and the record says what became of them");
    assert!(
        why.contains("removed") && why.contains("output budget"),
        "naming the figures: {why}"
    );

    // **And now the question a consumer actually asks**, later, holding the
    // job id and nothing else.
    let into = herd_runner::testing::TempDir::new("carriage-swept");
    let waited = harness
        .herdctl_in(
            &fixture,
            &[
                "wait",
                &first,
                "--outputs-to",
                &into.path().to_string_lossy(),
            ],
        )
        .await;
    let said = stderr(&waited);
    assert_eq!(
        status_code(&waited),
        0,
        "the job passed and still passed: a side channel never becomes a verdict (D20). \
         {said}"
    );
    assert!(
        said.contains(RESULTS) && said.contains("removed") && said.contains("output budget"),
        "the answer names the file and why it is gone, never an empty directory: {said}"
    );
    assert!(
        !into.path().join(RESULTS).exists(),
        "and nothing was invented to fill the gap"
    );
    // Never "still coming": a collection the sweeper took is the one
    // absence that is final, and a requester told to wait for it would wait
    // out the stall bound and then be told something worse (D150).
    assert!(
        !said.contains("stopped sending"),
        "an evicted collection is not a transfer that stalled: {said}"
    );
}

/// **The outage of 2026-10-19: a second collection on one link** (D158).
///
/// Every test above runs exactly one streamed collection and then throws its
/// hub and its runner away, which is why none of them could see this. The
/// runner's send window is per **connection**, and the pump used to spend a
/// permit on the acquisition that *finds the collection finished* and never
/// give it back — so the window shrank by one for every collection that
/// succeeded, and the one that arrived with the window at zero waited for
/// room for ever. On the first consumer that was the ninth collection of the
/// evening: `pack result: ok`, a complete manifest on the record, an empty
/// `files/` on the hub, and no line anywhere saying why.
///
/// So the thing under test is **two in a row on one link**, with the window
/// set to one so that two is enough (`Harness::with_files_in_flight`). The
/// second run is the assertion; the first is the one that has to not poison
/// it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_streamed_collection_on_one_link_comes_home_too() {
    let Some(fixture) = Fixture::new("a_second_streamed_collection_on_one_link_comes_home_too")
    else {
        return;
    };
    // One hub and one runner for both runs — the whole point. A window of
    // one makes the second collection the one that finds the window empty,
    // where the shipped figure of eight would make it the ninth.
    let harness = Harness::with_files_in_flight(1).await;
    let size = over_the_threshold().to_string();

    for round in 1..=2 {
        // A path of its own per round, so the two submissions are two jobs
        // and not one deduplicated onto the other (D156).
        let leaves = format!("files/results-{round}.json");
        let into = herd_runner::testing::TempDir::new(&format!("second-collection-{round}"));
        let output = harness
            .herdctl_in(
                &fixture,
                &[
                    "run",
                    "--command",
                    ASSEMBLE,
                    "--commit",
                    BRANCH,
                    "--outputs-to",
                    &into.path().to_string_lossy(),
                    "--",
                    "--leave",
                    &leaves,
                    "--leave-bytes",
                    &size,
                    "--verdict",
                    "0",
                ],
            )
            .await;
        let said = stderr(&output);
        assert_eq!(
            status_code(&output),
            0,
            "collection {round} of 2 on this link: {said}"
        );
        let landed = into.path().join(&leaves);
        let bytes = std::fs::metadata(&landed).map(|about| about.len()).ok();
        assert_eq!(
            bytes,
            Some(over_the_threshold()),
            "collection {round} of 2 on this link did not come home to {}. A collection \
             that works once and never again is a window that is not given back, not a \
             collection that is too big: the first one here succeeded.\n{said}",
            landed.display()
        );
        assert!(
            !said.contains("stopped sending"),
            "collection {round} of 2 stalled rather than arriving: {said}"
        );
    }

    // Both records really took the streamed carriage, so this test is about
    // the thing it says it is about and not about two inline collections.
    let records = harness.records();
    assert_eq!(records.len(), 2, "two jobs ran: {records:#?}");
    for record in &records {
        assert_eq!(
            record["outputs"]["streamed"].as_bool(),
            Some(true),
            "both collections must have streamed, or the window was never used: {record:#?}"
        );
    }
}
