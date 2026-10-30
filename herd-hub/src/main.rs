use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use herd_hub::{
    Authority, Budget, Hub, JobDirStore, JobStore, Keepers, Policy, Stickiness, Stop, StopSignals,
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
    /// Where this hub sends the four things E-37 asks to hear about: a URL
    /// or a command string. Left out, it sends nothing.
    ///
    /// **The whole of the configuration surface**, on purpose (invariant 1).
    /// There is no filter, no per-event switch, no template and no second
    /// destination: the hub ships the payload and the list, and the topic,
    /// the title and the routing are the consumer's. The events are a
    /// runner joining, a runner lost, an owner's own switch moving, and a
    /// job lost twice — never one per step job, per log line or per
    /// progress chunk, which is what E-37 asks for in so many words and
    /// what keeps a notification worth reading.
    ///
    /// `http://host[:port]/path` is one `POST` per event, the payload as
    /// the body and the title as a `Title:` header, which is what ntfy
    /// reads. Anything else is a command run by `sh -c` with the payload on
    /// its standard input — `curl -s -T - -H "Title: herd"
    /// https://ntfy.sh/our-topic` is an https sink in one line, and `https`
    /// as a URL here is refused by name because this hub carries no TLS
    /// client of its own (D14).
    ///
    /// **In the hub's own container image the command form has no shell to
    /// run in** (`nix/hub-image.nix` ships no shell on purpose), so a
    /// containerised hub uses the URL form: an ntfy in the same compose
    /// network is `http://ntfy/our-topic`, which needs neither a shell nor
    /// TLS. The command form is for a hub run as a plain binary.
    ///
    /// Nothing waits for it: a sink that hangs delays no verdict and costs
    /// this hub none of its stop budget, and an event that cannot be
    /// delivered is said out loud once, with the reason, and never retried.
    #[arg(long, env = "HERD_NOTIFY_SINK", value_name = "URL|COMMAND")]
    notify_sink: Option<String>,
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
    /// Free space to keep on the filesystem the state root sits on, below
    /// which a job's output files are removed although `--output-budget`
    /// still has room (D213, ORCH-095). For example `20GB`.
    ///
    /// **There is no default, and that is the decision.** Left out, nothing
    /// measures the volume and the output budget alone decides when
    /// something is removed — this hub's behaviour before this flag existed,
    /// byte for byte. A number chosen here by Herd would be a number applied
    /// to somebody else's disk, and the thing it does when it is wrong is
    /// delete evidence nobody asked it to delete (D194's rule, and
    /// `herd-hub report`'s).
    ///
    /// **What it bounds is the volume and not this hub.** The state root
    /// usually shares its filesystem with everything else on the host, so
    /// the figure this hub is really held to is `min(--output-budget, held +
    /// free − this floor)`, derived from what the filesystem says is free
    /// **at every trigger** and never from a reading taken at startup. When
    /// the volume is already below the floor, that ceiling is below what the
    /// hub holds and the sweeper frees exactly the shortfall.
    ///
    /// A hub that cannot measure its volume — a mount with no `statvfs`, a
    /// path that stopped resolving — keeps the behaviour of today and says
    /// so once, on `stderr`, naming the reason. It does not refuse to start:
    /// a guard that is not in force is a worse outage as a dead hub than as
    /// a loud one.
    #[arg(long, env = "HERD_VOLUME_FREE_FLOOR", value_name = "SIZE")]
    volume_free_floor: Option<String>,
    /// The runner that keeps every `leaves` key family no other runner is
    /// named for: this hub's **default keeper** (D10, M12, E-16).
    ///
    /// A keeper is asked to keep the newest five packs of each key family it
    /// keeps past the point its own `--pack-budget` would take them, so that
    /// a pack outlives the machine that built it and survives that machine
    /// being switched off.
    ///
    /// **D10 says the server's runner is the default keeper, and this is how
    /// that sentence is written down.** This hub cannot find that runner by
    /// itself: it knows a runner by the name the runner announced, and the
    /// handshake's `host` carries memory, threads and tmpfs — nothing a hub
    /// could compare with its own machine. So the deployment names it here,
    /// once, and the name must be the one that runner is approved under.
    ///
    /// **There is no default, and that is the decision** (D194's rule).
    /// Left out, this hub names no keeper, asks no runner to pin anything,
    /// and every pack on every runner is evictable by that runner's own
    /// budget alone — this hub's behaviour before this flag existed, byte
    /// for byte. A name chosen here by Herd would be a name applied to
    /// somebody else's fleet.
    ///
    /// **What it bounds is nothing.** The hub asks; the runner's own budget
    /// and gc decide what can really be kept, and a keeper that cannot come
    /// under its budget says so on the machine whose disk it is. A hub that
    /// enforced a figure against someone else's disk would be enforcing a
    /// bound its owner never agreed to.
    #[arg(long, env = "HERD_KEEPER", value_name = "RUNNER")]
    keeper: Option<String>,
    /// One `leaves` key family and the runner that keeps it, as
    /// `KEY=RUNNER`. Repeatable, and comma-separated in the environment
    /// variable.
    ///
    /// **A named family is MOVED off the default keeper, not added to it.**
    /// Naming a family here takes it out of `--keeper`'s share, so an
    /// operator whose server's disk is filling with one key family can put
    /// that family on another machine. Give the same family twice to have
    /// two runners keep it, which is D10's "one or more keepers", and name
    /// the default keeper again here to have it keep a family beside
    /// somebody else.
    ///
    /// A family named here is pinned whether or not this hub has a pack
    /// registered under it yet: it is what the operator asked for, and a
    /// keeper that had to wait for the first pack would be a keeper that
    /// missed it. `--keeper`'s own share is the other way round, because
    /// *every family nobody else keeps* has no spelling until this hub knows
    /// which families exist — it is filled from the families this hub has a
    /// pack registered under, and a family whose first pack arrives while a
    /// keeper is connected reaches it on the wire.
    #[arg(
        long = "keeper-for",
        env = "HERD_KEEPER_FOR",
        value_name = "KEY=RUNNER",
        value_delimiter = ','
    )]
    keeper_for: Vec<String>,
    /// What this binary does instead of serving. Left out, it serves, which
    /// is every existing invocation unchanged.
    #[command(subcommand)]
    command: Option<Command>,
}

