//! The five things this hub says out loud to somebody who is not watching
//! (E-37).
//!
//! Everything else the hub knows reaches a person because that person is
//! holding a stream (E-04), reading a record (E-53) or asking `herdctl`. This
//! module is for the other case: the owner is asleep, the orchestrator has
//! gone, and something happened that nobody is going to find out about until
//! they look. E-37 names exactly what those things are and nothing else:
//!
//! > Events we can route to ntfy (our topic, our titles): a detached job's
//! > end when the submitter asked, the nightly's verdict, a client lost or
//! > joined, an owner switch changed from the owner's side, a job lost twice.
//! > Never per step job by default.
//!
//! with the acceptance criterion *"a day's ntfy count does not grow with the
//! number of step jobs"*. Four of the five have a state transition in this
//! hub to hang off and are [`Event`]'s first four variants; the fifth — a
//! detached job's end — is [`Event::JobEnded`], which waited for the
//! submitter's asking to be carried on the submission and written on the
//! record, and has it now. What is left of E-37 is **the nightly's verdict,
//! and it needs no event of its own**: see [`Event`]'s own documentation. The
//! list is the whole of it: an event per step, per log line or per progress
//! chunk is the failure this is designed against, because the consumer's
//! answer to a noisy topic is to mute it, and then the one event that
//! mattered is lost with the rest.
//!
//! **It is not a notification system, and the shape is what keeps it from
//! becoming one** (invariant 1). There is one setting — a URL or a command
//! string — no filter language, no per-event switch, no template, no
//! subscription, no second destination. The hub ships the payload and the
//! list; the destination, the topic and the title a person reads are the
//! consumer's, which is what they asked for ("we adapt our dashboard and our
//! ntfy calls to them").
//!
//! ## The three properties this file is written around
//!
//! 1. **A sink that hangs never delays a verdict.** [`Notifier::say`] is not
//!    `async` and never blocks: it is a `try_send` into a bounded queue and a
//!    counter. Nothing on the path from a runner's verdict to the record and
//!    to the requester can await a notification, because the sink is a URL on
//!    somebody else's machine and a job's result is not theirs to hold up.
//! 2. **A sink that hangs never costs the hub its stop budget.** The one task
//!    that touches the sink is spawned into the hub's `Housekeeping`, which
//!    *aborts* its tasks and never joins them, so a delivery in flight is
//!    dropped where it stands when the hub stops. A stop stays bounded by
//!    [`crate::STOP_WITHIN`] and the notifier is not in that path at all.
//! 3. **Zero events per step.** No variant fires on a job that runs and
//!    finishes, whatever it printed, **unless its submitter asked for that
//!    one job's end** — which is a thing a person typed and not a thing the
//!    fleet does. A day of step jobs that work produces no event whatever,
//!    and [`Event::JobEnded`]'s own doc has why that is still true now that
//!    an end *can* be an event.
//!
//! And a fourth, which is about the operator rather than the hub: **a sink
//! that fails is said out loud, with the reason, and never retried for ever
//! in silence.** An operator who configured a broken webhook learns it from
//! this hub's stderr, not from the absence of messages. One attempt per
//! event, never a retry: a notification is news, and news redelivered at an
//! unknown delay is worse than news that did not arrive.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::job::Timestamp;

/// What sits in front of every payload this hub sends.
///
/// The same reasoning as `RECORD_VERSION` and the status document's version
/// (Q7): the consumer parses these with their own code, and a reader that
/// cannot tell which shape it was handed has to guess. It is **this format's**
/// number and is not the protocol's, the record's or the hub's: nothing on
/// the wire between hub and runner carries an event.
pub const EVENT_FORMAT_VERSION: u32 = 1;

/// How many events may be waiting for a sink that is not keeping up.
///
/// Bounded, because the alternative is a queue that grows while a webhook is
/// down and a hub that runs out of memory because of a notification. Deep
/// enough that an ordinary burst — a fleet of runners reconnecting after a
/// network blip — is not lost, shallow enough that it is a few kilobytes.
/// Past it an event is **dropped and said out loud**, never queued and never
/// waited for.
pub const EVENTS_QUEUED: usize = 64;

/// The longest one delivery may take before the hub gives up on it.
///
/// It bounds the queue's progress, not the hub's: nothing is waiting on this.
/// It exists because a sink that accepts a connection and then says nothing
/// for ever is the common shape of a broken webhook, and without a cap the
/// first such event would be the last one this hub ever delivered.
pub const DELIVER_WITHIN: Duration = Duration::from_secs(10);

