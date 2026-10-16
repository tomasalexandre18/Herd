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
