//! M10's runner half: a declared command run through the consumer's four
//! executor steps, on a real hub, a real runner and real shells (invariant 5,
//! D77, D108).
//!
//! ## The fixture names no consumer command
//!
//! E-11 wants no consumer command anywhere in Herd's own code or tests, so
//! every hook here is Herd's own invention, shaped like theirs: four shell
//! lines, one per step, each appending its own name to a file so the *order*
//! can be asserted rather than assumed, and the `run` hook pasting `{line}`
//! the way their `<tool> ws run {job} -- {line}` does.
//!
//! ## Skipping, and why it is loud
//!
//! Nothing here needs a cgroup, so nothing here skips for want of one. An
//! assertion that cannot be made on this host is still skipped with its
//! reason printed and appended to `$HERD_SKIP_LOG` (D27), never quietly
//! passed: `scripts/test.sh` prints that file as a summary.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use herd_hub::{Hub, JobDirStore, JobStore};
use herd_proto::{
    CommitRun, Declaration, Executor, JobId, JobOutcome, Step, StepRun, COMMIT_TOKEN, IN_TOKEN,
    LINE_TOKEN, OUT_TOKEN, RAN_TOKEN,
};
use herd_runner::testing::TempDir;
use herd_runner::Confinement;
use tokio::task::JoinHandle;

/// How long a test waits for something it has a reason to expect.
const PATIENCE: Duration = Duration::from_secs(20);

/// A hub with one runner joined, confined as the test asks, and the store the
/// hub writes to — which is where a step's line lands (D77), so a test reads
/// the steps from the record rather than from the runner's own memory.
struct Harness {
    hub_url: String,
    store: Arc<dyn JobStore>,
    data_root: TempDir,
    _state: TempDir,
    tasks: Vec<JoinHandle<()>>,
}

impl Harness {
    async fn new(label: &str, shape: impl FnOnce(&mut Confinement)) -> Self {
        let data_root = TempDir::new(label);
        let state = TempDir::new(&format!("{label}-state"));
        let mut confinement = Confinement::with_data_root(data_root.path());
        confinement.timeout = Duration::from_secs(60);
        confinement.memory_max = 512 << 20;
        confinement.log_cap = 4 << 20;
        confinement.disk_budget = 64 << 20;
        shape(&mut confinement);
        let store = Arc::new(JobDirStore::open(state.path()).expect("the state root opens"))
            as Arc<dyn JobStore>;
        // An unapproved runner claims nothing (E-48): this harness approves
        // the one runner it starts, as an operator does once.
        let token = store
            .approve_runner(&format!("m10-{label}"))
            .expect("the state root mints a token");
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::clone(&store),
        )
        .await
        .expect("the hub binds a free port");
        let hub_url = format!("ws://{}", hub.local_addr());
        let handle = hub.handle();
        let serving = tokio::spawn(async move {
            let _ = hub.serve().await;
        });
        let running = tokio::spawn({
            let config = herd_runner::Config {
                hub_url: hub_url.clone(),
                name: Some(format!("m10-{label}")),
                token: Some(token),
                confinement,
                ..herd_runner::Config::default()
            };
            async move {
                if let Err(error) = herd_runner::run(config).await {
                    eprintln!("herd-runner (test): {error:#}");
                }
            }
        });
        let deadline = Instant::now() + PATIENCE;
        while !handle.runner_connected() {
            assert!(Instant::now() < deadline, "the runner never joined the hub");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Self {
            hub_url,
            store,
            data_root,
            _state: state,
            tasks: vec![serving, running],
        }
    }

    fn requester(&self) -> herdctl::Config {
        herdctl::Config {
            hub_url: self.hub_url.clone(),
            token: None,
            name: Some("m10-requester".to_owned()),
        }
    }

    /// Submit one job and follow it to its end, keeping its whole log.
    async fn run(&self, command: Vec<String>, declared: herdctl::Declared) -> Ran {
        let mut log = String::new();
        let finished = herdctl::run_job(
            &self.requester(),
            command,
            herd_proto::LabelSet::default(),
            herd_proto::Priority::Step,
            declared,
            |_stream, data| log.push_str(&String::from_utf8_lossy(data)),
            |_note| {},
        )
        .await
        .expect("the hub answers with an outcome");
        Ran {
            steps: self.steps_of(&finished.job_id),
            recorded_commit: self.commit_of(&finished.job_id),
            told_the_requester: finished.commit.clone(),
            outcome: finished.outcome,
            log,
        }
    }

    /// Every step the record holds for the job's **first** attempt, in the
    /// order the record holds them.
    fn steps_of(&self, job_id: &JobId) -> Vec<StepRun> {
        let record = self
            .store
            .job(job_id)
            .expect("the record is readable")
            .expect("the hub minted this job");
        record
            .attempts
            .first()
            .map(|attempt| attempt.steps.clone())
            .unwrap_or_default()
    }

    /// What the record says the job's **first** attempt ran, beside the
    /// commit it declared (D176).
    fn commit_of(&self, job_id: &JobId) -> Option<CommitRun> {
        self.store
            .job(job_id)
            .expect("the record is readable")
            .expect("the hub minted this job")
            .attempts
            .first()
            .and_then(|attempt| attempt.commit.clone())
    }