/// One of the things E-37 asks to hear about.
///
/// **Five variants for five listed things, and the fifth of E-37's list has
/// no variant of its own because it needs none:**
///
/// * *"a detached job's end when the submitter asked"* is
///   [`Event::JobEnded`]. It took a field carried on the submission
///   (`ToHub::SubmitJob`'s `announce_end`), through `NewJob`, onto
///   `job.json` ([`crate::job::JobRecord::announce_end`]) — because an
///   announcement at the end has to be asked for at the beginning, and
///   nothing on the connection survives a `herdctl submit` that has already
///   left.
/// * *"the nightly's verdict"* is **that same event and not a new one**, and
///   what it waits for is M14's nightly, not more work here. A scheduled
///   pass is `herdctl if-moved` submitting on the consumer's own timer
///   (D207, D217): a submission nobody is following, which asks for its end
///   to be announced exactly as any other detached submission does. The
///   verdict then arrives as a `job_ended` whose `cause` and `how` are the
///   nightly's. The one deviation D217 already records stays a deviation: a
///   pass that submitted *nothing* is `herdctl`'s own stderr line, because
///   the hub never heard of a job and an event about a job that does not
///   exist is not a thing this hub can say.
///
/// Neither was invented ahead of its source: an event with no transition
/// behind it is a shape nobody has measured, and shipping one would make the
/// list longer than E-37's, which is the one thing this module must not do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// A machine joined the fleet: it finished its handshake and this hub can
    /// place work on it (E-37's *"a client ... joined"*).
    RunnerJoined {
        runner: String,
        /// Which build of the runner it is, as its handshake said (E-51).
        version: String,
    },
    /// A machine left the fleet, by its link closing or by going quiet past
    /// the hub's silence threshold (E-37's *"a client lost"*, E-21).
    RunnerLost { runner: String, why: String },
    /// A machine's owner moved her own switch, on her own machine (E-37's
    /// *"an owner switch changed from the owner's side"*, E-33).
    ///
    /// **From the owner's side** is the whole point of this one, and telling
    /// the two hands apart took more than reading which frame the position
    /// arrived on. A position the hub asked for comes back both as
    /// `ToHub::SwitchApplied` and on the next health report, on two clocks,
    /// and either can be first: with a 0.2 s cadence the report won 60 ms
    /// after a join, so "it came on a health report" attributed an agent's
    /// own `herdctl switch` to the machine's owner. The hub therefore
    /// remembers the position it asked for and forgets it the moment that
    /// position comes back — `ConnectedRunner::asked_for` — and this event is
    /// what is left: a position nobody here asked for.
    ///
    /// The one case it still cannot tell apart is a position the hub asked
    /// for which the runner never acknowledged *and* which it then reports
    /// twice — her own hand setting the same position again. That reads as
    /// hers, which is the right way round: the machine really did change
    /// position, and the consumer would rather hear it attributed to the
    /// wrong hand than not hear it at all.
    OwnerSwitch {
        runner: String,
        /// `on`, `drain` or `off`.
        switch: String,
        /// The owner's own reason, which E-33 requires for `off` and
        /// `drain` — *"she is gaming"* rather than only *"off"*.
        why: String,
    },
    /// A job is over without a verdict because the machine under it went away
    /// twice (E-37's *"a job lost twice"*, E-21's second loss).
    ///
    /// Only that ending. A job that ends non-zero has a verdict and a
    /// requester to read it; a job refused as full is D182's different
    /// ending, where nothing failed and no machine went anywhere.
    JobLost { job: String, reason: String },
    /// A job whose submitter **asked to be told** has reached its end
    /// (E-37's *"a detached job's end when the submitter asked"*).
    ///
    /// **The asking is the whole gate, and it is the only gate.** The hub
    /// does not also check that nobody happens to be following the job: a
    /// follower list is a fact about this second, and the question E-37 asks
    /// is about what the submitter wanted when they submitted. A `herdctl
    /// submit --announce-end` is detached by construction — it leaves with
    /// the id and closes — and `herdctl run`, which stays and reads the
    /// verdict on its own stream, is offered no such flag at all. So the
    /// shape of the CLI is what makes "detached" true, and the record's one
    /// `announce_end` is what makes it *asked*.
    ///
    /// **Every ending, not only a green one.** A detached submitter who
    /// asked to hear how their job went hears it whether it exited 0, exited
    /// 3, was cancelled at its deadline, was stopped at a cap or could never
    /// have run: *"a detached job's end"* is the end, and an announcement
    /// that only fired on success would be the one that let a red nightly
    /// pass unnoticed. A job lost twice is the one ending that can produce
    /// two events, [`Event::JobLost`] and this one, and that is correct: the
    /// first is about a *machine* going and is addressed to the fleet's
    /// operator, the second is about a *job* ending and is addressed to
    /// whoever asked. Each still fires at most once.
    ///
    /// It does **not** make a step job noisy, and the acceptance criterion
    /// is unaffected: a day's count grows with the number of *submitters who
    /// asked*, which is a number a person chose, and not with the number of
    /// step jobs, which is a number the fleet produces.
    JobEnded {
        job: String,
        /// The one machine-readable word for what ended it, which is
        /// [`herd_proto::JobOutcome::cause`] and therefore the same word the
        /// verdict document and the wire use. A consumer's router switches
        /// on this.
        cause: String,
        /// The same ending as a person reads it, in the hub's own words —
        /// `exited 3 rather than 0`, `was cancelled: …`. One sentence, so
        /// that the body of the notification needs no second lookup.
        how: String,
    },
}

impl Event {
    /// A machine joined (E-37).
    pub fn runner_joined(runner: &str, version: &str) -> Self {
        Self::RunnerJoined {
            runner: ascii(runner),
            version: ascii(version),
        }
    }

    /// A machine went (E-37).
    pub fn runner_lost(runner: &str, why: &str) -> Self {
        Self::RunnerLost {
            runner: ascii(runner),
            why: ascii(why),
        }
    }

