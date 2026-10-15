//! A declared command, end to end: the real `herdctl` binary standing in the
//! fixture consumer's own checkout, a real hub, a real runner, and the four
//! executor steps really running (M10).
//!
//! What the unit tests beside this one cannot show is the whole of E-02 and
//! E-11 at once, because the thing being asserted is what a **caller** sees:
//! the status its shell read, and the bytes on its stdout. So this runs the
//! binary, in the fixture's work tree, with no argument naming a machine
//! (invariant 6) and none naming a command line either — only a command the
//! checkout declares and a ref.
//!
//! * the output begins with exactly one `ran on: <runner> (<why>)`, and the
//!   consumer tool's own verdict and exit code follow it unchanged: 0, 1 and
//!   75 all the way through to the shell (E-02);
//! * the ref resolves to the hash the fixture's **own** git holds, and that
//!   hash is what the job's record carries and what the workspace is at
//!   (E-01);
//! * a dirty tree and an unpushed commit are each refused, naming the file
//!   and the hash, by the binary and before any job exists (E-08);
//! * and a **third** command, added to the fixture's TOML and to nothing
//!   else, runs through the very same binary (E-11).
//!
//! No consumer command is named here or in the fixture: every name is
//! invented (E-11).

use std::process::Output;
use std::sync::Arc;
use std::time::{Duration, Instant};

use herd_hub::{Hub, JobDirStore, JobStore};
use herd_runner::testing::TempDir as DataRootDir;
use tempfile::TempDir;
use tokio::process::Command;
use tokio::task::JoinHandle;

mod fixture;

use fixture::{Fixture, ASSEMBLE, BRANCH, SKETCH, TAG, TRACKED};

/// The runner this harness lends, named so the framing line can be asserted
/// against a name a test chose.
const RUNNER: &str = "declared-runner";

/// What the fixture's two commands ask for, offered by the one runner here.
///
/// Shared, because nothing in this fixture contends for anything: what is
/// being asserted is the contract, not the placement, and the placement has
/// its own suite.
const WHAT_THE_FIXTURE_ASKS_FOR: &str = r#"[
    {"name":"grove-toolchain","kind":{"type":"shared"},"origins":{"declared":true}},
    {"name":"linux","kind":{"type":"shared"},"origins":{"declared":true}}
]"#;

/// A hub with one runner joined, and the means to run `herdctl` in a checkout.
struct Harness {
    hub_url: String,
    state: TempDir,
    tasks: Vec<JoinHandle<()>>,
    _data_root: DataRootDir,
}

impl Harness {
    async fn new() -> Harness {
        let state = tempfile::tempdir().expect("a temporary state directory");
        let store = JobDirStore::open(state.path()).expect("the state root opens");
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::new(store) as Arc<dyn JobStore>,
        )
        .await
        .expect("the hub binds a free port");
        let hub_url = format!("ws://{}", hub.local_addr());
        let handle = hub.handle();
        let mut tasks = vec![tokio::spawn(async move {
            let _ = hub.serve().await;
        })];

