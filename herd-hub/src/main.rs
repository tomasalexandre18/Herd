use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use herd_hub::{
    Authority, Budget, Hub, JobDirStore, JobStore, Policy, Stickiness, Stop, StopSignals,
    AGING_STEP, DEFAULT_LISTEN_ADDR, DRAIN_FIRST_LIMIT, LOST_AFTER, OUTPUT_BUDGET, OUTPUT_FLOOR,
    READINGS_AT_ONCE, READING_CAP, REQUEUE_BUDGET, STICKINESS_LIMIT, STOP_WITHIN, WAIT_RESTATEMENT,
};
use herd_proto::{HEALTH_EVERY, STILL_THERE_EVERY};

/// Where the hub keeps its state unless told otherwise. Relative on purpose:
/// a development hub writes beside the repository, and the deployment names
/// its volume (Q9, M16).
const DEFAULT_STATE_DIR: &str = "herd-state";

/// The Herd hub: accepts a runner and requesters, dispatches jobs, relays logs.
#[derive(Debug, Parser)]
#[command(name = "herd-hub", version, about)]
struct Args {
    /// Address to listen on. Plain ws: TLS is the reverse proxy's job (D4).
    #[arg(long, env = "HERD_LISTEN", default_value = DEFAULT_LISTEN_ADDR)]
    listen: SocketAddr,
    /// Directory holding the hub's state: one directory per job (D8). It is
    /// created if it is missing, and the hub refuses to start if it cannot be
    /// written.
    #[arg(long, env = "HERD_STATE_DIR", default_value = DEFAULT_STATE_DIR)]
    state_dir: PathBuf,
    /// File holding the one shared token every requester must carry (Q8).
    ///
    /// A **path**, never the token itself: a secret on a command line is
    /// readable out of `/proc` by every user of the host, and invariant 7
    /// names a file path given at startup as the way to pass one. The hub
    /// reads it once, keeps a hash, and refuses every requester that does not
    /// carry it. Left out, this hub authenticates no requester — which is
    /// only safe where it listens by default, on loopback behind the reverse
    /// proxy (D14) — and says so in one line as it starts.
    ///
    /// Runners are not covered by this: each one carries a token of its own,
    /// minted by `herdctl runner approve` and kept hashed in the state root
    /// (E-48). An unapproved runner is refused whatever this option says.
    #[arg(long, env = "HERD_REQUESTER_TOKEN_FILE", value_name = "PATH")]
    requester_token_file: Option<PathBuf>,
    /// Seconds without a health report after which a runner is lost and the
    /// jobs it was running are attempted once more (E-21). It must outlast
    /// three health reports, or a healthy runner is declared lost between two
    /// of its own: the hub refuses to start otherwise, naming both figures.
    #[arg(long, env = "HERD_LOST_AFTER", default_value_t = LOST_AFTER.as_secs_f64())]
    lost_after: f64,
    /// Seconds between the health reports this hub expects of its runners
    /// (E-40). It sends none itself: this is what `--lost-after` is judged
    /// against at startup. A runner that announces a cadence of its own is
    /// judged against *that* when it dials, and refused by name if
    /// `--lost-after` cannot outlast three of its reports — so this figure is
    /// the expectation, never the whole of the check.
    #[arg(long, env = "HERD_HEALTH_EVERY", default_value_t = HEALTH_EVERY.as_secs_f64())]
    health_every: f64,
    /// Seconds a queued job waits before its priority counts as one level
    /// higher, so that the lowest level never starves (E-18). The bound it
    /// gives is this figure times the number of levels above the job: at the
    /// default, ninety minutes for the lowest of the four. Zero turns aging
    /// off, which is said in the sentence every waiting job carries.
    #[arg(long, env = "HERD_AGING_STEP", default_value_t = AGING_STEP.as_secs_f64())]
    aging_step: f64,
    /// Seconds a job waits for the runner it is sticking to before this hub
    /// places it on a less preferred one (E-19).
    ///
    /// A job goes to the most preferred machine that can run it, and waits
    /// for that machine while it is connected rather than falling back to a
    /// lesser one that is free. This is the safety limit on that wait, and
    /// E-19 asks for it to be visible: it is printed at startup and named in
    /// the sentence of every job waiting on it. Zero makes a job move the
    /// moment anything else can take it, which is to say it turns the
    /// stickiness off.
    #[arg(long, env = "HERD_STICKINESS_LIMIT", default_value_t = STICKINESS_LIMIT.as_secs_f64())]
    stickiness_limit: f64,
    /// Seconds between two tellings of why a queued job is still waiting,
    /// to whoever is following it (E-04).
    ///
    /// E-04 asks for "at least every 60 s", which is the default. It is a
    /// setting because both ways of failing it are real: a rarer cadence
    /// leaves a ten-minute wait in silence, and a line per dispatch pass
    /// fills a transcript with hundreds of identical sentences. Over a wait
    /// of W a follower gets at most W / this + 1 lines. Zero is refused.
    #[arg(long, env = "HERD_WAIT_RESTATEMENT", default_value_t = WAIT_RESTATEMENT.as_secs_f64())]
    wait_restatement: f64,
    /// Seconds a follower of a job that is not over may hear nothing at all
    /// before this hub speaks to it.
    ///
    /// A job that is *running* and printing nothing sends its follower no
    /// frames whatever, and behind a reverse proxy with an idle timeout that
    /// connection is closed under the follower — which `herdctl` reads as the
    /// stream ending with no verdict and reports as exit 125 on a job that
    /// succeeded. So the hub breathes on the link at this cadence instead.
    /// Nothing is printed for it and no client acts on the figure. Fifteen
    /// seconds, the same number as `--health-every` on purpose. Zero is
    /// refused.
    #[arg(long, env = "HERD_STILL_THERE_EVERY", default_value_t = STILL_THERE_EVERY.as_secs_f64())]
    still_there_every: f64,
    /// Seconds a job may hold a runner empty while it waits for that host to
    /// go quiet (E-27).
    ///
    /// A capability its machine's operator declared `drain_first` starts only
    /// on a quiet host, and while such a job is waiting the host claims
    /// nothing new. This bounds that: past it the host takes other work
    /// again and the job says, in the sentence on its record, that the drain
    /// did not win. Fifteen minutes, the figure E-27 names.
    #[arg(long, env = "HERD_DRAIN_FIRST_LIMIT", default_value_t = DRAIN_FIRST_LIMIT.as_secs_f64())]
    drain_first_limit: f64,
    /// How many goes a job gets at a capability that turned out to be taken
    /// (D76).
    ///
    /// A job may declare one exit code meaning "what you gave me has gone"
    /// (`herdctl --requeue-exit-code`, 75 for the first consumer). Such a
    /// code is not a verdict: it requeues the job with the capability read
    /// again, this many times, and then ends it naming the code and the
    /// machines that answered it. **Separate from `--attempts`**, which is
    /// E-21's budget for a runner that disappears.
    #[arg(long, env = "HERD_REQUEUE_BUDGET", default_value_t = REQUEUE_BUDGET)]
    requeue_budget: u32,
    /// Seconds the `run` step of a `labels_from` reading may take (D74).
    ///
    /// A command may declare that what its jobs need is read on a machine
    /// rather than stated in the file. That reading is an ordinary job: it
    /// goes through the one queue, it is confined like any other, and this is
    /// the cap on it. Five minutes by default, because a reading builds and
    /// boots nothing; a reading that overruns it refuses the job it was read
    /// for, by name, rather than letting it be placed on a guess. The
    /// runner's own cap still bounds this and is never raised by it (D108).
    #[arg(long, env = "HERD_READING_CAP", default_value_t = READING_CAP.as_secs_f64())]
    reading_cap: f64,
    /// How many requirement readings one runner may run at once (D169).
    ///
    /// A reading takes no slot and no capability — it builds and boots
    /// nothing (D159) — so this is the only thing that bounds how many of
    /// them one machine is given. Two by default. It exists because a
    /// consumer submitted 214 jobs in one second, every one of them needing
    /// a reading first, and the hub put some two hundred of those readings
    /// on the one runner it had. Beyond this figure a reading **queues**: it
    /// is never refused, and it is named in the sentence of every job
    /// waiting behind it. Zero is refused.
    #[arg(long, env = "HERD_READINGS_AT_ONCE", default_value_t = READINGS_AT_ONCE)]
    readings_at_once: u32,
    /// How much of this hub's volume a job's output files may hold, in all
    /// (D148, D152). `200GB`, `64MiB`, or a plain number of bytes.
    ///
    /// **The output files only**, not the state root: a record is kilobytes
    /// and a log has a cap of its own (`--job-log-cap` on the runner), and
    /// one number answering for three different things would be a number
    /// nobody can reason about. It counts whole, verified collections in
    /// every job's `files/`; a collection still crossing is outside it,
    /// because evicting a transfer in flight would destroy the thing the
    /// hub is being given.
    ///
    /// **It is a chosen figure and not a derived one.** Nothing computed
    /// 200 GB: the arithmetic that would — a few hundred megabytes times the
    /// jobs a day that really leave that much — needs a distribution only
    /// the first consumer has. D148 says at length why that matters.
    #[arg(
        long,
        env = "HERD_OUTPUT_BUDGET",
        default_value_t = herd_proto::show_bytes(OUTPUT_BUDGET),
        value_name = "SIZE",
    )]
    output_budget: String,
    /// Seconds a job's output files are promised before the sweeper may
    /// take them (D148, D152). Seven days.
    ///
    /// **A floor, not a lifetime.** Nothing is removed for merely being
    /// older than this, and a collection outlives it by as long as the
    /// budget allows; what it does is keep a fresh collection safe while
    /// there is anything older to take instead. When honouring it would
    /// mean going over the budget, the budget wins and this hub **says so
    /// on the day**, naming both figures — a floor that broke quietly would
    /// be worse than no floor, because the next thing that happens is a
    /// full disk nobody saw coming.
    ///
    /// Zero turns the promise off, in the way `--aging-step 0` turns aging
    /// off: everything is then evictable by the order alone.
    #[arg(long, env = "HERD_OUTPUT_FLOOR", default_value_t = OUTPUT_FLOOR.as_secs_f64())]
    output_floor: f64,
    /// What this binary does instead of serving. Left out, it serves, which
    /// is every existing invocation unchanged.
    #[command(subcommand)]
    command: Option<Command>,
}