    /// An owner moved her switch on her own machine (E-37, E-33).
    pub fn owner_switch(runner: &str, switch: &str, why: &str) -> Self {
        Self::OwnerSwitch {
            runner: ascii(runner),
            switch: ascii(switch),
            why: ascii(why),
        }
    }

    /// A job was lost twice (E-37, E-21).
    pub fn job_lost(job: &str, reason: &str) -> Self {
        Self::JobLost {
            job: ascii(job),
            reason: ascii(reason),
        }
    }

    /// A job ended and its submitter had asked to hear about it (E-37).
    pub fn job_ended(job: &str, cause: &str, how: &str) -> Self {
        Self::JobEnded {
            job: ascii(job),
            cause: ascii(cause),
            how: ascii(how),
        }
    }

    /// The short machine-readable name of what happened. The same string the
    /// payload's `kind` carries, and what a consumer's router switches on.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::RunnerJoined { .. } => "runner_joined",
            Self::RunnerLost { .. } => "runner_lost",
            Self::OwnerSwitch { .. } => "owner_switch",
            Self::JobLost { .. } => "job_lost",
            Self::JobEnded { .. } => "job_ended",
        }
    }

    /// One line, ASCII, no colon-separated structure to parse: the consumer
    /// titles their own notifications and this is the fallback.
    pub fn title(&self) -> String {
        match self {
            Self::RunnerJoined { runner, .. } => format!("herd: {runner} joined"),
            Self::RunnerLost { runner, .. } => format!("herd: {runner} lost"),
            Self::OwnerSwitch { runner, switch, .. } => {
                format!("herd: {runner} switched {switch}")
            }
            Self::JobLost { job, .. } => format!("herd: {job} lost"),
            Self::JobEnded { job, cause, .. } => format!("herd: {job} {cause}"),
        }
    }

    /// The body a person reads, in the words this hub already uses on its own
    /// stderr. ASCII, because the consumer's topic takes ASCII bodies.
    pub fn body(&self) -> String {
        match self {
            Self::RunnerJoined { runner, version } => {
                format!("runner {runner} has joined this hub (build {version})")
            }
            Self::RunnerLost { runner, why } => format!("runner {runner} is gone: {why}"),
            Self::OwnerSwitch {
                runner,
                switch,
                why,
            } => format!("runner {runner} is now switched {switch} ({why})"),
            Self::JobLost { job, reason } => format!("{job} is lost: {reason}"),
            Self::JobEnded { job, how, .. } => format!("{job} {how}"),
        }
    }

    /// The whole payload, one line of JSON, as it goes to the sink.
    fn payload(&self) -> String {
        let wire = Wire {
            herd_event_version: EVENT_FORMAT_VERSION,
            at: Timestamp::now(),
            title: one_line(&self.title()),
            body: self.body(),
            event: self,
        };
        // Infallible in practice — every field is a string, a number or a
        // timestamp — and a notification is not worth a panic if it ever is
        // not: the sink gets a payload that says the serialisation failed,
        // and the operator reads which event it was.
        serde_json::to_string(&wire).unwrap_or_else(|error| {
            format!(
                "{{\"herd_event_version\":{EVENT_FORMAT_VERSION},\"kind\":\"{}\",\
                 \"body\":\"this event could not be serialised: {error}\"}}",
                self.kind()
            )
        })
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.body())
    }
}

/// The payload's own shape: the envelope, then the event's fields flattened
/// into it so that `kind` and the fields sit at one level for a reader.
#[derive(Serialize)]
struct Wire<'a> {
    herd_event_version: u32,
    at: Timestamp,
    title: String,
    body: String,
    #[serde(flatten)]
    event: &'a Event,
}

/// Where the events go. The whole of E-37's configuration surface.
///
/// A URL or a command string, and nothing else — no headers, no method, no
/// body template, no second sink, no per-event destination. Each of those is
/// how a specialized tool becomes a generic one (invariant 1), and the
/// command form is the escape hatch that makes every one of them
/// unnecessary: whatever the operator needs doing to a notification, their
/// own one-line command does it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sink {
    /// `http://host[:port]/path`: one `POST` per event, the payload as the
    /// body, the title as a `Title:` header, which is what ntfy reads.
    ///
    /// **Plain `http` only, and `https` is refused by name.** The hub speaks
    /// no TLS: it listens in plain `ws://` behind the consumer's reverse
    /// proxy (D14) and has no client TLS stack in it. A hub that pretended to
    /// take an `https` URL would either need one — a new dependency and a new
    /// decision — or quietly fall back to plaintext, and a notification
    /// leaving a machine in the clear because of a silent fallback is not a
    /// thing this hub should do. An `https` destination is one `curl` in the
    /// command form.
    Url(String),
    /// A command string, run by `sh -c` with the payload on its standard
    /// input. `curl -T - -H "Title: herd" https://ntfy.sh/our-topic` is the
    /// whole of an https sink.
    ///
    /// The event's own text never reaches the command *string*: it goes on
    /// stdin, so there is nothing an event's reason could contain that would
    /// change what command runs.
    ///
    /// **It needs a shell, and the hub's own container image has none.**
    /// `nix/hub-image.nix` ships the hub, a C library and two text files:
    /// no shell, no package manager, no CA bundle, on the stated grounds
    /// that the hub execs nothing. So in the deployment as it stands this
    /// form fails — loudly, once, with `sh` not found as the reason — and
    /// the [`Sink::Url`] form is the one that works there: an ntfy in the
    /// consumer's own compose network is `http://ntfy/our-topic` and needs
    /// neither a shell nor TLS. A hub run as a native binary, which is how
    /// every test here runs it, has whatever shell its host has.
    Command(String),
}