        // One data root per runner, thrown away with the harness (E-45).
        let root = DataRootDir::new("declared-e2e");
        let offer: herd_proto::CapabilityOffer = serde_json::from_str(WHAT_THE_FIXTURE_ASKS_FOR)
            .expect("the capabilities this fixture's commands ask for");
        let config = herd_runner::Config {
            hub_url: hub_url.clone(),
            name: Some(RUNNER.to_owned()),
            token: None,
            capabilities: offer,
            // A size the owner states, rather than this host's own. The
            // fixture's build declares a per-thread figure, so the hub
            // derives a memory need and admission weighs it (E-29): a runner
            // that says nothing about the floor it keeps for itself admits
            // nothing at all, and the job would wait for ever on a machine
            // that could have run it. Stated figures also make the derived
            // `-j` the same on every host, which is what keeps this test
            // about the contract and not about the machine it ran on.
            host: herd_proto::HostSize {
                memory_mib: Some(8192),
                threads: Some(4),
                memory_floor_mib: Some(1024),
                tmpfs_mib: None,
            },
            confinement: herd_runner::Confinement::with_data_root(root.path()),
            ..herd_runner::Config::default()
        };
        tasks.push(tokio::spawn(async move {
            let _ = herd_runner::run(config).await;
        }));
        // Wait on the hub's own state, never on a sleep.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.runner_connected() {
            assert!(Instant::now() < deadline, "the runner never joined the hub");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Harness {
            hub_url,
            state,
            tasks,
            _data_root: root,
        }
    }

    /// Run `herdctl` **in** a checkout, as a requester standing in one does.
    ///
    /// The working directory is the whole point: the declaration and the
    /// commit are read where the repository already is (D106), and nothing
    /// tells the binary where that is but the directory it was started in.
    async fn herdctl_in(&self, fixture: &Fixture, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_herdctl"))
            .arg("--hub")
            .arg(&self.hub_url)
            .args(arguments)
            .current_dir(fixture.root())
            // A git config of the fixture's own, so the person's identity and
            // their `~/.gitconfig` are out of the way and the refusals read
            // the same on every machine.
            .env("HOME", fixture.home())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .await
            .expect("herdctl runs")
    }

    /// The record of the one job this harness ran, read off the hub's disk as
    /// a reader outside Herd would (E-15).
    fn only_record(&self) -> serde_json::Value {
        let jobs = self.state.path().join("jobs");
        let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(&jobs)
            .expect("the hub's job directory")
            .map(|entry| entry.expect("a job directory").path())
            .collect();
        dirs.sort();
        assert_eq!(dirs.len(), 1, "exactly one job was submitted: {dirs:?}");
        let text = std::fs::read_to_string(dirs[0].join("job.json")).expect("the record reads");
        serde_json::from_str(&text).expect("the record is json")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
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

/// E-02's framing line, and the job's own output after it.
///
/// Insisted on with the shape a caller's own reader is anchored on —
/// `^ran on: (\S+) \((.*)\)$` — through [`herd_proto::unframe`], which is the
/// one definition of the format. A line that merely *contains* `ran on:` is a
/// line their reader drops on the floor, so a looser check here would pass
/// for output they could not read.
fn framed(output: &Output) -> (String, String) {
    let (line, rest) = herd_proto::unframe(&output.stdout).unwrap_or_else(|| {
        panic!(
            "stdout does not begin with E-02's `ran on: <runner> (<why>)`: {:?}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            stderr(output)
        )
    });
    let (runner, why) = herd_proto::ran_on_parts(&line)
        .unwrap_or_else(|| panic!("E-02's line does not have E-02's shape: {line:?}"));
    assert_eq!(
        runner, RUNNER,
        "the line must name the machine this harness lends: {line:?}"
    );
    assert!(
        !why.is_empty(),
        "the line must say why that machine: {line:?}"
    );
    // Q7: a public format carries its version, and it rides inside the
    // parentheses so that the expression above still matches around it.
    let version = format!("herd output format {}", herd_proto::OUTPUT_FORMAT_VERSION);
    assert!(
        why.contains(&version),
        "E-02's line must carry the output format version ({version}): {line:?}"
    );
    // Exactly one: a second framing line would mean a caller reading the
    // first line and then the output would be reading Herd's words as the
    // tool's.
    let after = String::from_utf8_lossy(rest).into_owned();
    assert!(
        !after.contains(herd_proto::RAN_ON_PREFIX),
        "the output carries more than one framing line: {after:?}"
    );
    (line, after)
}

/// The three codes the first consumer's tools mean something by, submitted
/// through a **declared** command and read off the shell: 0 is a pass, 1 is a
/// fail, 75 is absent-or-leased. None of them is rewritten, and each one
/// arrives behind the tool's own verdict line, unchanged (E-02, D16).
#[tokio::test]
async fn a_declared_command_wears_its_tool_s_verdict_and_exit_code() {
    let Some(fixture) = Fixture::new("a_declared_command_wears_its_tool_s_verdict_and_exit_code")
    else {
        return;
    };
    for code in [0, 1, 75] {
        let harness = Harness::new().await;
        let verdict = code.to_string();
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
                    &verdict,
                ],
            )
            .await;
        assert_eq!(
            status_code(&output),
            code,
            "the consumer's own exit code must reach the shell untouched: {}",
            stderr(&output)
        );
        let (line, said) = framed(&output);
        // The tool's own last word, exactly as the tool wrote it, and *last*:
        // a caller reads the verdict off the end of the output and must not
        // have to skip anything of Herd's to find it.
        assert!(
            said.ends_with(&format!("grove {ASSEMBLE}: verdict {code}\n")),
            "the output must end with the tool's own verdict: {said:?} (framed by {line:?})"
        );
        // And Herd said nothing about a failure of its own: 1 and 75 are the
        // job's verdicts, not Herd failing to run it (D16).
        assert!(
            !stderr(&output).contains(&herdctl::could_not_start_message("")),
            "exit code {code} was read as Herd failing to run the job: {}",
            stderr(&output)
        );
    }
}

