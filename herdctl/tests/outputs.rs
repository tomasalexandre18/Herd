//! A job's declared outputs, end to end: the real `herdctl` binary, a real
//! hub, a real runner, a real job writing `files/results.json` under `{out}`,
//! and the requester getting those exact bytes back (D131).
//!
//! This is the whole of what was missing. Herd returned an exit code and a
//! log; the first consumer's result contract is a `results.json` and the
//! files beside it, and the workspace holding them was reclaimed when the job
//! ended. What is asserted here is the end of that:
//!
//! * a job that **passes** leaves `files/results.json` and the requester gets
//!   it, byte for byte, with the directory kept;
//! * a job that **exits non-zero** does too — the case the consumer needs
//!   most, and the one a success condition would have lost;
//! * a requester that attaches **after** the job is over gets the same bytes,
//!   out of the record, through `herdctl wait`;
//! * **E-02's stream is unchanged**: stdout is the `ran on:` line, the job's
//!   own bytes and the verdict, with no output file and no word about one in
//!   it;
//! * over the runner's cap, **no file comes back** and the refusal names both
//!   figures — never a truncated results file, which would read like a whole
//!   one.
//!
//! The fixture is the one M10's acceptance uses, with one addition: its tool
//! takes `--leave <path>` and writes that path under `{out}`, which is how a
//! consumer whose result is a file is stood in for without naming one (E-11).

use std::process::Output;

mod fixture;

use fixture::{left_bytes, Fixture, Harness, ASSEMBLE, BRANCH, RUNNER};

/// Where the fixture's commands leave their result, shaped like the first
/// consumer's: a file under a directory, so that "the path is preserved and
/// not flattened" is what is being asserted and not a happy accident of a
/// file at the top level.
const RESULTS: &str = "files/results.json";

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

/// A throwaway directory for `--outputs-to`, removed with the test.
fn a_directory(label: &str) -> herd_runner::testing::TempDir {
    herd_runner::testing::TempDir::new(label)
}

/// The id of the one job a harness ran.
fn the_job_id(harness: &Harness) -> String {
    harness.only_record()["id"]
        .as_str()
        .expect("the record carries its id")
        .to_owned()
}

/// A declared command run through the real binary, leaving `RESULTS` under
/// `{out}` and wearing the verdict it was told to.
async fn run_leaving(harness: &Harness, fixture: &Fixture, verdict: i32, extra: &[&str]) -> Output {
    let code = verdict.to_string();
    let mut arguments: Vec<&str> = vec!["run", "--command", ASSEMBLE, "--commit", BRANCH];
    arguments.extend_from_slice(extra);
    arguments.extend_from_slice(&["--", "--leave", RESULTS, "--verdict", &code]);
    harness.herdctl_in(fixture, &arguments).await
}

/// The case the consumer's whole contract rests on, for a job that passed and
/// for a job that failed — **both**, in one test, because the difference
/// between them is exactly what a success condition on the collection would
/// have got wrong, and a job that fails is the one whose results you most
/// want (D108's reasoning, applied to output rather than to teardown).
#[tokio::test]
async fn a_job_s_results_file_comes_back_whether_the_job_passed_or_failed() {
    let Some(fixture) =
        Fixture::new("a_job_s_results_file_comes_back_whether_the_job_passed_or_failed")
    else {
        return;
    };
    for verdict in [0, 1] {
        let harness = Harness::new().await;
        let into = a_directory("outputs-e2e");
        let output = run_leaving(
            &harness,
            &fixture,
            verdict,
            &["--outputs-to", &into.path().to_string_lossy()],
        )
        .await;

        // The verdict is untouched: collecting a file is not a verdict about
        // anything (E-02, D20).
        assert_eq!(
            status_code(&output),
            verdict,
            "the tool's own code is worn unchanged: {}",
            stderr(&output)
        );

        // The bytes, on the requester's disk, at the path the job used.
        let landed = into.path().join(RESULTS);
        let got = std::fs::read(&landed).unwrap_or_else(|why| {
            panic!(
                "{} was not written: {why}\nstderr: {}",
                landed.display(),
                stderr(&output)
            )
        });
        assert_eq!(
            String::from_utf8_lossy(&got),
            left_bytes("assemble", verdict),
            "the requester gets the bytes the job wrote, byte for byte"
        );

        // And the path is **preserved**: a flattened `results.json` at the
        // top would be a different contract.
        assert!(
            !into.path().join("results.json").exists(),
            "the path must not be flattened"
        );

        // The hub's record holds them too, which is what makes the wait below
        // possible at all (E-15, D8).
        let job_id = the_job_id(&harness);
        assert_eq!(
            harness.kept_paths(&job_id),
            [RESULTS],
            "the job's record holds exactly the file the job left"
        );
        assert_eq!(
            harness
                .kept(&job_id, RESULTS)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
            Some(left_bytes("assemble", verdict)),
            "and holds its bytes"
        );
        // The summary says the same thing to a reader of `job.json` alone.
        let record = harness.only_record();
        assert_eq!(
            record["outputs"]["files"][0]["path"].as_str(),
            Some(RESULTS),
            "the record names what it keeps: {record:?}"
        );
        assert_eq!(
            record["outputs"]["files"][0]["bytes"].as_u64(),
            Some(left_bytes("assemble", verdict).len() as u64),
            "with its size: {record:?}"
        );
        // One line per file, on stderr and never on stdout.
        assert!(
            stderr(&output).contains("wrote") && stderr(&output).contains(RESULTS),
            "the files written are reported on stderr: {}",
            stderr(&output)
        );
    }
}