impl Sink {
    /// Read the one setting an operator gave, or say why it cannot be served.
    ///
    /// Refused at startup and never at the first event: a sink nobody can use
    /// has to be found by the operator who typed it, not by the absence of a
    /// notification three weeks later.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err(
                "an empty --notify-sink is neither a URL nor a command; leave the option out \
                 to send no event at all"
                    .to_owned(),
            );
        }
        if let Some(rest) = spec.strip_prefix("http://") {
            Target::of(rest).map(|_| Self::Url(spec.to_owned()))
        } else if let Some(scheme) = unsupported_scheme(spec) {
            Err(format!(
                "--notify-sink {spec} names the scheme {scheme}://, which this hub does not \
                 send to. Plain http:// is served directly; everything else goes through the \
                 command form, which is one line: --notify-sink 'curl -s -T - -H \"Title: \
                 herd\" {spec}'. The hub carries no TLS client of its own (D14), and a \
                 notification that left this machine in the clear because the hub quietly \
                 ignored your scheme would be worse than this refusal."
            ))
        } else {
            Ok(Self::Command(spec.to_owned()))
        }
    }

    /// What the hub prints at startup, so that an operator reading the log
    /// knows which of the two forms their setting was taken as.
    pub fn described(&self) -> String {
        match self {
            Self::Url(url) => format!("one POST per event to {url}"),
            Self::Command(command) => {
                format!("one run of `sh -c {command}` per event, payload on stdin")
            }
        }
    }
}

/// Where an `http://` sink's bytes go, read out of the URL once.
#[derive(Debug, PartialEq, Eq)]
struct Target {
    host: String,
    port: u16,
    path: String,
}

impl Target {
    /// The host, port and path of what followed `http://`.
    ///
    /// Hand-read rather than taken from a URL library, because this is the
    /// whole of what the hub needs of a URL and the alternative is a
    /// dependency in the hub for one `POST`.
    fn of(rest: &str) -> Result<Self, String> {
        let (authority, path) = match rest.find('/') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        if authority.is_empty() {
            return Err("--notify-sink http:// names no host".to_owned());
        }
        if !authority.is_ascii() || authority.contains(['@', '[', ']', ' ']) {
            return Err(format!(
                "--notify-sink http://{authority}... is not a plain host[:port]; this hub \
                 sends to a host and a port, and anything richer belongs in the command form"
            ));
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (
                host,
                port.parse::<u16>().map_err(|_| {
                    format!("--notify-sink http://{authority}...: {port} is not a port")
                })?,
            ),
            None => (authority, 80),
        };
        if host.is_empty() {
            return Err("--notify-sink http:// names no host".to_owned());
        }
        Ok(Self {
            host: host.to_owned(),
            port,
            path: path.to_owned(),
        })
    }
}

/// The hub's end of the sink: what every call site holds.
///
/// Cheap to clone, cheap to hold, and **cheap to not have**: a hub with no
/// sink configured carries a `None` and every [`Notifier::say`] is a match on
/// it. That is the default, and it is what every test that is about something
/// else runs with.
#[derive(Debug, Clone, Default)]
pub struct Notifier {
    to: Option<mpsc::Sender<Event>>,
    /// Events dropped because the queue was full, counted here so that the
    /// delivering task can say how many and the call site never has to care.
    dropped: Arc<AtomicU64>,
}

impl Notifier {
    /// A hub that tells nobody anything: no sink configured.
    pub fn silent() -> Self {
        Self::default()
    }

    /// A hub that sends its events to this sink, with the task that does the
    /// sending.
    ///
    /// Two halves on purpose: the [`Notifier`] goes into the hub's state,
    /// where every call site can reach it, and the [`Deliveries`] goes into
    /// the hub's `Housekeeping`, which aborts it. Nothing joins it, which is
    /// property 2 — a blocked sink cannot extend a stop — and the type
    /// system is what keeps it that way.
    pub fn to(sink: Sink) -> (Self, Deliveries) {
        let (to, events) = mpsc::channel(EVENTS_QUEUED);
        let dropped = Arc::new(AtomicU64::new(0));
        (
            Self {
                to: Some(to),
                dropped: Arc::clone(&dropped),
            },
            Deliveries {
                sink,
                events,
                dropped,
            },
        )
    }

    /// Whether anything is configured at all.
    pub fn configured(&self) -> bool {
        self.to.is_some()
    }