/// E-01, through the binary: a submission names a **ref**, and what the job
/// records is the hash the fixture's own git holds for it — with the four
/// executor steps on the record, each with its own timestamped line (D77).
#[tokio::test]
async fn a_submission_naming_a_ref_records_the_hash_the_fixture_s_git_holds() {
    let Some(fixture) =
        Fixture::new("a_submission_naming_a_ref_records_the_hash_the_fixture_s_git_holds")
    else {
        return;
    };
    let harness = Harness::new().await;
    // The independent reading, from the fixture's own git. No hash is written
    // into this file.
    let held = fixture.rev_parse(TAG);

    // A tag, so that resolution is shown peeling one rather than reading a
    // branch — and a machine is named nowhere (invariant 6).
    let output = harness
        .herdctl_in(
            &fixture,
            &[
                "run",
                "--command",
                ASSEMBLE,
                "--commit",
                TAG,
                "--",
                "--only",
                "a b",
            ],
        )
        .await;
    assert_eq!(status_code(&output), 0, "{}", stderr(&output));
    let (_, said) = framed(&output);

    // The workspace really went to that commit, said by the fixture's own
    // `fetch` hook after a real fetch from its own bare remote.
    assert!(
        said.contains(&format!("grove: at {held}")),
        "the workspace is not at the commit the ref resolved to: {said:?}"
    );
    // The arguments reached the tool as two words, the second with its space
    // intact: `{args}` is quoted, one word per argument, and never re-read
    // (D107).
    assert!(
        said.contains("grove: workspace") && said.contains("args: --only a b"),
        "the arguments did not reach the hook: {said:?}"
    );

    let record = harness.only_record();
    assert_eq!(
        record["declaration"]["commit"].as_str(),
        Some(held.as_str()),
        "the record must carry the hash, never the ref: {record}"
    );
    assert_eq!(
        record["declaration"]["command"].as_str(),
        Some(ASSEMBLE),
        "the record must name the declared command: {record}"
    );
    // The three places that fill a placeholder, read off the record (D107).
    // `herdctl` filled the requester's two before the job was minted — a line
    // still holding `{args}` when the job is minted is a line two submissions
    // with *different* arguments would deduplicate into one — and shell-quoted
    // the arguments, one word per argument, so `a b` is one word and not two.
    // `{j}` is still here, because the figure is a property of a machine the
    // hub had not chosen yet.
    let line = record["command"][0]
        .as_str()
        .expect("the record's command line");
    assert!(
        !line.contains(herd_proto::ARGS_TOKEN) && !line.contains(herd_proto::COMMIT_TOKEN),
        "the record holds a template of the requester's own tokens: {line:?}"
    );
    assert!(
        line.contains("'--only' 'a b'"),
        "the arguments must be on the record shell-quoted, one word each: {line:?}"
    );
    assert!(
        line.contains(herd_proto::PARALLELISM_TOKEN),
        "the hub's own token must still be the hub's to fill: {line:?}"
    );
    // And the hub never saw TOML: the resolved declaration travelled with the
    // submission, hooks and environment already filled (D106).
    assert!(
        record["declaration"]["executor"]["fetch"]
            .as_str()
            .is_some_and(|fetch| fetch.contains(&held)),
        "the resolved `fetch` hook must carry the hash: {record}"
    );

    // One timestamped line per executor step, in order (D77).
    let steps: Vec<&str> = record["attempts"][0]["steps"]
        .as_array()
        .expect("the attempt's steps")
        .iter()
        .map(|step| step["step"].as_str().expect("a step name"))
        .collect();
    assert_eq!(
        steps,
        vec!["up", "fetch", "run", "down"],
        "the four steps, in order: {record}"
    );
    for step in record["attempts"][0]["steps"]
        .as_array()
        .expect("the attempt's steps")
    {
        assert!(
            step["started_unix_ms"].as_i64().is_some_and(|at| at > 0),
            "a step line without a timestamp: {step}"
        );
        assert_eq!(
            step["exit_code"].as_i64(),
            Some(0),
            "every step of a passing job came off: {step}"
        );
    }
}