/// E-02, unchanged. stdout is the framing line, the job's own bytes, then the
/// verdict — and a job that left a file changes none of it.
///
/// The thing this forbids is subtle and would be easy to do by accident: a
/// results file, or a sentence about one, printed into the stream the
/// consumer's own tooling reads. Their reader is anchored on the first line
/// and on the last; anything Herd adds in between is read as their tool's
/// output.
#[tokio::test]
async fn a_job_that_leaves_a_file_leaves_e_02_s_stream_alone() {
    let Some(fixture) = Fixture::new("a_job_that_leaves_a_file_leaves_e_02_s_stream_alone") else {
        return;
    };
    let harness = Harness::new().await;
    let into = a_directory("outputs-e2e-stream");
    let output = run_leaving(
        &harness,
        &fixture,
        0,
        &["--outputs-to", &into.path().to_string_lossy()],
    )
    .await;
    assert_eq!(status_code(&output), 0, "{}", stderr(&output));

    // The first line is E-02's, read through the one definition of the format.
    let (line, after) = herd_proto::unframe(&output.stdout).unwrap_or_else(|| {
        panic!(
            "stdout does not begin with E-02's `ran on: <runner> (<why>)`: {:?}\nstderr: {}",
            stdout(&output),
            stderr(&output)
        )
    });
    let (runner, _) = herd_proto::ran_on_parts(&line)
        .unwrap_or_else(|| panic!("E-02's line does not have E-02's shape: {line:?}"));
    assert_eq!(runner, RUNNER);
    let after = String::from_utf8_lossy(after).into_owned();

    // The last line is the tool's own verdict, with nothing of Herd's after
    // it.
    assert_eq!(
        after.lines().last(),
        Some("grove assemble: verdict 0"),
        "the output must end with the tool's verdict: {after:?}"
    );

    // And nothing anywhere in the stream is about an output file: not the
    // bytes, not the path, not a word Herd invented.
    let results = left_bytes("assemble", 0);
    assert!(
        !after.contains(results.trim()),
        "the results file's bytes must not be on stdout: {after:?}"
    );
    for forbidden in ["wrote ", "output cap", "--outputs-to"] {
        assert!(
            !after.contains(forbidden),
            "stdout must say nothing about output files ({forbidden:?}): {after:?}"
        );
    }
    // The job's own `left` line is the job's: the tool printed it, so it
    // belongs on stdout and this is the control showing the assertion above
    // is not passing by the stream being empty.
    assert!(
        after.contains("grove assemble: left files/results.json"),
        "the job's own output is still all there: {after:?}"
    );
}

/// `herdctl wait`, attaching to a job that is already over, gets the files.
///
/// This is the whole reason the mechanism is a field on the verdict frame and
/// not a stream: a requester that was never connected while the job ran —
/// a merger reading a detached submission, a script an hour later, another
/// machine — holds nothing but the id, and the record is what still knows.
#[tokio::test]
async fn a_requester_attaching_after_the_job_is_over_still_gets_its_files() {
    let Some(fixture) =
        Fixture::new("a_requester_attaching_after_the_job_is_over_still_gets_its_files")
    else {
        return;
    };
    let harness = Harness::new().await;

    // Submitted and detached: `submit` prints the id and leaves, so nothing
    // is following this job while it runs (E-03, E-42).
    let submitted = harness
        .herdctl_in(
            &fixture,
            &[
                "submit",
                "--command",
                ASSEMBLE,
                "--commit",
                BRANCH,
                "--",
                "--leave",
                RESULTS,
                "--verdict",
                "1",
            ],
        )
        .await;
    assert_eq!(status_code(&submitted), 0, "{}", stderr(&submitted));
    let job_id = stdout(&submitted).trim().to_owned();
    assert!(job_id.starts_with('j'), "an id on stdout: {job_id:?}");

    // Waited for afterwards, by id alone. The wait is answered from the hub's
    // record however long it has been.
    let into = a_directory("outputs-e2e-wait");
    let waited = harness
        .herdctl_in(
            &fixture,
            &[
                "wait",
                &job_id,
                "--outputs-to",
                &into.path().to_string_lossy(),
            ],
        )
        .await;
    assert_eq!(
        status_code(&waited),
        1,
        "the verdict comes back too: {}",
        stderr(&waited)
    );
    let landed = into.path().join(RESULTS);
    assert_eq!(
        std::fs::read_to_string(&landed).unwrap_or_else(|why| panic!(
            "{} was not written by a wait: {why}\nstderr: {}",
            landed.display(),
            stderr(&waited)
        )),
        left_bytes("assemble", 1),
        "a requester that was never connected while the job ran gets the same bytes"
    );
}