    /// Say that this happened, and **return at once**.
    ///
    /// Not `async`, nothing to await, no lock held past this line: the whole
    /// of property 1 is that a verdict's path cannot wait on a notification,
    /// and the way to guarantee that is a function a verdict's path *cannot*
    /// wait on. A full queue drops the event and says so once; a delivery
    /// task that has gone drops it silently, because the only way that
    /// happens is a hub that is stopping.
    pub fn say(&self, event: Event) {
        let Some(to) = self.to.as_ref() else {
            return;
        };
        match to.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(event)) => {
                let before = self.dropped.fetch_add(1, Ordering::Relaxed);
                if before == 0 {
                    eprintln!(
                        "herd-hub: the event sink is not keeping up, so this event was \
                         dropped rather than queued: {event}. Nothing else is delayed by \
                         it (E-37), and the count of what was dropped is said when the \
                         sink answers again."
                    );
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {}
        }
    }
}

/// The one task that touches the sink.
///
/// It owns the receiving end and the sink, and it is the only thing in this
/// hub that waits for a webhook. It is spawned into `Housekeeping` and
/// aborted with it: a delivery in flight when the hub stops is dropped where
/// it stands, which is the right answer — a notification about a hub that is
/// going down is not worth a second of a bounded stop.
pub struct Deliveries {
    sink: Sink,
    events: mpsc::Receiver<Event>,
    dropped: Arc<AtomicU64>,
}

impl Deliveries {
    /// Deliver what comes, one event at a time, until the hub goes.
    ///
    /// One at a time rather than one task per event: a sink that is slow
    /// should not be hit by sixty-four connections at once, and the order a
    /// person reads their notifications in is worth keeping. The queue in
    /// front is what absorbs the burst, and its bound is what stops a slow
    /// sink from growing without end.
    pub async fn run(mut self) {
        let mut failures: u64 = 0;
        let mut said = 0u64;
        while let Some(event) = self.events.recv().await {
            // What was lost while the sink was busy, said with the event
            // that got through rather than from the call site that dropped
            // it: the call site must not do any of this work.
            let dropped = self.dropped.swap(0, Ordering::Relaxed);
            if dropped > 0 {
                eprintln!(
                    "herd-hub: {dropped} event(s) were dropped while the sink was not \
                     keeping up; the queue holds {EVENTS_QUEUED}"
                );
            }
            // One attempt, bounded, and never a second one at the same
            // event: a notification is news, and news redelivered at an
            // unknown delay is worse than news that did not arrive.
            let went = match tokio::time::timeout(DELIVER_WITHIN, deliver(&self.sink, &event)).await
            {
                Ok(Ok(())) => None,
                Ok(Err(why)) => Some(why),
                Err(_) => Some(format!("it did not answer within {DELIVER_WITHIN:?}")),
            };
            match went {
                None => {
                    if failures > 0 {
                        eprintln!(
                            "herd-hub: the event sink answered again, after {failures} \
                             failure(s)"
                        );
                        failures = 0;
                    }
                    said += 1;
                }
                Some(why) => {
                    failures += 1;
                    // **Said out loud, and not once and then for ever in
                    // silence.** The first failure is the one an operator
                    // who has just typed a broken webhook needs; saying
                    // every one of them would fill a journal while a sink is
                    // down for a day; saying none after the first would
                    // leave a sink that has been dead for a week looking
                    // exactly like a fleet where nothing has happened. So:
                    // the first, and then every tenth.
                    if failures == 1 || failures.is_multiple_of(10) {
                        eprintln!(
                            "herd-hub: the event sink failed and this event is not retried: \
                             {why}. The event was: {event}. That is failure {failures} since \
                             the sink last answered, {said} event(s) having been delivered \
                             before it; the next is said at every tenth."
                        );
                    }
                }
            }
        }
    }
}

/// One attempt at one event. The error is what the operator reads.
async fn deliver(sink: &Sink, event: &Event) -> Result<(), String> {
    let payload = event.payload();
    match sink {
        Sink::Url(url) => post(url, event, &payload).await,
        Sink::Command(command) => run(command, &payload).await,
    }
}

/// `POST` the payload, read the status line, and judge it.
///
/// A hand-written HTTP/1.1 request because that is cheaper than an HTTP
/// client in the hub: one connection, one request, `Connection: close`, no
/// keep-alive, no redirect, no retry. A sink that needs any of those is a
/// sink for the command form.
async fn post(url: &str, event: &Event, payload: &str) -> Result<(), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("{url} is not an http:// URL"))?;
    let target = Target::of(rest)?;
    let mut link = tokio::net::TcpStream::connect((target.host.as_str(), target.port))
        .await
        .map_err(|error| format!("connecting to {}:{}: {error}", target.host, target.port))?;
    // The title goes in a header, so it is folded to one line first. Nothing
    // in an event's text may be able to end a header and start another one:
    // a runner's name and an owner's reason both reach this function, and
    // both come from outside the hub.
    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}:{}\r\nUser-Agent: herd-hub/{}\r\nContent-Type: \
         application/json\r\nContent-Length: {}\r\nTitle: {}\r\nX-Herd-Event: {}\r\nConnection: \
         close\r\n\r\n{}",
        target.path,
        target.host,
        target.port,
        crate::server::hub_version(),
        payload.len(),
        one_line(&event.title()),
        event.kind(),
        payload,
    );
    link.write_all(request.as_bytes())
        .await
        .map_err(|error| format!("writing to {url}: {error}"))?;
    // The answer matters, because a 404 on a mistyped topic is exactly the
    // mistake this has to surface. Read a little and judge the status line;
    // the body is the sink's and is not this hub's business.
    let mut answer = Vec::new();
    let mut head = [0u8; 512];
    loop {
        let read = link
            .read(&mut head)
            .await
            .map_err(|error| format!("reading {url}'s answer: {error}"))?;
        if read == 0 {
            break;
        }
        answer.extend_from_slice(&head[..read]);
        if answer.len() >= 512 || answer.windows(2).any(|pair| pair == b"\r\n") {
            break;
        }
    }
    let status = String::from_utf8_lossy(&answer);
    let status = status.lines().next().unwrap_or("").trim().to_owned();
    if status.is_empty() {
        return Err(format!("{url} answered nothing at all"));
    }
    let code = status.split_whitespace().nth(1).unwrap_or("");
    if code.starts_with('2') {
        Ok(())
    } else {
        Err(format!("{url} answered {status}"))
    }
}