    /// The one data root this runner was configured with, as a string: what
    /// every path a step is given has to be inside (E-45).
    fn root(&self) -> String {
        self.data_root.path().display().to_string()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

struct Ran {
    outcome: JobOutcome,
    steps: Vec<StepRun>,
    log: String,
    /// What the hub's record says about the revision this attempt ran.
    recorded_commit: Option<CommitRun>,
    /// And what the requester was told in the verdict frame, which has to be
    /// the same thing: the record is the one place both are read from.
    told_the_requester: Option<CommitRun>,
}

impl Ran {
    /// The step of one name, if the attempt ran it.
    fn step(&self, step: Step) -> Option<&StepRun> {
        self.steps.iter().find(|ran| ran.step == step)
    }

    /// Which steps ran, in order, by name.
    fn names(&self) -> Vec<&'static str> {
        self.steps.iter().map(|ran| ran.step.name()).collect()
    }
}

/// A declaration whose four hooks are shell lines of this test's own making.
///
/// Each hook appends its own name to one file, so the order the steps ran in
/// is a fact on a disk rather than an inference, and prints one line of its
/// own, so a log that is missing a step says which. `extra` is pasted into
/// the hook it names, which is how a test makes one step fail, sleep, or look
/// at its environment.
struct Fixture {
    dir: TempDir,
    extra: BTreeMap<&'static str, String>,
    /// The arguments the requester typed, as they travel beside the line
    /// (D107). They are already quoted into the line by then; the runner
    /// holds them so that it can fill the declared text around them without
    /// filling them.
    args: Vec<String>,
}

impl Fixture {
    fn new(label: &str) -> Self {
        Self {
            dir: TempDir::new(&format!("{label}-fixture")),
            extra: BTreeMap::new(),
            args: Vec::new(),
        }
    }

    /// The arguments this submission carries.
    fn with_args(mut self, args: &[&str]) -> Self {
        self.args = args.iter().map(|arg| (*arg).to_owned()).collect();
        self
    }

    /// Append a shell line to one hook, after the line that records the step.
    fn and_then(mut self, step: Step, said: &str) -> Self {
        self.extra.insert(step.name(), said.to_owned());
        self
    }

    /// Where the steps write their names.
    fn trace_path(&self) -> PathBuf {
        self.dir.path().join("trace")
    }

    /// The names of the steps that have run, in order.
    fn trace(&self) -> Vec<String> {
        std::fs::read_to_string(self.trace_path())
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn hook(&self, step: Step) -> String {
        let name = step.name();
        let trace = self.trace_path().display().to_string();
        let mut line =
            format!("printf '%s\\n' {name} >> '{trace}'; printf 'the {name} step ran\\n'");
        if let Some(extra) = self.extra.get(name) {
            line.push_str("; ");
            line.push_str(extra);
        }
        line
    }

    fn declaration(&self) -> Declaration {
        Declaration {
            command: "fixture".to_owned(),
            // Forty lowercase hex, as `herdctl` resolved it: the hub refuses
            // anything else (E-01).
            commit: "cb5a".repeat(10),
            args: self.args.clone(),
            executor: Executor {
                up: self.hook(Step::Up),
                fetch: self.hook(Step::Fetch),
                // `{line}` pasted into a shell line, the way their own `run`
                // hook pastes it: the step's code is then the line's code.
                run: format!("{}; {LINE_TOKEN}", self.hook(Step::Run)),
                down: self.hook(Step::Down),
                env: BTreeMap::from([
                    (
                        "FIXTURE_LEAVES".to_owned(),
                        format!("{OUT_TOKEN}/left-here"),
                    ),
                    ("FIXTURE_TAKES".to_owned(), IN_TOKEN.to_owned()),
                ]),
            },
        }
    }

    /// What a requester sends for this declaration, with the command's own
    /// `run` cap if it declares one.
    fn declared(&self, timeout_s: Option<u64>) -> herdctl::Declared {
        herdctl::Declared {
            declaration: Some(self.declaration()),
            timeout_s,
            ..herdctl::Declared::default()
        }
    }
}

/// The declared line, as one word: it is a shell line and the hub substitutes
/// `{j}` word by word, so a declared command's `command` is one element.
fn line(said: &str) -> Vec<String> {
    vec![said.to_owned()]
}

fn shell(script: &str) -> Vec<String> {
    vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()]
}

/// Where one line appears in a text, so that "before" can be asserted rather
/// than assumed.
fn at(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is not in:\n{text}"))
}

fn skipped(test: &str, why: &str) {
    herd_runner::testing::skipped(test, why);
}

/// Whether a hook on this host can make a step take time, and why not if it
/// cannot.
///
/// Four assertions below need a step that lasts — a duration to measure, a
/// cap to trip, a `run` step to cancel in the middle of — and `sleep` is the
/// only way a shell line waits without spinning a core. A host without one
/// skips them loudly rather than failing with a shell's "not found", which
/// would read as Herd's defect (D27).
fn a_sleeping_hook_or_reason() -> Result<(), String> {
    match std::process::Command::new("sleep").arg("0").status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "`sleep 0` on this host ended as {status}, so a hook cannot make a step last"
        )),
        Err(error) => Err(format!(
            "this host has no `sleep` a hook could call, so no step here can be made to \
             last: {error}"
        )),
    }
}