/// The one thing this binary does that is not serving.
///
/// **Why it is a subcommand of the hub and not of `herdctl`** (ORCH-125).
/// `herdctl` is a requester: everything it does goes over the WebSocket
/// protocol, and giving it this would mean a new message, which means a
/// `PROTOCOL_VERSION` bump on a version that is frozen at 2. It is also the
/// wrong machine — a requester runs anywhere, and this reads a directory
/// that exists on one host. The hub binary is already in the consumer's
/// image, so `docker compose exec hub herd-hub report` needs no new image,
/// no new entrypoint and no port: the operator who runs the hub can run
/// this against the live root, which is the only way its figures ever reach
/// this project (D161, invariant 9).
///
/// A subcommand rather than a flag on the serving path, because every one
/// of the options above is about serving and none of them means anything
/// here. A `--report` flag would have to be documented as ignoring
/// seventeen others.
#[derive(Debug, Subcommand)]
enum Command {
    /// Print what a state root holds, serve nothing, and exit.
    ///
    /// Three figures: the file-level deduplication ratio over the records,
    /// per-job output size by decile, and what the records claim against
    /// what the directories measure. It is safe against a running hub — it
    /// takes no lock, writes nothing, and reads no output or log byte — and
    /// what it prints is a point-in-time snapshot, which it says itself.
    Report {
        /// The state root to read. The same directory the serving hub is
        /// given, so `HERD_STATE_DIR` alone is enough inside the hub's own
        /// container.
        #[arg(
            env = "HERD_STATE_DIR",
            default_value = DEFAULT_STATE_DIR,
            value_name = "STATE_DIR",
        )]
        state_dir: PathBuf,
    },
    /// Print what record retention would remove from a state root, and what
    /// it would cost. **It removes nothing** (ORCH-146).
    ///
    /// Per priority class: how many records are past the span given for that
    /// class, what they weigh, the oldest and the newest of them, and the
    /// clock reading every figure was judged against. It is safe against a
    /// running hub for the same reasons `report` is — no lock, no write, no
    /// output or log byte read — and it has no deletion path and no flag
    /// that could be given one. M12's retention engine is a separate thing;
    /// this is what an operator runs *before* it, because once a record is
    /// gone nothing in Herd can bring it back.
    ///
    /// **Every span is required and none is defaulted.** The spans in
    /// `docs/plan.md` and `docs/disk-policy.md` are a recorded policy and
    /// this binary states none of its own: a shipped default here would be a
    /// number nobody could later change (D194). A span of `0` means that
    /// class **never expires**, and the output says so in those words.
    RetentionPreview {
        /// The state root to read, as `report` takes it.
        #[arg(
            env = "HERD_STATE_DIR",
            default_value = DEFAULT_STATE_DIR,
            value_name = "STATE_DIR",
        )]
        state_dir: PathBuf,
        /// How long a `merger` record would be kept: `90d`, `36h`, `90m`,
        /// `30s`, or `0` for never. The unit is required.
        #[arg(long, required = true, value_name = "SPAN", value_parser = span)]
        merger: std::time::Duration,
        /// How long a `step` record would be kept. Same shape.
        #[arg(long, required = true, value_name = "SPAN", value_parser = span)]
        step: std::time::Duration,
        /// How long a `nightly` record would be kept. Same shape.
        #[arg(long, required = true, value_name = "SPAN", value_parser = span)]
        nightly: std::time::Duration,
        /// How long a `producer` record would be kept. Same shape.
        #[arg(long, required = true, value_name = "SPAN", value_parser = span)]
        producer: std::time::Duration,
    },
}