/// Run the command with the payload on its stdin, and judge its exit code.
async fn run(command: &str, payload: &str) -> Result<(), String> {
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // So that a command the hub gave up on — the timeout above, or the
        // hub stopping — is reaped rather than left running on the
        // operator's host.
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("starting `sh -c {command}`: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A sink that does not read its stdin is not an error: `true` is a
        // legitimate sink, and a broken pipe here says nothing about whether
        // the command did its job. Its exit code says that.
        // With a newline after it, so that a sink as plain as `cat >> file`
        // leaves one JSON object per line. The payload itself carries none —
        // an HTTP body's length is counted from it — so this is the one place
        // the line ending is added.
        let _ = stdin.write_all(payload.as_bytes()).await;
        let _ = stdin.write_all(b"\n").await;
        let _ = stdin.shutdown().await;
    }
    let done = child
        .wait_with_output()
        .await
        .map_err(|error| format!("waiting for `sh -c {command}`: {error}"))?;
    if done.status.success() {
        return Ok(());
    }
    // The command's own words, because that is the reason the operator needs
    // and the hub has no better one. Bounded, because a sink that prints a
    // megabyte on failure must not put a megabyte in the journal.
    let mut why = String::from_utf8_lossy(&done.stderr).trim().to_owned();
    if why.is_empty() {
        why = String::from_utf8_lossy(&done.stdout).trim().to_owned();
    }
    let why = one_line(&ascii(&why));
    let why = if why.len() > 400 {
        format!("{}...", &why[..400])
    } else {
        why
    };
    Err(format!(
        "`sh -c {command}` ended {}{}",
        done.status,
        if why.is_empty() {
            String::new()
        } else {
            format!(": {why}")
        }
    ))
}

/// The scheme this setting opens with, when it opens with one this hub does
/// not send to.
///
/// **Only when the setting itself is a URL**, which is the whole subtlety
/// here: `curl -s -T - http://127.0.0.1/t` contains a `://` and is a command,
/// and reading it as a mistyped URL would refuse the one form that makes
/// every other scheme reachable. So a scheme is a scheme only where a scheme
/// can be — at the front, with nothing but a scheme's own characters before
/// the `://`.
fn unsupported_scheme(spec: &str) -> Option<&str> {
    let (scheme, _) = spec.split_once("://")?;
    let mut characters = scheme.chars();
    let first = characters.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'))
        .then_some(scheme)
}

/// The same text with every character a header or a notification body cannot
/// carry turned into a space, and no run of them repeated.
///
/// It is not tidiness: a title goes into an HTTP header, and a newline in a
/// header is a second header. A runner's name and an owner's switch reason
/// both come from outside this hub.
fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for character in text.chars() {
        if character.is_whitespace() || character.is_control() {
            if !out.is_empty() && !space {
                out.push(' ');
                space = true;
            }
        } else {
            out.push(character);
            space = false;
        }
    }
    out.trim_end().to_owned()
}