// ------------------------------------------------------------- the order ----

/// The four steps run in the one order they run in, nothing is skipped, and
/// every step's output is the job's output (E-14's reason for the whole
/// thing: their `up` prints how long a cache pull took).
#[tokio::test]
async fn the_four_steps_run_in_order_and_every_step_s_output_is_the_job_s_log() {
    let harness = Harness::new("in-order", |_| {}).await;
    let fixture = Fixture::new("in-order");
    let ran = harness
        .run(
            line("printf 'the declared line ran\\n'"),
            fixture.declared(None),
        )
        .await;

    assert_eq!(
        ran.outcome,
        JobOutcome::Exited { code: 0 },
        "the whole of it should have come off: {}",
        ran.log
    );
    assert_eq!(
        fixture.trace(),
        ["up", "fetch", "run", "down"],
        "the four steps, in the fixed order, each exactly once"
    );
    for step in Step::ALL {
        assert!(
            ran.log.contains(&format!("the {step} step ran")),
            "the {step} step's output is missing from the job's log:\n{}",
            ran.log
        );
    }
    assert!(
        ran.log.contains("the declared line ran"),
        "the declared line's own output is missing:\n{}",
        ran.log
    );
    // In order, in the one log the hub keeps.
    assert!(at(&ran.log, "the up step ran") < at(&ran.log, "the fetch step ran"));
    assert!(at(&ran.log, "the fetch step ran") < at(&ran.log, "the run step ran"));
    assert!(at(&ran.log, "the run step ran") < at(&ran.log, "the down step ran"));

    // One line per step on the record, each with a code (D77).
    assert_eq!(ran.names(), ["up", "fetch", "run", "down"]);
    for ran_step in &ran.steps {
        assert_eq!(
            ran_step.exit_code,
            Some(0),
            "{} should carry the code its shell exited with: {ran_step:?}",
            ran_step.step
        );
        assert_eq!(
            ran_step.ended, None,
            "a step that exited says nothing about why there is no code"
        );
        assert!(
            ran_step.started_unix_ms > 1_700_000_000_000,
            "a step's start is a moment on the runner's clock: {ran_step:?}"
        );
    }
}

/// A step's line carries how long the step took, which is the whole point of
/// timestamping them: the first consumer's `up` may pull a warm cache for
/// minutes, and that time must not read as a slow build (D77, D108).
#[tokio::test]
async fn a_step_s_line_carries_the_time_that_step_took() {
    if let Err(why) = a_sleeping_hook_or_reason() {
        skipped("a_step_s_line_carries_the_time_that_step_took", &why);
        return;
    }
    let harness = Harness::new("step-duration", |_| {}).await;
    let fixture = Fixture::new("step-duration").and_then(Step::Up, "sleep 1");
    let ran = harness.run(line("true"), fixture.declared(None)).await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 }, "{}", ran.log);

    let up = ran.step(Step::Up).expect("up ran").clone();
    assert!(
        up.duration_ms >= 900,
        "the up step slept for a second and its line says {} ms",
        up.duration_ms
    );
    let down = ran.step(Step::Down).expect("down ran").clone();
    assert!(
        down.duration_ms < up.duration_ms,
        "the takedown did not sleep and should not be timed as if it had: \
         up {} ms, down {} ms",
        up.duration_ms,
        down.duration_ms
    );
}

/// The declaration's environment reaches every step, with `{out}` filled —
/// their own `RESULTS` variable is `{out}/results.json`, and a value the
/// substitution skipped would hand their tool a path with a brace in it
/// (D107).
#[tokio::test]
async fn the_declared_environment_reaches_the_steps_with_the_runner_s_tokens_filled() {
    let harness = Harness::new("declared-env", |_| {}).await;
    let fixture = Fixture::new("declared-env")
        .and_then(Step::Up, "printf 'leaves at %s\\n' \"$FIXTURE_LEAVES\"");
    let ran = harness.run(line("true"), fixture.declared(None)).await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 }, "{}", ran.log);
    assert!(
        ran.log.contains("leaves at /") && ran.log.contains("/out/left-here"),
        "the declared environment should reach the step with {OUT_TOKEN} filled:\n{}",
        ran.log
    );
    assert!(
        !ran.log.contains(OUT_TOKEN),
        "no brace may reach a step:\n{}",
        ran.log
    );
}