/// A retention span as the command line gives one, with the unit required.
///
/// A thin wrapper so clap reports the refusal itself, naming the value the
/// operator typed: a span is the one figure here whose mistyping cannot be
/// recovered from, so it is refused before the state root is even opened.
fn span(text: &str) -> Result<std::time::Duration, String> {
    herd_hub::parse_span(text)
}

/// The shared requester token, from the file the operator named (Q8).
///
/// Trimmed, because a file written with `echo` ends in a newline and a token
/// that differed from itself by one byte would be a refusal nobody could
/// explain. Empty is an error and not "no token": a hub told to require a
/// token and handed an empty file must not quietly serve everyone.
fn read_requester_token(path: &Path) -> Result<herd_hub::SharedSecret> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the requester token from {}", path.display()))?;
    let token = text.trim();
    if token.is_empty() {
        anyhow::bail!(
            "the requester token file {} is empty: this hub was told to require a token and \
             an empty one would admit every requester",
            path.display()
        );
    }
    herd_hub::SharedSecret::of(token)
        .map_err(|error| anyhow::anyhow!("hashing the requester token: {error}"))
}

/// Seconds as a duration, with a negative figure read as zero rather than
/// silently becoming a very long wait.
fn seconds(value: f64) -> std::time::Duration {
    std::time::Duration::try_from_secs_f64(value.max(0.0)).unwrap_or(std::time::Duration::ZERO)
}