/// What this binary does that is not serving.
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
    /// Print what the fleet is doing, out of the status document the serving
    /// hub publishes (E-35, E-40).
    ///
    /// The runners connected and what each one lends, what is running where,
    /// what is queued and what each queued job is waiting for, with the
    /// health ages and the hub's own version. It is **a render of
    /// `status.json` and of nothing else**: a screen with a second source
    /// would be a second answer to the same question, and the first time the
    /// two disagreed nobody would know which to believe.
    ///
    /// Safe against a running hub for the reasons `report` is, and more
    /// simply: one read-only open of one file, no lock, no write, no walk.
    /// A figure the hub does not know prints as `unknown`, never as zero.
    ///
    /// **Exit 1 when no hub has published a document in this root**, with
    /// the path named. An absent document is a question that was not
    /// answered, and a script reading exit 0 as "here is the fleet" while
    /// getting nothing is the failure this refuses to be.
    Status {
        /// The state root to read, as `report` takes it.
        #[arg(
            env = "HERD_STATE_DIR",
            default_value = DEFAULT_STATE_DIR,
            value_name = "STATE_DIR",
        )]
        state_dir: PathBuf,
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
    // Before the lock and the sweep for the same reason `report` is, and it
    // needs even less of the root than `report` does: one file, read-only
    // (ORCH-125's reasoning, E-35's surface).
    if let Some(Command::Status { state_dir }) = &args.command {
        let Some(status) = herd_hub::Status::read(state_dir)
            .with_context(|| format!("reading the state root {}", state_dir.display()))?
        else {
            anyhow::bail!(
                "no hub has published a status document in {}: there is no {} \
                 there. Either no hub has served this root since the document \
                 existed, or the hub serving it is older than the document. \
                 This is not an empty fleet and is not reported as one.",
                state_dir.display(),
                herd_hub::STATUS_FILE,
            );
        };
        print!("{status}");
        // The age, measured here and against this reader's own clock rather
        // than taken from the document, because the one mistake a reader of
        // this screen can make is to read a dead hub's last word as live.
        // The document carries the moment; this says how long ago that was,
        // and the two together are what make an operator look at the hub
        // instead of at the fleet.
        let read_at = herd_hub::Timestamp::now();
        println!(
            "read at {read_at}: this document was published {:?} earlier. \
             Nothing here is live — it is what the hub last wrote, and a hub \
             publishes only when something changes.",
            read_at.since(status.as_of),
        );
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
        // Read here for the same reason as the budget above, and the reason
        // is stronger for this one: a figure that does not parse must stop
        // the hub before the state root is opened, because opening it
        // sweeps — and this is the setting whose job is to decide what a
        // sweep takes. `None` is left alone: no flag, no figure, no reading
        // of the volume at all.
        free_floor: match &args.volume_free_floor {
            Some(size) => Some(
                herd_proto::parse_bytes(size)
                    .map_err(|why| anyhow::anyhow!("--volume-free-floor: {why}"))?,
            ),
            None => None,
        },
    };
    // Who keeps which packs (D10, M12). Read here for the reason the budget
    // above is: a `--keeper-for` that does not parse must stop this hub
    // before the state root is opened, where the message names what the
    // operator typed — and opening the root sweeps.
    let keepers = Keepers::parse(args.keeper.as_deref(), &args.keeper_for)
        .map_err(|why| anyhow::anyhow!(why))?;
    // Here for the same reason, and it is the stronger half of it: a sink
    // nobody can send to has to be found by the operator who typed it, not by
    // the absence of a notification three weeks later (E-37) — and a startup
    // that refuses it before the state root is opened has taken no `flock`
    // and swept nothing, so a mistyped scheme costs an operator a message and
    // not a sweep.
    let sink = match &args.notify_sink {
        Some(spec) => Some(herd_hub::Sink::parse(spec).map_err(|why| anyhow::anyhow!(why))?),
        None => None,
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
    // The third figure, and the one line that has to say **where it came
    // from** (ORCH-095). Three situations print three sentences, and two of
    // them carry the same number for opposite reasons: a hub with no floor
    // is bound by its budget and never claimed otherwise, and a hub that
    // cannot measure its volume is bound by its budget while an operator
    // believes a floor is protecting them. Read through the store's own
    // `volume_ceiling`, so the figure printed here is the body the sweeper
    // uses and not a second derivation of it (D79).
    let ceiling = store.volume_ceiling();
    if ceiling.derived() {
        println!(
            "herd-hub: with --volume-free-floor set, this hub may hold {} of job output \
             right now — {}. That figure is DERIVED from the volume and is re-derived at \
             every eviction trigger: it is not a setting, it moves when anything else on \
             this filesystem writes, and nothing reads it back from this line.",
            ceiling.said_bytes(),
            ceiling.said(),
        );
    } else {
        println!(
            "herd-hub: this hub may hold {} of job output, which is {}. NOT derived from \
             the volume: no free space on {} was measured to produce it.",
            ceiling.said_bytes(),
            ceiling.said(),
            args.state_dir.display(),
        );
    }
    // Custody, printed like every other setting (D148) and printed in both
    // states: the failure this line exists to prevent is an operator
    // believing a pack is kept somewhere when no runner was ever told to
    // keep it.
    println!("{}", keepers.said());
    match &sink {
        Some(sink) => println!(
            "herd-hub: events go to {} (--notify-sink). Five things are said and nothing \
             else: a runner joining, a runner lost, an owner's own switch moving, a job \
             lost twice, and the end of a job whose submitter asked for it with \
             `herdctl submit --announce-end`. Never one per step job, so a day of green \
             steps nobody asked about is a day of silence. Nothing waits for this sink: a \
             sink that hangs delays no verdict and costs this hub none of its stop budget, \
             and a delivery that fails is said here once, with the reason, and never \
             retried.",
            sink.described()
        ),
        None => println!(
            "herd-hub: no event sink is configured, so this hub tells nobody anything \
             (--notify-sink URL|COMMAND, HERD_NOTIFY_SINK). Everything it knows is still on \
             its disk and in every stream it is following."
        ),
    }
    // A timing that cannot mean what it says is refused here, before the port
    // is bound and before any runner can join (E-21, E-40).
    let hub = Hub::bind_with_sink(args.listen, Arc::new(store), policy, authority, sink)
        .await?
        // E-35: the fleet's state goes beside the records, refreshed whenever
        // it changes, so that nobody has to read a log to know what the hub
        // is doing. `herd-hub status` prints it; a renderer parses it.
        .publishing_status_to(args.state_dir.clone())
        // D10's custody, before a single connection is accepted: a welcome
        // that went out while this was still empty would tell a keeper it
        // keeps nothing, and nothing would ever correct it — the frame that
        // could is only sent when the policy *changes*.
        .naming_keepers(keepers);
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

        let asking = Args::parse_from(["herd-hub", "status", "/srv/herd-state"]);
        match asking.command {
            Some(Command::Status { state_dir }) => {
                assert_eq!(state_dir, PathBuf::from("/srv/herd-state"));
            }
            other => panic!("`status` did not parse as itself: {other:?}"),
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

    /// Every figure `herd-hub --help` states in prose, read back **through
    /// the constant that decides it**.
    ///
    /// The companion of `herd-runner`'s and `herdctl`'s tests of the same
    /// name, and the reason all three exist is one measured defect: three
    /// shipped releases described `--job-output-cap` as *"32 MiB by default,
    /// which is also the most this can be"* against a constant of 1 GiB, and
    /// the only thing that caught it was a person reading. Every figure here
    /// is interpolated from its constant rather than typed again, because a
    /// test carrying its own copy of the number is a fourth place for the
    /// number to be wrong.
    ///
    /// Whitespace is flattened first: clap wraps to a width this test has no
    /// business knowing, and a sentence that straddles a line break is still
    /// that sentence.
    ///
    /// **The one figure in this binary's help that cannot be checked from
    /// here**, and it is a finding rather than an omission:
    /// `--still-there-every` explains itself by saying `herdctl` *"reports as
    /// exit 125 on a job that succeeded"*. 125 is `herdctl::NO_VERDICT_CODE`,
    /// and `herd-hub` does not depend on `herdctl` — so that figure is
    /// asserted in `herdctl`'s own test, against its own constant, and the
    /// copy of it in this help has no gate reachable from where it is
    /// written.
    #[test]
    fn the_long_help_states_every_figure_its_constants_decide() {
        let help = flattened_long_help();

        // The claim `--still-there-every` makes about itself, asserted
        // against the two constants rather than against the sentence: "the
        // same number as `--health-every` on purpose".
        assert_eq!(
            STILL_THERE_EVERY, HEALTH_EVERY,
            "`--still-there-every`'s help says it is the same number as `--health-every` \
             on purpose; if they ever diverge, that sentence is what has to change"
        );
        for (span, unit) in [
            (DRAIN_FIRST_LIMIT, 60),
            (READING_CAP, 60),
            (OUTPUT_FLOOR, 24 * 60 * 60),
        ] {
            assert_eq!(
                span.as_secs() % unit,
                0,
                "the help spells this span in whole {unit}-second units and {span:?} is no \
                 longer a whole number of them: the sentence needs reading, not this test"
            );
        }

        for (figure, flag) in [
            (
                format!("at least every {} s", WAIT_RESTATEMENT.as_secs()),
                "--wait-restatement's default, which E-04 asks for",
            ),
            (
                format!(
                    "{} seconds, the same number as",
                    a_number_in_words(STILL_THERE_EVERY.as_secs())
                ),
                "--still-there-every's cadence",
            ),
            (
                format!(
                    "{} minutes, the figure E-27 names",
                    a_number_in_words(DRAIN_FIRST_LIMIT.as_secs() / 60)
                ),
                "--drain-first-limit's default",
            ),
            (
                format!(
                    "{} minutes by default",
                    a_number_in_words(READING_CAP.as_secs() / 60)
                ),
                "--reading-cap's default",
            ),
            (
                format!("{} by default", a_number_in_words(READINGS_AT_ONCE.into())),
                "--readings-at-once' default",
            ),
            (
                format!("Nothing computed {}", herd_proto::show_bytes(OUTPUT_BUDGET)),
                "--output-budget's chosen figure",
            ),
            (
                format!(
                    "{} days",
                    a_number_in_words(OUTPUT_FLOOR.as_secs() / (24 * 60 * 60))
                ),
                "--output-floor's promise",
            ),
            (
                format!(
                    "newest {} packs",
                    // Fully qualified, and that is not a style choice: the
                    // constant is read by this test alone, so an import at
                    // the top of the file would be an unused one in the
                    // binary's own build and `-D warnings` would refuse it.
                    a_number_in_words(herd_hub::KEEPER_KEEP_PER_KEY.into())
                ),
                "--keeper's custody figure",
            ),
        ] {
            assert!(
                help.to_lowercase().contains(&figure.to_lowercase()),
                "`herd-hub --help` must state {flag} as its constant decides it, and the \
                 rendered help does not contain {figure:?}.\n\
                 \n\
                 **The constant is the authority and the help text is what is wrong.** Do \
                 not change the constant to match the sentence, and do not change this \
                 test: fix the doc comment on the flag.\n\
                 \n\
                 --- the rendered long help, with its whitespace flattened:\n{help}"
            );
        }
    }

    /// **The keeper policy as the command line gives it** (D10, M12, E-16).
    ///
    /// The refusals matter more than the acceptance, and for the reason the
    /// retention spans' do: every one of these mistakes is otherwise
    /// *silent*. A `--keeper-for` without its `=` would name a key family
    /// nothing is ever filed under, and the operator would read a startup
    /// line that looked exactly right while nothing was pinned anywhere.
    ///
    /// The last case is the one that has to hold for ever: a hub given
    /// neither flag names nobody, which is this binary's behaviour before
    /// the flags existed.
    #[test]
    fn the_keeper_policy_is_read_off_the_command_line_or_refused_by_name() {
        let named = Args::parse_from([
            "herd-hub",
            "--keeper",
            "server-runner",
            "--keeper-for",
            "reference=pc-runner",
        ]);
        assert_eq!(named.keeper.as_deref(), Some("server-runner"));
        assert_eq!(named.keeper_for, vec!["reference=pc-runner".to_owned()]);
        let keepers = Keepers::parse(named.keeper.as_deref(), &named.keeper_for)
            .expect("a default keeper and one named family parse");
        assert!(
            !keepers.names_nobody(),
            "a hub given both flags names somebody"
        );
        let said = keepers.said();
        for owed in [
            "server-runner is the default keeper",
            "reference is kept by pc-runner",
            "MOVED off the default keeper",
        ] {
            assert!(
                said.contains(owed),
                "the startup line has to say {owed:?}, and it says:\n{said}"
            );
        }

        for (typed, wrong) in [
            (vec!["reference".to_owned()], "has no `=` in it"),
            (vec!["=pc-runner".to_owned()], "leaves the key family empty"),
            (
                vec!["reference=".to_owned()],
                "leaves the runner's name empty",
            ),
        ] {
            match Keepers::parse(None, &typed) {
                Err(why) => assert!(
                    why.contains(wrong),
                    "{typed:?} must be refused with {wrong:?}, and it said: {why}"
                ),
                Ok(_) => panic!("{typed:?} is not a KEY=RUNNER and must be refused"),
            }
        }
        assert!(
            Keepers::parse(Some("  "), &[]).is_err(),
            "--keeper with an empty name is a mistake and not a hub that names nobody"
        );

        let silent = Args::parse_from(["herd-hub"]);
        assert!(silent.keeper.is_none() && silent.keeper_for.is_empty());
        assert!(
            Keepers::parse(silent.keeper.as_deref(), &silent.keeper_for)
                .expect("neither flag parses")
                .names_nobody(),
            "a hub given neither flag names NO keeper: no welcome carries keeper_for and no \
             PinPacks is ever sent, which is this binary's behaviour before the flags existed"
        );
    }

    /// The long help, rendered from the real `Args` and flattened to one
    /// line, because clap's wrap width is not this test's business.
    fn flattened_long_help() -> String {
        use clap::CommandFactory;
        Args::command()
            .render_long_help()
            .to_string()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A small number as `--help` spells it, derived from the constant rather
    /// than typed beside it.
    ///
    /// The panic is the useful half: a number nobody has a word for here is
    /// one somebody has just changed, and the sentence that spells it out
    /// needs reading before this test can be made to pass.
    fn a_number_in_words(n: u64) -> String {
        match n {
            1 => "one".to_owned(),
            2 => "two".to_owned(),
            3 => "three".to_owned(),
            4 => "four".to_owned(),
            5 => "five".to_owned(),
            6 => "six".to_owned(),
            7 => "seven".to_owned(),
            8 => "eight".to_owned(),
            9 => "nine".to_owned(),
            10 => "ten".to_owned(),
            15 => "fifteen".to_owned(),
            30 => "thirty".to_owned(),
            60 => "sixty".to_owned(),
            _ => panic!(
                "`--help` spells this figure out in words and this test has no word for \
                 {n}. Somebody has changed the constant: read the sentence that spells it, \
                 then either add the word here or write the figure in the help"
            ),
        }
    }
}