/// Over the cap: no file, both figures, and the verdict untouched.
///
/// The cap is lowered to a few hundred bytes so that a real job really does
/// go over a real cap — nothing here is simulated. What is being proved is
/// the shape of the answer: the results file is **absent**, not short.
#[tokio::test]
async fn an_output_over_the_cap_is_refused_with_figures_and_never_truncated() {
    let Some(fixture) =
        Fixture::new("an_output_over_the_cap_is_refused_with_figures_and_never_truncated")
    else {
        return;
    };
    // Smaller than the file below and larger than nothing, so the refusal is
    // about the cap and not about a cap of zero.
    let cap = 512u64;
    let left = 8192u64;
    let harness = Harness::with_output_cap(cap).await;
    let into = a_directory("outputs-e2e-cap");
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
                RESULTS,
                "--leave-bytes",
                &left.to_string(),
                "--verdict",
                "0",
            ],
        )
        .await;

    // The job passed, and it still passed: a cap on a side channel is not a
    // verdict about the tests (D20).
    assert_eq!(
        status_code(&output),
        0,
        "the verdict is the tool's own: {}",
        stderr(&output)
    );
    // Nothing on the requester's disk. Not a short file — none.
    assert!(
        !into.path().join(RESULTS).exists(),
        "over the cap there is no file at all, never a truncated one"
    );
    // The refusal, with the cap and the real size, on stderr.
    let said = stderr(&output);
    assert!(
        said.contains("this runner's output cap is 512 bytes"),
        "the refusal must name the cap: {said}"
    );
    assert!(
        said.contains("Nothing was truncated"),
        "and say that nothing was cut down: {said}"
    );
    // It used to have to name M11 as somewhere else to go. M11 **is** this
    // now — the streamed carriage above the inline threshold (D148) — so what
    // the sentence names instead is the flag that would let the job hand its
    // output back and the figure that bounds that flag.
    assert!(
        said.contains("--job-output-cap") && said.contains("disk budget"),
        "and what to do about it, now that there is no other milestone to point at: {said}"
    );
    // And the record says it, for whoever reads the job afterwards.
    let job_id = the_job_id(&harness);
    assert!(
        harness.kept_paths(&job_id).is_empty(),
        "the hub kept no file either: {:?}",
        harness.kept_paths(&job_id)
    );
    let record = harness.only_record();
    let refused = record["outputs"]["refused"]
        .as_str()
        .unwrap_or_else(|| panic!("the record must carry the refusal: {record:?}"))
        .to_owned();
    assert_eq!(
        refused,
        said_in(&said),
        "the sentence the requester read and the one on the record are the same sentence"
    );
    assert!(
        refused.contains("1 file(s)"),
        "it must say how many files: {refused}"
    );
    // The measurement, and not only the limit: E-04's style is both figures,
    // so a reader can see how far over it is. Asserted as a number rather
    // than as a literal, because the figure is the file plus its path plus
    // the per-file charge and a test that pinned the string would pin the
    // charge.
    let measured = measured_bytes(&refused);
    assert!(
        measured > left,
        "the refusal must name the real size, which is at least the {left} bytes the job \
         wrote: {refused}"
    );
}

/// The refusal as the requester read it, lifted out of `herdctl`'s stderr.
fn said_in(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| line.contains("is not in its record"))
        .unwrap_or_else(|| panic!("no refusal on stderr: {stderr}"))
        .trim_start_matches("herdctl: ")
        .to_owned()
}

/// The figure the refusal names for what did not fit.
fn measured_bytes(refusal: &str) -> u64 {
    let (_, after) = refusal
        .split_once("record: ")
        .unwrap_or_else(|| panic!("the refusal has lost its shape: {refusal}"));
    let (figure, _) = after
        .split_once(' ')
        .unwrap_or_else(|| panic!("the refusal has lost its shape: {refusal}"));
    figure
        .parse()
        .unwrap_or_else(|why| panic!("{figure:?} is not a number of bytes ({why}): {refusal}"))
}

/// A job that leaves nothing changes nothing, and the record of it is silent.
///
/// The control for every assertion above: the field is absent from `job.json`
/// entirely, which is what makes this additive to the frozen record format
/// rather than a new shape every old record lacks.
#[tokio::test]
async fn a_job_that_leaves_nothing_says_nothing_about_outputs() {
    let Some(fixture) = Fixture::new("a_job_that_leaves_nothing_says_nothing_about_outputs") else {
        return;
    };
    let harness = Harness::new().await;
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
                "--verdict",
                "0",
            ],
        )
        .await;
    assert_eq!(status_code(&output), 0, "{}", stderr(&output));
    let job_id = the_job_id(&harness);
    assert!(harness.kept_paths(&job_id).is_empty());
    let record = harness.only_record();
    assert!(
        record.get("outputs").is_none(),
        "a job that kept nothing has no outputs field at all: {record:?}"
    );
    assert!(
        !stderr(&output).contains("output file"),
        "and nothing is said about files that do not exist: {}",
        stderr(&output)
    );
}