#[tokio::main]
async fn main() -> Result<()> {
    // **Before everything**, including the option parsing and the opening of
    // the state root (ORCH-081). Until these handlers exist, `SIGTERM` kills
    // this process where it stands, and opening the state root sweeps it —
    // which on a large volume is the longest, least interruptible moment
    // this binary has. Installed here, a signal that lands in that moment is
    // buffered and acted on the instant the serving loop begins.
    let signals = StopSignals::listen();
    let args = Args::parse();
    // Before the budget is parsed and long before the state root is opened,
    // because opening it takes the `flock` on `hub.lock` and sweeps: a report
    // that did either would be a report nobody may run against the hub that
    // is serving (ORCH-125).
    if let Some(Command::Report { state_dir }) = &args.command {
        let report = herd_hub::StateRootReport::read(state_dir)
            .with_context(|| format!("reading the state root {}", state_dir.display()))?;
        print!("{report}");
        return Ok(());
    }
    // Here for the same reason, and with one thing more: it removes nothing,
    // so the only way it could harm this root is by taking a lock or writing
    // to it, and it is placed before anything that does either (ORCH-146).
    if let Some(Command::RetentionPreview {
        state_dir,
        merger,
        step,
        nightly,
        producer,
    }) = &args.command
    {
        let spans = herd_hub::Spans::new(*merger, *step, *nightly, *producer);
        let preview =
            herd_hub::RetentionPreview::read(state_dir, spans, herd_hub::Timestamp::now())
                .with_context(|| format!("reading the state root {}", state_dir.display()))?;
        print!("{preview}");
        // A refusal is not a success. The figures an operator would have
        // acted on were deliberately not printed, so the exit status must not
        // say "here they are": a script that read this as green and went on
        // to run the engine is exactly the sequence this subcommand exists to
        // interrupt.
        if let Some(why) = preview.verdict.refusal() {
            anyhow::bail!("{why}");
        }
        return Ok(());
    }
    // Read before the state root is opened, because opening it sweeps: a
    // budget that does not parse must stop the hub here, where the message
    // names the figure the operator typed, and not after bytes have gone.
    let budget = Budget {
        bytes: herd_proto::parse_bytes(&args.output_budget)
            .map_err(|why| anyhow::anyhow!("--output-budget: {why}"))?,
        floor: seconds(args.output_floor),
    };
    let store = JobDirStore::open_with(&args.state_dir, budget).with_context(|| {
        format!(
            "the hub's state directory {} is not usable",
            args.state_dir.display()
        )
    })?;
    let unfinished = store.unfinished_jobs()?.len();
    // A job the hub read and will not act on is named here as well as on the
    // store's own log, because the count below would otherwise be the only
    // number anyone reads and it does not include them (D52).
    let quarantined = store.quarantined()?;
    for (id, why) in &quarantined {
        println!("herd-hub: {id} is quarantined and will not be dispatched: {why}");
    }
    let policy = Policy {
        lost_after: seconds(args.lost_after),
        health_every: seconds(args.health_every),
        aging_step: seconds(args.aging_step),
        stickiness: Stickiness {
            limit: seconds(args.stickiness_limit),
        },
        wait_restatement: seconds(args.wait_restatement),
        still_there_every: seconds(args.still_there_every),
        drain_first_limit: seconds(args.drain_first_limit),
        requeue_budget: args.requeue_budget,
        reading_cap: seconds(args.reading_cap),
        readings_at_once: args.readings_at_once,
        ..Policy::default()
    };
    let state_root_id = store.state_root_id().to_owned();
    let approved = store.approved_runner_count();
    // Who is allowed here (E-48, Q8). Read before the port is bound: a hub
    // that started, accepted requesters and then found its token file
    // unreadable would be an open hub nobody meant to run.
    let authority = Authority {
        requester_token: match &args.requester_token_file {
            Some(path) => Some(read_requester_token(path)?),
            None => None,
        },
    };
    match &args.requester_token_file {
        Some(path) => println!(
            "herd-hub: every requester must carry the token in {} (--requester-token-file); \
             this hub keeps only a hash of it",
            path.display()
        ),
        None => println!(
            "herd-hub: NO requester token is set, so anything that can reach this hub may \
             submit, cancel and approve runners. That is only safe on loopback behind the \
             reverse proxy; set --requester-token-file (HERD_REQUESTER_TOKEN_FILE) before \
             this hub is reachable from anywhere else"
        ),
    }
    println!(
        "herd-hub: {approved} runner(s) are approved on this state root; a runner that is not \
         approved, or that carries the wrong token, is refused by name and claims nothing \
         (`herdctl runner approve <name>` mints one, `herdctl runner list` says who is \
         approved)"
    );
    // The two retention figures, printed like every other setting this hub
    // has (D148). An operator reading one line has to be able to answer the
    // consumer's own question — *how long do I have to fetch yesterday's
    // failing screenshots?* — and the honest answer is two numbers and the
    // sentence that joins them.
    println!(
        "herd-hub: a job's output files have {} on this volume, with the newest {} of them \
         kept while there is anything older to remove instead; past the budget the oldest \
         go first among the jobs that passed and were already fetched, and a collection \
         still arriving is never removed. When honouring the floor would mean going over \
         the budget, the budget wins and this hub says so on the day. A record keeps every \
         path, size and digest for ever: only the bytes are ever removed.",
        herd_proto::show_bytes(budget.bytes),
        herd_hub::in_days(budget.floor),
    );
    // A timing that cannot mean what it says is refused here, before the port
    // is bound and before any runner can join (E-21, E-40).
    let hub = Hub::bind_with(args.listen, Arc::new(store), policy, authority).await?;
    println!(
        "herd-hub {} listening on ws://{}, state in {} ({state_root_id}), \
         {unfinished} job(s) not over, {} quarantined, a runner reporting every {:?} is \
         lost after {:?}, a queued job rising one priority level every {:?}, and waiting \
         up to {:?} for the runner it is sticking to before it is placed elsewhere, and \
         saying again every {:?} why a job it is following is still queued and breathing \
         on a quiet follower's link every {:?}; a job that \
         needs a quiet host holds one empty for at most {:?}, and a job whose declared \
         requeue code comes back gets {} goes at it, separately from the {} goes it gets \
         at a runner that goes away; a reading of what a job needs is capped at {:?}, and \
         one runner runs at most {} of them at once",
        herd_hub::hub_version(),
        hub.local_addr(),
        args.state_dir.display(),
        quarantined.len(),
        policy.health_every,
        policy.lost_after,
        policy.aging_step,
        policy.stickiness.limit,
        policy.wait_restatement,
        policy.still_there_every,
        policy.drain_first_limit,
        policy.requeue_budget,
        policy.attempts_allowed,
        policy.reading_cap,
        policy.readings_at_once,
    );
    // Said at startup and not only at the stop, because it is a promise an
    // operator plans around: a container's own grace period has to outlast
    // this, or the stop path is cut off by a SIGKILL before it can finish
    // (ORCH-081).
    println!(
        "herd-hub: SIGTERM and SIGINT stop this hub cleanly — it stops accepting connections \
         and submissions, lets the dispatch pass in flight finish, and exits, giving that \
         pass at most {STOP_WITHIN:?}. Give the container at least that much grace. Jobs \
         already running are not cancelled and their runners keep their verdicts until a hub \
         records them."
    );
    match hub.serve_until_stopped(signals).await? {
        Stop::Clean { asked, took } => {
            println!(
                "herd-hub: {asked}, and it stopped cleanly: the dispatch pass in flight was \
                 over after {took:?}, inside the {STOP_WITHIN:?} this hub allows. Nothing \
                 queued was lost and no attempt was numbered on the way out."
            );
            Ok(())
        }
        // Non-zero, and loudly: an ordinary stop that is quietly a crash is
        // the thing this path exists to remove, so one that is still a crash
        // must be countable. The store is no worse off than after a SIGKILL
        // at the same instant — the writes are atomic and the next start
        // names any attempt this one died inside — but the operator is owed
        // the fact that it happened.
        Stop::Overran { asked, after } => anyhow::bail!(
            "{asked}, and the dispatch pass in flight had not finished after {after:?}, so \
             this hub is leaving under it. That costs exactly what a SIGKILL at this instant \
             would have cost and no more: every record is replaced whole or not at all, and \
             the next start names any attempt this one died inside. If it keeps happening, \
             the queue at this hub's depth takes longer to judge than the bound allows."
        ),
        Stop::ListenerFailed(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// clap's own check of the derive, plus the two invocations that have to
    /// keep meaning what they mean.
    ///
    /// The first matters most: adding a subcommand to a binary that had none
    /// must leave every existing invocation of it alone, because the
    /// consumer's compose file is one of them.
    #[test]
    fn the_command_line_is_well_formed_and_the_serving_hub_is_unchanged() {
        use clap::CommandFactory;
        Args::command().debug_assert();

        let serving = Args::parse_from(["herd-hub", "--state-dir", "/srv/herd-state"]);
        assert!(
            serving.command.is_none(),
            "no subcommand is still the serving hub"
        );
        assert_eq!(serving.state_dir, PathBuf::from("/srv/herd-state"));

        let reporting = Args::parse_from(["herd-hub", "report", "/srv/herd-state"]);
        match reporting.command {
            Some(Command::Report { state_dir }) => {
                assert_eq!(state_dir, PathBuf::from("/srv/herd-state"));
            }
            other => panic!("`report` did not parse as itself: {other:?}"),
        }
    }

    /// The retention preview's four spans: required, no default, unit
    /// compulsory (ORCH-146).
    ///
    /// The three refusals matter more than the acceptance. A span left out
    /// must refuse rather than fall back on a figure this binary chose
    /// (D194), and a span typed without its unit must refuse rather than be
    /// read as seconds — `--step 14` meaning fourteen seconds would select
    /// every record on the root, and nothing could bring them back.
    #[test]
    fn the_retention_preview_requires_a_span_per_class_with_its_unit() {
        use std::time::Duration;

        let asked = Args::parse_from([
            "herd-hub",
            "retention-preview",
            "/srv/herd-state",
            "--merger",
            "90d",
            "--step",
            "14d",
            "--nightly",
            "36h",
            "--producer",
            "0",
        ]);
        match asked.command {
            Some(Command::RetentionPreview {
                state_dir,
                merger,
                step,
                nightly,
                producer,
            }) => {
                assert_eq!(state_dir, PathBuf::from("/srv/herd-state"));
                assert_eq!(merger, Duration::from_secs(90 * 86_400));
                assert_eq!(step, Duration::from_secs(14 * 86_400));
                assert_eq!(nightly, Duration::from_secs(129_600));
                assert_eq!(producer, Duration::ZERO, "0 is never, not now");
            }
            other => panic!("`retention-preview` did not parse as itself: {other:?}"),
        }

        for missing in [
            vec!["herd-hub", "retention-preview"],
            vec![
                "herd-hub",
                "retention-preview",
                "--merger",
                "90d",
                "--step",
                "14d",
                "--nightly",
                "14d",
            ],
        ] {
            assert!(
                Args::try_parse_from(missing.clone()).is_err(),
                "a span left out must refuse, not be defaulted: {missing:?}"
            );
        }
        let no_unit = [
            "herd-hub",
            "retention-preview",
            "--merger",
            "90d",
            "--step",
            "14",
            "--nightly",
            "14d",
            "--producer",
            "14d",
        ];
        assert!(
            Args::try_parse_from(no_unit).is_err(),
            "a bare figure carries no unit and must refuse"
        );
    }
}