/// A declared command's **own line** may name `{out}` and `{in}`, and the
/// step gets the directory rather than the three characters — while an
/// argument the requester typed that says the same three characters is text
/// and stays text (D107).
///
/// Both halves in one job on purpose: they are one property, and a fix for
/// the first that broke the second would turn a requester's own string into
/// a template.
#[tokio::test]
async fn a_declared_line_s_own_tokens_are_filled_and_a_requester_s_arguments_are_not() {
    let harness = Harness::new("declared-line-tokens", |_| {}).await;
    let typed = ["--only", OUT_TOKEN];
    let fixture = Fixture::new("declared-line-tokens").with_args(&typed);
    // The line as `herdctl` resolves it: the declared template, with the
    // arguments already quoted into its `{args}` (D107).
    let said = format!(
        "printf 'out is %s\\n' {OUT_TOKEN}/packs; printf 'args are %s %s\\n' {}",
        herd_proto::args_as_line(&typed.map(str::to_owned))
    );
    let ran = harness.run(line(&said), fixture.declared(None)).await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 }, "{}", ran.log);
    assert!(
        ran.log.contains("out is /") && ran.log.contains("/out/packs"),
        "the declared line's own {OUT_TOKEN} should have reached the hook as a path:\n{}",
        ran.log
    );
    assert!(
        ran.log.contains(&format!("args are --only {OUT_TOKEN}")),
        "an argument the requester typed is text and must arrive as typed:\n{}",
        ran.log
    );
}

/// `{in}` and `{out}` are made before `up` runs, and they are inside the one
/// data root and nowhere else (E-45, D107).
#[tokio::test]
async fn the_exchange_directories_exist_before_up_runs_and_are_inside_the_data_root() {
    let seen = TempDir::new("exchange-seen");
    let where_they_are = seen.path().join("paths");
    let harness = Harness::new("exchange", |_| {}).await;
    let fixture = Fixture::new("exchange").and_then(
        Step::Up,
        &format!(
            "printf '%s\\n%s\\n' \"$(dirname \"$FIXTURE_LEAVES\")\" \"$FIXTURE_TAKES\" >> '{}'; \
             test -d \"$(dirname \"$FIXTURE_LEAVES\")\" && test -d \"$FIXTURE_TAKES\"",
            where_they_are.display()
        ),
    );
    let ran = harness.run(line("true"), fixture.declared(None)).await;
    assert_eq!(
        ran.outcome,
        JobOutcome::Exited { code: 0 },
        "the up step itself asserted that {IN_TOKEN} and {OUT_TOKEN} are directories that \
         already exist:\n{}",
        ran.log
    );
    let said = std::fs::read_to_string(&where_they_are).expect("the up step wrote the paths");
    let mut paths = said.lines();
    let out = paths.next().expect("the out directory").trim();
    let taken = paths.next().expect("the in directory").trim();
    assert!(
        Path::new(out).ends_with("out") && Path::new(taken).ends_with("in"),
        "they should be this job's own two directories: {out} and {taken}"
    );
    for path in [out, taken] {
        assert!(
            path.starts_with(&harness.root()),
            "{path} has to be inside the configured data root {}",
            harness.root()
        );
    }
}

// ------------------------------------------------- what an ending means ----

/// `up` non-zero: the job **never started**, named by step and code, and
/// never an exit code a caller could read as a failing test (D108, D20). And
/// the takedown runs anyway — a workspace nobody takes down is a leak on
/// somebody's machine (E-06, invariant 2).
#[tokio::test]
async fn an_up_that_fails_never_starts_the_job_and_the_takedown_still_runs() {
    let harness = Harness::new("up-fails", |_| {}).await;
    let fixture = Fixture::new("up-fails").and_then(Step::Up, "exit 3");
    let ran = harness.run(line("true"), fixture.declared(None)).await;

    match &ran.outcome {
        JobOutcome::CouldNotStart { reason } => {
            assert!(reason.contains("up step"), "{reason}");
            assert!(reason.contains("exited 3"), "{reason}");
            assert!(reason.contains("never started"), "{reason}");
        }
        other => panic!("a preparation step that failed is not a verdict: {other:?}"),
    }
    assert_eq!(
        fixture.trace(),
        ["up", "down"],
        "fetch and run have no workspace to run in; down runs however up ended"
    );
    assert_eq!(ran.names(), ["up", "down"]);
    assert_eq!(ran.step(Step::Up).and_then(|ran| ran.exit_code), Some(3));
    assert_eq!(ran.step(Step::Down).and_then(|ran| ran.exit_code), Some(0));
    assert!(
        ran.log.contains("the up step exited 3"),
        "the log a requester reads has to say why nothing ran:\n{}",
        ran.log
    );
}

/// The same for `fetch`: the commit could not be checked out, so the job
/// never started.
#[tokio::test]
async fn a_fetch_that_fails_never_starts_the_job_and_the_takedown_still_runs() {
    let harness = Harness::new("fetch-fails", |_| {}).await;
    let fixture = Fixture::new("fetch-fails").and_then(Step::Fetch, "exit 128");
    let ran = harness.run(line("true"), fixture.declared(None)).await;

    match &ran.outcome {
        JobOutcome::CouldNotStart { reason } => {
            assert!(reason.contains("fetch step"), "{reason}");
            assert!(reason.contains("exited 128"), "{reason}");
        }
        other => panic!("a preparation step that failed is not a verdict: {other:?}"),
    }
    assert_eq!(fixture.trace(), ["up", "fetch", "down"]);
    assert_eq!(ran.names(), ["up", "fetch", "down"]);
    assert_eq!(
        ran.step(Step::Fetch).and_then(|ran| ran.exit_code),
        Some(128)
    );
}