/// The same text in ASCII, because the consumer's topic takes ASCII bodies.
///
/// The hub's own sentences carry an em dash and a `≤` here and there, and the
/// two that read as something else are translated rather than lost; anything
/// else becomes a `?`, which is honest about there having been a character
/// there.
fn ascii(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '—' | '–' => out.push('-'),
            '≤' => out.push_str("<="),
            '≥' => out.push_str(">="),
            '“' | '”' | '„' => out.push('"'),
            '‘' | '’' => out.push('\''),
            '…' => out.push_str("..."),
            '\u{a0}' => out.push(' '),
            character if character.is_ascii() => out.push(character),
            _ => out.push('?'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use tokio::io::AsyncBufReadExt;

    #[test]
    fn the_sink_is_a_url_or_a_command_and_nothing_else() {
        assert_eq!(
            Sink::parse("http://127.0.0.1:8080/herd").expect("a plain http URL is served"),
            Sink::Url("http://127.0.0.1:8080/herd".to_owned())
        );
        assert_eq!(
            Sink::parse("  curl -s -T - http://127.0.0.1/t  ").expect("a command is a command"),
            Sink::Command("curl -s -T - http://127.0.0.1/t".to_owned())
        );
        let https = Sink::parse("https://ntfy.sh/our-topic").expect_err("https is refused");
        assert!(
            https.contains("curl") && https.contains("https://ntfy.sh/our-topic"),
            "the refusal has to carry the one-line alternative: {https}"
        );
        assert!(Sink::parse("").is_err(), "an empty sink is not a sink");
        assert!(
            Sink::parse("http://").is_err(),
            "an http URL with no host is not a sink"
        );
        assert!(
            Sink::parse("http://host:not-a-port/x").is_err(),
            "a port that is not a number is not a port"
        );
    }

    #[test]
    fn an_http_target_is_read_out_of_the_url() {
        assert_eq!(
            Target::of("127.0.0.1:8080/herd/topic").expect("host, port and path"),
            Target {
                host: "127.0.0.1".to_owned(),
                port: 8080,
                path: "/herd/topic".to_owned(),
            }
        );
        assert_eq!(
            Target::of("ntfy.example").expect("a bare host is port 80 and /"),
            Target {
                host: "ntfy.example".to_owned(),
                port: 80,
                path: "/".to_owned(),
            }
        );
    }

    #[test]
    fn the_payload_carries_its_format_version_and_is_ascii() {
        let event = Event::owner_switch("pc", "off", "elle joue — ne touche pas à ma machine");
        let payload = event.payload();
        let read: serde_json::Value =
            serde_json::from_str(&payload).expect("the payload is one JSON object");
        assert_eq!(read["herd_event_version"], EVENT_FORMAT_VERSION);
        assert_eq!(read["kind"], "owner_switch");
        assert_eq!(read["runner"], "pc");
        assert_eq!(read["switch"], "off");
        assert!(
            read["at"].as_str().expect("an instant").contains('T'),
            "the payload says when, in RFC 3339: {payload}"
        );
        assert!(
            payload.is_ascii(),
            "the consumer's topic takes ASCII bodies: {payload}"
        );
        assert!(
            read["why"]
                .as_str()
                .expect("the owner's reason")
                .starts_with("elle joue - ne touche pas ?"),
            "a reason that is not ASCII is translated where it can be and \
             marked where it cannot: {payload}"
        );
        assert!(!payload.contains('\n'), "one event is one line: {payload}");
    }

    /// E-37's detached end: the payload carries the job, the one word a
    /// router switches on, and the sentence a person reads.
    ///
    /// The two are separate fields on purpose. A consumer that routes on
    /// *what kind of ending* this was must not have to parse an English
    /// sentence to find out, and a person reading the notification must not
    /// have to look the job up to learn what `exited` meant.
    #[test]
    fn an_end_says_the_job_the_cause_and_the_sentence() {
        let event = Event::job_ended(
            "j0000042",
            "stopped_by_cap",
            "was stopped by its Time cap of 600",
        );
        let payload = event.payload();
        let read: serde_json::Value =
            serde_json::from_str(&payload).expect("the payload is one JSON object");
        assert_eq!(read["herd_event_version"], EVENT_FORMAT_VERSION);
        assert_eq!(read["kind"], "job_ended");
        assert_eq!(read["job"], "j0000042");
        assert_eq!(read["cause"], "stopped_by_cap");
        assert_eq!(read["how"], "was stopped by its Time cap of 600");
        assert_eq!(read["title"], "herd: j0000042 stopped_by_cap");
        assert_eq!(read["body"], "j0000042 was stopped by its Time cap of 600");
        assert!(payload.is_ascii(), "{payload}");
        assert!(!payload.contains('\n'), "one event is one line: {payload}");
    }

    #[test]
    fn nothing_in_an_event_can_forge_a_header_or_a_second_line() {
        let event = Event::runner_lost("pc\r\nX-Smuggled: yes", "it went\naway");
        let title = one_line(&event.title());
        assert!(
            !title.contains('\r') && !title.contains('\n'),
            "the title goes in a header and must be one line: {title}"
        );
        assert_eq!(title, "herd: pc X-Smuggled: yes lost");
        let payload = event.payload();
        assert!(
            !payload.contains('\n') && !payload.contains('\r'),
            "the payload is one line: {payload}"
        );
    }

    #[test]
    fn a_hub_with_no_sink_says_nothing_and_costs_nothing() {
        let notifier = Notifier::silent();
        assert!(!notifier.configured());
        // The point is that this is a no-op that cannot fail and cannot
        // block: every call site in the hub runs it on every transition,
        // including in every test of something else.
        let began = Instant::now();
        for _ in 0..10_000 {
            notifier.say(Event::runner_joined("test-runner", "0.4.0"));
        }
        assert!(
            began.elapsed() < Duration::from_millis(500),
            "ten thousand events to no sink took {:?}",
            began.elapsed()
        );
    }

    #[tokio::test]
    async fn say_never_blocks_on_a_sink_that_hangs_and_drops_rather_than_grows() {
        // The real sink, hanging the real way: a command that does not
        // return. Nothing outside this machine is touched, and the child is
        // reaped when the delivery task is dropped.
        let (notifier, deliveries) = Notifier::to(Sink::Command("sleep 45".to_owned()));
        let delivering = tokio::spawn(deliveries.run());
        // Far more events than the queue holds, so the last ones meet a full
        // queue and the first one meets a sink that will never answer.
        let began = Instant::now();
        for which in 0..(EVENTS_QUEUED * 4) {
            notifier.say(Event::job_lost(&format!("j{which:07}"), "its runner went"));
        }
        let took = began.elapsed();
        assert!(
            took < Duration::from_millis(500),
            "{} events against a sink that hangs took {took:?}; property 1 is that \
             nothing waits on a sink",
            EVENTS_QUEUED * 4
        );
        assert!(
            notifier.dropped.load(Ordering::Relaxed) > 0,
            "a bounded queue in front of a hanging sink has to drop, not grow"
        );
        // And dropping the task kills the child: nothing of this test is
        // left running on a shared host.
        delivering.abort();
    }

    #[tokio::test]
    async fn an_event_reaches_an_http_sink_as_one_post_with_its_title() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let addr = listener.local_addr().expect("the port it got");
        let heard = tokio::spawn(async move {
            let (link, _) = listener.accept().await.expect("the hub connects");
            let mut reader = tokio::io::BufReader::new(link);
            let mut head = String::new();
            loop {
                let mut line = String::new();
                let read = reader.read_line(&mut line).await.expect("the request");
                head.push_str(&line);
                if read == 0 || line == "\r\n" {
                    break;
                }
            }
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .expect("a length")
                .trim()
                .parse()
                .expect("a number");
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).await.expect("the body");
            reader
                .into_inner()
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .expect("the answer");
            (head, String::from_utf8(body).expect("JSON is UTF-8"))
        });
        let sink = Sink::parse(&format!("http://{addr}/herd/our-topic")).expect("a URL sink");
        let event = Event::runner_lost("pc", "its link to the hub closed");
        deliver(&sink, &event).await.expect("the sink answered 200");
        let (head, body) = heard.await.expect("the listener finished");
        assert!(
            head.starts_with("POST /herd/our-topic HTTP/1.1\r\n"),
            "one POST to the path the URL named: {head}"
        );
        assert!(
            head.contains("Title: herd: pc lost\r\n"),
            "the title is a header, which is what ntfy reads: {head}"
        );
        assert!(
            head.contains("X-Herd-Event: runner_lost\r\n"),
            "the kind is a header too, so a router needs no body: {head}"
        );
        let read: serde_json::Value = serde_json::from_str(&body).expect("the body is the payload");
        assert_eq!(read["kind"], "runner_lost");
        assert_eq!(read["runner"], "pc");
    }

    #[tokio::test]
    async fn a_sink_that_refuses_is_a_failure_with_its_own_answer_in_it() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let addr = listener.local_addr().expect("the port it got");
        tokio::spawn(async move {
            let (mut link, _) = listener.accept().await.expect("the hub connects");
            let mut swallow = [0u8; 4096];
            let _ = link.read(&mut swallow).await;
            let _ = link
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                .await;
        });
        let sink = Sink::parse(&format!("http://{addr}/no-such-topic")).expect("a URL sink");
        let why = deliver(&sink, &Event::runner_joined("pc", "0.4.0"))
            .await
            .expect_err("404 is not a delivery");
        assert!(
            why.contains("404"),
            "the operator reads the sink's own answer: {why}"
        );
    }

    #[tokio::test]
    async fn a_command_sink_gets_the_payload_on_its_stdin_and_its_exit_code_is_the_verdict() {
        let kept = tempfile::tempdir().expect("a temporary directory");
        let file = kept.path().join("events.jsonl");
        let sink = Sink::parse(&format!("cat >> {}", file.display())).expect("a command sink");
        deliver(&sink, &Event::runner_joined("pc", "0.4.0"))
            .await
            .expect("cat takes it");
        deliver(&sink, &Event::job_lost("j0000001", "its runner went twice"))
            .await
            .expect("cat takes it");
        let written = std::fs::read_to_string(&file).expect("the file the sink wrote");
        let lines: Vec<&str> = written.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 2, "one line per event: {written}");
        let first: serde_json::Value = serde_json::from_str(lines[0]).expect("JSON");
        assert_eq!(first["kind"], "runner_joined");

        let refusing = Sink::parse("exit 3").expect("a command sink");
        let why = deliver(&refusing, &Event::runner_joined("pc", "0.4.0"))
            .await
            .expect_err("a non-zero exit is a failure");
        assert!(
            why.contains("exit 3") || why.contains("exit status: 3"),
            "the failure names the command and its code: {why}"
        );
    }

    #[tokio::test]
    async fn a_sink_that_hangs_is_given_up_on_and_the_next_event_still_goes() {
        // The same shape as the hub's own loop, with the cap shortened to
        // what a test can wait for: the point is that a hung delivery ends
        // and the queue keeps moving.
        let began = Instant::now();
        let hung = tokio::time::timeout(
            Duration::from_millis(300),
            deliver(
                &Sink::Command("sleep 45".to_owned()),
                &Event::job_lost("j1", "gone"),
            ),
        )
        .await;
        assert!(hung.is_err(), "a sink that hangs does not answer");
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the timeout is what ends it, not the sink: {:?}",
            began.elapsed()
        );
        deliver(
            &Sink::Command("true".to_owned()),
            &Event::job_lost("j2", "gone"),
        )
        .await
        .expect("the next event still goes");
    }
}