/// E-08's first refusal, through the binary: a tree that does not say what
/// its commit says is refused **naming the file**, and no job is made.
#[tokio::test]
async fn the_binary_refuses_a_dirty_tree_naming_the_file() {
    let Some(fixture) = Fixture::new("the_binary_refuses_a_dirty_tree_naming_the_file") else {
        return;
    };
    let harness = Harness::new().await;
    let changed = fixture.dirty(TRACKED);

    let output = harness
        .herdctl_in(
            &fixture,
            &["run", "--command", ASSEMBLE, "--commit", BRANCH],
        )
        .await;
    // 125: Herd declining to run something, never a verdict a caller could
    // read as a failing test (D16, D20).
    assert_eq!(
        status_code(&output),
        125,
        "a refused submission is not a job that failed: {}",
        stderr(&output)
    );
    let said = stderr(&output);
    assert!(
        said.contains(&changed),
        "the refusal must name the file that changed: {said}"
    );
    assert!(
        said.contains("1 change that is in no commit"),
        "the refusal must say what is wrong: {said}"
    );
    assert!(
        said.contains("nothing gets past this refusal"),
        "the refusal must offer no way past it: {said}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "",
        "stdout belongs to the job, and no job was made"
    );
    assert!(
        !harness.state.path().join("jobs").exists()
            || std::fs::read_dir(harness.state.path().join("jobs"))
                .expect("the hub's job directory")
                .next()
                .is_none(),
        "a refused submission must leave no job on the hub"
    );
}

/// E-08's second refusal: a commit no remote-tracking ref holds is refused
/// **naming the hash**, because a runner fetches the commit from the
/// consumer's own origin and one only this checkout has is a job that could
/// never start.
#[tokio::test]
async fn the_binary_refuses_an_unpushed_commit_naming_the_hash() {
    let Some(fixture) = Fixture::new("the_binary_refuses_an_unpushed_commit_naming_the_hash")
    else {
        return;
    };
    let harness = Harness::new().await;
    let unpushed = fixture.commit_unpushed("src/only-here.txt");

    let output = harness
        .herdctl_in(
            &fixture,
            &["run", "--command", ASSEMBLE, "--commit", BRANCH],
        )
        .await;
    assert_eq!(
        status_code(&output),
        125,
        "a refused submission is not a job that failed: {}",
        stderr(&output)
    );
    let said = stderr(&output);
    assert!(
        said.contains(&unpushed),
        "the refusal must name the hash: {said}"
    );
    assert!(
        said.contains("is on no remote-tracking ref"),
        "the refusal must say what is wrong: {said}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "",
        "stdout belongs to the job, and no job was made"
    );
}

/// E-11's acceptance sentence, through the **same binary**: a third command
/// is added to the fixture's TOML and to nothing else, and it runs.
///
/// This is what the unit test beside it cannot say. That one proves the two
/// commands already declared resolve to values that compare equal before and
/// after; this one proves the addition needs no Herd at all — not a
/// recompilation, not a protocol change, not a restart of the hub or the
/// runner. The binary under test was built before this command existed, the
/// hub and the runner have been up since before it was committed, and it runs
/// anyway.
#[tokio::test]
async fn a_third_command_runs_through_the_binary_that_knew_nothing_of_it() {
    let Some(fixture) =
        Fixture::new("a_third_command_runs_through_the_binary_that_knew_nothing_of_it")
    else {
        return;
    };
    let harness = Harness::new().await;

    // Established first: with the fixture as it stands, the name is refused
    // and the refusal lists what the checkout does declare. So what follows
    // proves something.
    let before = harness
        .herdctl_in(&fixture, &["run", "--command", SKETCH, "--commit", BRANCH])
        .await;
    assert_eq!(status_code(&before), 125, "{}", stderr(&before));
    assert!(
        stderr(&before).contains(SKETCH) && stderr(&before).contains(ASSEMBLE),
        "the refusal must list the commands this checkout declares: {}",
        stderr(&before)
    );

    // One TOML table, committed and pushed. Nothing else anywhere.
    let third = fixture.add_third_command();

    let after = harness
        .herdctl_in(
            &fixture,
            &[
                "run",
                "--command",
                SKETCH,
                "--commit",
                BRANCH,
                "--",
                "--verdict",
                "1",
            ],
        )
        .await;
    assert_eq!(
        status_code(&after),
        1,
        "the third command's own verdict must reach the shell: {}",
        stderr(&after)
    );
    let (_, said) = framed(&after);
    assert!(
        said.ends_with(&format!("grove {SKETCH}: verdict 1\n")),
        "the output must end with the third command's own verdict: {said:?}"
    );
    assert!(
        said.contains(&format!("grove: at {third}")),
        "the third command ran on another commit than the one declaring it: {said:?}"
    );
}