/// The `run` step's code **is** the verdict, worn untouched: 0, 1 and 75
/// alike (E-02, D20). 75 above all — it is the consumer's "the capability I
/// was given went away", and a runner that rewrote it would break D76.
#[tokio::test]
async fn the_run_step_s_code_is_the_verdict_worn_untouched() {
    let harness = Harness::new("verdict", |_| {}).await;
    for code in [0, 1, 75] {
        let fixture = Fixture::new(&format!("verdict-{code}"));
        let ran = harness
            .run(line(&format!("exit {code}")), fixture.declared(None))
            .await;
        assert_eq!(
            ran.outcome,
            JobOutcome::Exited { code },
            "the declared line exited {code} and that is the verdict: {}",
            ran.log
        );
        assert_eq!(
            ran.step(Step::Run).and_then(|ran| ran.exit_code),
            Some(code)
        );
        assert_eq!(
            fixture.trace(),
            ["up", "fetch", "run", "down"],
            "a failing verdict is still a job that ran all four steps"
        );
    }
}

/// `down` non-zero is recorded on its own step line, said in the log, and
/// changes **nothing** about the verdict: the verdict already exists, and a
/// workspace that failed to be taken down is an operator's problem, not a
/// test result (D108).
#[tokio::test]
async fn a_down_that_fails_leaves_the_verdict_alone() {
    let harness = Harness::new("down-fails", |_| {}).await;
    let fixture = Fixture::new("down-fails").and_then(Step::Down, "exit 4");
    let ran = harness.run(line("exit 1"), fixture.declared(None)).await;

    assert_eq!(
        ran.outcome,
        JobOutcome::Exited { code: 1 },
        "the run step's 1 is the verdict, whatever the takedown did: {}",
        ran.log
    );
    assert_eq!(ran.step(Step::Down).and_then(|ran| ran.exit_code), Some(4));
    assert!(
        ran.log.contains("the down step exited 4"),
        "a takedown that did not come off is said in the log:\n{}",
        ran.log
    );
    assert!(
        ran.log.contains("verdict is unchanged"),
        "and it says that it changes nothing:\n{}",
        ran.log
    );
}

// --------------------------------------------------------------- the caps ----

/// The declared figure binds `run` when it is the smaller of the two, and the
/// record says which figure it was (D108).
#[tokio::test]
async fn the_declared_cap_binds_the_run_step_when_it_is_the_smaller_figure() {
    if let Err(why) = a_sleeping_hook_or_reason() {
        skipped(
            "the_declared_cap_binds_the_run_step_when_it_is_the_smaller_figure",
            &why,
        );
        return;
    }
    let harness = Harness::new("declared-cap", |confinement| {
        confinement.timeout = Duration::from_secs(60);
    })
    .await;
    let fixture = Fixture::new("declared-cap");
    let ran = harness
        .run(line("sleep 30"), fixture.declared(Some(1)))
        .await;

    assert_eq!(
        ran.outcome,
        JobOutcome::StoppedByCap {
            cap: herd_proto::Cap::Time,
            limit: 1,
            detail: None,
        },
        "the command declared 1 s and the runner lends 60 s: {}",
        ran.log
    );
    let run = ran.step(Step::Run).expect("the run step ran").clone();
    assert_eq!(run.exit_code, None, "nothing exited: {run:?}");
    let said = run.ended.clone().expect("a step with no code says why");
    assert!(said.contains("time cap of 1 s"), "{said}");
    assert!(
        said.contains("the figure command fixture declared"),
        "the record has to say which of the two figures bound the job: {said}"
    );
    assert!(
        ran.log.contains("capped at 1 s") && ran.log.contains("smaller than this runner's own"),
        "and the log says it before the step runs:\n{}",
        ran.log
    );
    assert_eq!(
        fixture.trace(),
        ["up", "fetch", "run", "down"],
        "a capped run step still has its workspace taken down"
    );
}

/// And the runner's own figure binds it when **it** is the smaller: a
/// requester does not overrule the owner of the machine she is lending
/// (invariant 2, D108).
#[tokio::test]
async fn the_runner_s_own_cap_binds_the_run_step_when_it_is_the_smaller_figure() {
    if let Err(why) = a_sleeping_hook_or_reason() {
        skipped(
            "the_runner_s_own_cap_binds_the_run_step_when_it_is_the_smaller_figure",
            &why,
        );
        return;
    }
    let harness = Harness::new("owner-cap", |confinement| {
        confinement.timeout = Duration::from_secs(1);
    })
    .await;
    let fixture = Fixture::new("owner-cap");
    let ran = harness
        .run(line("sleep 30"), fixture.declared(Some(600)))
        .await;

    assert_eq!(
        ran.outcome,
        JobOutcome::StoppedByCap {
            cap: herd_proto::Cap::Time,
            limit: 1,
            detail: None,
        },
        "the command declared 600 s and the runner lends 1 s: {}",
        ran.log
    );
    let said = ran
        .step(Step::Run)
        .and_then(|run| run.ended.clone())
        .expect("the run step has no code and says why");
    assert!(said.contains("time cap of 1 s"), "{said}");
    assert!(
        said.contains("this runner's own figure"),
        "the record has to say which of the two figures bound the job: {said}"
    );
    assert!(
        ran.log
            .contains("does not raise the figure the owner of this machine lends"),
        "and the log says a declared figure does not raise the owner's:\n{}",
        ran.log
    );
    assert_eq!(fixture.trace(), ["up", "fetch", "run", "down"]);
}

// ------------------------------------------------------- the two refusals ----

/// A hook holding a token nobody fills is **given back naming the token**,
/// never run with a brace in it (D107). The attempt is handed back, so the
/// hub gives the job its goes and then ends it as lost, carrying the
/// refusal's own words.
#[tokio::test]
async fn a_hook_left_holding_a_token_gives_the_attempt_back_naming_the_token() {
    let harness = Harness::new("token-left", |_| {}).await;
    let fixture = Fixture::new("token-left");
    let mut declared = fixture.declared(None);
    let declaration = declared
        .declaration
        .as_mut()
        .expect("the fixture declares a command");
    // What `herdctl` fills before a job is minted (D107) — left unfilled here,
    // which is exactly the case the runner must not guess at.
    declaration.executor.fetch = format!("{} {COMMIT_TOKEN}", declaration.executor.fetch);
    let ran = harness.run(line("true"), declared).await;

    match &ran.outcome {
        JobOutcome::Lost { reason } => {
            assert!(reason.contains(COMMIT_TOKEN), "{reason}");
            assert!(reason.contains("fixture"), "{reason}");
            assert!(
                reason.contains("brace"),
                "the refusal says what it refused to do: {reason}"
            );
        }
        other => panic!("a declaration nobody can fill is given back, not run: {other:?}"),
    }
    assert!(
        fixture.trace().is_empty(),
        "nothing may run: {:?}",
        fixture.trace()
    );
    assert!(
        ran.steps.is_empty(),
        "an attempt that never ran a step records none: {:?}",
        ran.steps
    );
}

/// A declared command whose line is not exactly one word is given back naming
/// that: Herd neither joins nor splits a command line (E-11).
#[tokio::test]
async fn a_declared_line_that_is_not_one_word_is_given_back_naming_that() {
    let harness = Harness::new("two-words", |_| {}).await;
    let fixture = Fixture::new("two-words");
    let ran = harness
        .run(
            vec!["printf".to_owned(), "two\\n".to_owned()],
            fixture.declared(None),
        )
        .await;

    match &ran.outcome {
        JobOutcome::Lost { reason } => {
            assert!(reason.contains("2 word(s)"), "{reason}");
            assert!(reason.contains("exactly one word"), "{reason}");
        }
        other => panic!("a line Herd would have to guess at is given back: {other:?}"),
    }
    assert!(
        fixture.trace().is_empty(),
        "nothing may run: {:?}",
        fixture.trace()
    );
}

// ------------------------------------------------------- cancel and ad hoc ----

/// A cancel in the middle of `run` still takes the workspace down (E-06,
/// D108). The cancel is sent only once the `run` step has written its own
/// name, so "in the middle of run" is a fact and not a hope (D101).
#[tokio::test]
async fn a_cancel_during_the_run_step_still_takes_the_workspace_down() {
    if let Err(why) = a_sleeping_hook_or_reason() {
        skipped(
            "a_cancel_during_the_run_step_still_takes_the_workspace_down",
            &why,
        );
        return;
    }
    let harness = Harness::new("cancel-mid-run", |_| {}).await;
    let fixture = Fixture::new("cancel-mid-run");
    let job_id = herdctl::submit_job(
        &harness.requester(),
        line("sleep 30"),
        herd_proto::LabelSet::default(),
        herd_proto::Priority::Step,
        fixture.declared(None),
    )
    .await
    .expect("the hub accepts the submission");

    let deadline = Instant::now() + PATIENCE;
    while !fixture.trace().iter().any(|name| name == "run") {
        assert!(
            Instant::now() < deadline,
            "the run step never started: {:?}",
            fixture.trace()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    herdctl::cancel_job(&harness.requester(), job_id.clone())
        .await
        .expect("the hub answers the cancel");

    let finished = herdctl::wait_for_job(&harness.requester(), job_id.clone(), |_, _| {}, |_| {})
        .await
        .expect("the hub answers with an outcome");
    match &finished.outcome {
        JobOutcome::Cancelled { .. } => {}
        other => panic!("a cancelled job is cancelled, not a verdict: {other:?}"),
    }
    let deadline = Instant::now() + PATIENCE;
    while !fixture.trace().iter().any(|name| name == "down") {
        assert!(
            Instant::now() < deadline,
            "the takedown never ran after the cancel: {:?}",
            fixture.trace()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let steps = harness.steps_of(&job_id);
    let run = steps
        .iter()
        .find(|ran| ran.step == Step::Run)
        .expect("the run step has a line");
    assert_eq!(run.exit_code, None, "nothing exited: {run:?}");
    assert!(
        run.ended
            .as_ref()
            .is_some_and(|said| said.contains("ended before it finished")),
        "the run step's line says it was ended: {run:?}"
    );
}

/// A step reaches the record **as it ends**, not only with the verdict
/// (`ToHub::StepFinished`, D108). That is the whole reason the frame exists: a
/// 5 h nightly stopped at its cap, or a runner lost under a 30-minute cache
/// pull, would otherwise leave no step on the record at all — the preparation
/// time invisible in exactly the case someone is looking for it.
///
/// Asserted while the job is still running, which is the only moment at which
/// it can be asserted: the complete list arrives again at the end and would
/// hide a frame that never went out.
#[tokio::test]
async fn the_steps_reach_the_record_as_they_end_rather_than_only_with_the_verdict() {
    if let Err(why) = a_sleeping_hook_or_reason() {
        skipped(
            "the_steps_reach_the_record_as_they_end_rather_than_only_with_the_verdict",
            &why,
        );
        return;
    }
    let harness = Harness::new("steps-as-they-end", |_| {}).await;
    let fixture = Fixture::new("steps-as-they-end");
    let job_id = herdctl::submit_job(
        &harness.requester(),
        line("sleep 30"),
        herd_proto::LabelSet::default(),
        herd_proto::Priority::Step,
        fixture.declared(None),
    )
    .await
    .expect("the hub accepts the submission");

    // The `run` step has started — its own hook said so on a disk — so `up`
    // and `fetch` have both ended and the job has certainly not.
    let deadline = Instant::now() + PATIENCE;
    while !fixture.trace().iter().any(|name| name == "run") {
        assert!(
            Instant::now() < deadline,
            "the run step never started: {:?}",
            fixture.trace()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let deadline = Instant::now() + PATIENCE;
    loop {
        let names: Vec<&str> = harness
            .steps_of(&job_id)
            .iter()
            .map(|ran| ran.step.name())
            .collect();
        if names == ["up", "fetch"] {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the record should already hold up and fetch while the run step is still \
             going, and holds {names:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let record = harness
        .store
        .job(&job_id)
        .expect("the record is readable")
        .expect("the hub minted this job");
    assert!(
        !record.state.is_terminal(),
        "the two step lines have to be on the record *before* the job ends: {:?}",
        record.state
    );

    // Ended here rather than waited out: this test is about when a step line
    // lands, not about a half-hour sleep.
    herdctl::cancel_job(&harness.requester(), job_id.clone())
        .await
        .expect("the hub answers the cancel");
    let _ = herdctl::wait_for_job(&harness.requester(), job_id, |_, _| {}, |_| {}).await;
}

/// An ad-hoc job — no declaration — ends with **no step at all**. That is not
/// a leftover: it is how Herd is tested against itself with no consumer tree
/// in the picture, which is what E-11 asks of Herd's own code.
#[tokio::test]
async fn an_ad_hoc_job_still_ends_with_no_step_at_all() {
    let harness = Harness::new("ad-hoc", |_| {}).await;
    let ran = harness
        .run(
            shell("printf 'an ad-hoc job\\n'"),
            herdctl::Declared::default(),
        )
        .await;
    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 }, "{}", ran.log);
    assert!(ran.log.contains("an ad-hoc job"), "{}", ran.log);
    assert!(
        ran.steps.is_empty(),
        "a job that is not a declared command ran no executor step: {:?}",
        ran.steps
    );
}

/// A declared command whose `run` hook cannot be run at all is Herd failing
/// to run the job, never the job failing (D20) — and the workspace is still
/// taken down.
#[tokio::test]
async fn a_run_step_whose_line_is_not_a_command_is_still_a_verdict_about_the_line() {
    let harness = Harness::new("no-such-line", |_| {}).await;
    let fixture = Fixture::new("no-such-line");
    let ran = harness
        .run(
            line("herd-no-such-command-anywhere"),
            fixture.declared(None),
        )
        .await;
    // The shell ran; it is the shell that could not find the command, so the
    // step exited 127 and 127 is the line's own verdict.
    assert_eq!(
        ran.outcome,
        JobOutcome::Exited { code: 127 },
        "the shell's own answer to a command it cannot find: {}",
        ran.log
    );
    assert_eq!(fixture.trace(), ["up", "fetch", "run", "down"]);
}

/// **The revision a `fetch` really reached, end to end** (D176, invariant
/// 5).
///
/// A real hub, a real runner, a real shell, and a `fetch` hook that answers
/// at `{ran}` the way a consumer's own would: `… && printf '%s' "$(…)" >
/// {ran}`. Three cases, because the field is worth nothing unless all three
/// are told apart on the record:
///
/// * the hook answers with the commit the job declared — agreement;
/// * the hook answers with something else — a **divergence**, recorded,
///   with the job's verdict untouched;
/// * the hook answers nothing, which is where every consumer starts.
///
/// The commit the fixture declares is `cb5a…`, forty hex, as `herdctl`
/// would have resolved it (E-01).
#[tokio::test]
async fn a_fetch_hook_that_answers_at_ran_puts_the_revision_on_the_record() {
    let declared = "cb5a".repeat(10);
    let harness = Harness::new("ran-agreed", |_| {}).await;
    let fixture = Fixture::new("ran-agreed").and_then(
        Step::Fetch,
        &format!("printf '%s\\n' {declared} > {RAN_TOKEN}"),
    );
    let ran = harness.run(line("true"), fixture.declared(None)).await;

    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 }, "{}", ran.log);
    assert_eq!(
        ran.step(Step::Fetch)
            .expect("the fetch step ran")
            .ran_commit
            .as_deref(),
        Some(declared.as_str()),
        "the runner carried home what the hook wrote, newline trimmed"
    );
    let commit = ran
        .recorded_commit
        .clone()
        .expect("the record compared the two");
    assert_eq!(commit.declared, declared);
    assert_eq!(commit.ran.as_deref(), Some(declared.as_str()));
    assert!(!commit.diverged(), "the tree was where it was asked to be");
    assert_eq!(
        ran.told_the_requester.as_ref(),
        Some(&commit),
        "the requester is told what the record says, and from the record"
    );

    // No step but `fetch` answers this question: a revision read after
    // `run` would be whatever the job's own command left behind.
    for step in [Step::Up, Step::Run, Step::Down] {
        assert_eq!(
            ran.step(step).and_then(|run| run.ran_commit.clone()),
            None,
            "only fetch reports a revision, not {step}"
        );
    }
}

/// The case the whole field exists for: a `fetch` that **exits 0 without
/// reaching the declared commit**.
///
/// Before this, the verdict was attributed to a commit the tree was never
/// at and nothing on the record could contradict it. Now the record says
/// both, the requester is told both, and the job's own verdict is exactly
/// what it was — a divergence is recorded, never refused, and never turned
/// into a failure behind the consumer's back (D20).
#[tokio::test]
async fn a_fetch_that_reached_another_revision_is_a_divergence_and_not_a_refusal() {
    let declared = "cb5a".repeat(10);
    let elsewhere = "9f41".repeat(10);
    let harness = Harness::new("ran-diverged", |_| {}).await;
    // A `fetch` that exits 0 having gone somewhere else entirely: a stale
    // mirror, a fallback, a `switch` whose failure an `&&` swallowed.
    let fixture = Fixture::new("ran-diverged").and_then(
        Step::Fetch,
        &format!("printf '%s\\n' {elsewhere} > {RAN_TOKEN}"),
    );
    let ran = harness.run(line("true"), fixture.declared(None)).await;

    assert_eq!(
        ran.outcome,
        JobOutcome::Exited { code: 0 },
        "the job passed, and that is still the verdict: {}",
        ran.log
    );
    let commit = ran.recorded_commit.expect("the record compared the two");
    assert!(commit.diverged(), "{commit:?}");
    assert_eq!(commit.declared, declared);
    assert_eq!(commit.ran.as_deref(), Some(elsewhere.as_str()));
    assert!(
        commit.line().contains(&declared) && commit.line().contains(&elsewhere),
        "the record's own sentence names both: {}",
        commit.line()
    );
    assert_eq!(
        ran.told_the_requester,
        Some(commit),
        "a divergence a requester cannot see is not an oracle"
    );
    assert_eq!(fixture.trace(), ["up", "fetch", "run", "down"]);
}

/// Every consumer's first day: a `fetch` hook that names `{ran}` nowhere.
///
/// Nothing fails, nothing is refused, and the record says **"did not
/// report"** rather than leaving a blank that reads as agreement.
#[tokio::test]
async fn a_fetch_hook_that_says_nothing_is_recorded_as_having_said_nothing() {
    let harness = Harness::new("ran-silent", |_| {}).await;
    let fixture = Fixture::new("ran-silent");
    let ran = harness.run(line("true"), fixture.declared(None)).await;

    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 }, "{}", ran.log);
    assert_eq!(
        ran.step(Step::Fetch)
            .expect("the fetch step ran")
            .ran_commit,
        None
    );
    let commit = ran
        .recorded_commit
        .expect("the record still says something");
    assert_eq!(commit.ran, None);
    assert!(!commit.diverged(), "silence is not a divergence");
    assert!(
        commit.line().contains("did not report a revision"),
        "{}",
        commit.line()
    );
}

/// A hook that writes something that is not an answer.
///
/// The record gets silence — the runner has no revision to put on it — and
/// the **job's log** says why, so a consumer whose hook writes the wrong
/// thing does not look like a consumer whose hook writes nothing. The job
/// itself is untouched: a bad answer to a question Herd asked politely is
/// not a reason to fail somebody's build.
#[tokio::test]
async fn a_revision_file_that_cannot_be_used_is_said_in_the_log_and_not_on_the_record() {
    let harness = Harness::new("ran-unusable", |_| {}).await;
    let fixture = Fixture::new("ran-unusable").and_then(
        Step::Fetch,
        // A hook that wrote its own error message into the answer.
        &format!("printf 'fatal: not a git repository\\n' > {RAN_TOKEN}"),
    );
    let ran = harness.run(line("true"), fixture.declared(None)).await;

    assert_eq!(ran.outcome, JobOutcome::Exited { code: 0 }, "{}", ran.log);
    assert_eq!(
        ran.step(Step::Fetch)
            .expect("the fetch step ran")
            .ran_commit,
        None,
        "an answer Herd cannot use is not an answer"
    );
    assert!(
        ran.log.contains("so this attempt reports no revision"),
        "the job's log says why it is silent: {}",
        ran.log
    );
}
