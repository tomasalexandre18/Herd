//! The order of the one queue, and the words a waiting job gets (E-18, E-20).
//!
//! **One queue holds everything the hub has to do** (E-20): the queue *is*
//! the set of job directories that have not reached a terminal state, and
//! this module is the order in which they come out of it. There is no second
//! list, no lane for a merger and no in-memory fast path for an urgent job —
//! a priority is a number this module sorts on, never a side door
//! ([`order`]).
//!
//! **Aging is computed, never written** (E-18). A job's effective level is
//! its declared level raised by one step for every [`Policy::aging_step`] it
//! has spent in the queue, capped at the top. That is a pure function of the
//! record and the current moment, so:
//!
//! * nothing has to tick, so a hub with a full queue and no runner does not
//!   write to the disk once a minute;
//! * there is nothing to lose in a crash — a job's age is `now` minus the
//!   `submitted_at` on its own record, which the store fsynced when it minted
//!   the job (D68);
//! * and no record ever claims a level its requester did not ask for. The
//!   `priority` field is what was *asked for* (and what an operator raised
//!   it to); the effective level is derived, every time.
//!
//! The moment it is computed from is an argument, never read from the clock
//! in here. That is what lets a test make ninety minutes pass in no time at
//! all **through the same code the hub runs**: `dispatch_queued_jobs` calls
//! [`order`] with `Timestamp::now()`, and the simulated-time tests call it
//! with a moment of their choosing. There is no second implementation to
//! disagree with (D67).

use std::time::Duration;

use herd_proto::Priority;

use crate::job::{JobRecord, Timestamp};

/// How long a job waits in the queue before its effective level rises one
/// step (E-18).
///
/// Thirty minutes, so that the lowest of the four levels — the cache
/// producer — reaches the top ninety minutes after it was submitted, which
/// is the figure E-18's own acceptance criterion names: *a producer job
/// behind 4 step jobs runs within 90 min on a busy day*. An operator can
/// tune it (`--aging-step`), and a test sets it to milliseconds.
pub const AGING_STEP: Duration = Duration::from_secs(30 * 60);

/// Where one queued job stands: what it asked for, what it counts as now,
/// and how long until that changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    /// The level on the record: what the requester asked for, raised by an
    /// operator if one did.
    pub declared: Priority,
    /// The level the queue weighs it at now: `declared`, raised one step per
    /// whole `aging_step` it has waited, capped at the top.
    pub effective: Priority,
    /// How long it has been in the queue.
    pub waited: Duration,
    /// The next level up and how long until it gets there, or `None` when it
    /// is already at the top and nothing more will change.
    pub rises_to: Option<(Priority, Duration)>,
}

impl Standing {
    /// Whether aging has moved this job at all.
    pub fn aged(&self) -> bool {
        self.effective != self.declared
    }
}

/// Where a job stands at a given moment, under a given aging step.
///
/// `now` is an argument on purpose: see this module's own documentation.
pub fn standing(record: &JobRecord, now: Timestamp, aging_step: Duration) -> Standing {
    let declared = record.priority;
    let waited = now.since(record.submitted_at);
    // A step of zero would mean "rise infinitely often, immediately", which
    // is not aging but a queue with one level. Read as no aging at all, which
    // is the only other honest reading and the one an operator can see in the
    // sentence a waiting job carries.
    let (steps, time_into_this_step) = if aging_step.is_zero() {
        (0, Duration::ZERO)
    } else {
        (
            u32::try_from(waited.as_nanos() / aging_step.as_nanos()).unwrap_or(u32::MAX),
            Duration::from_nanos((waited.as_nanos() % aging_step.as_nanos()) as u64),
        )
    };
    let effective = declared.raised_by(steps);
    let rises_to = match effective.higher() {
        // Still climbing: the next step comes at the end of the one it is in.
        Some(next) if !aging_step.is_zero() => Some((next, aging_step - time_into_this_step)),
        _ => None,
    };
    Standing {
        declared,
        effective,
        waited,
        rises_to,
    }
}

/// The one queue, in the order the hub serves it (E-18, E-20).
///
/// Highest effective level first; within a level, **the order the jobs were
/// minted in** — which is FIFO, and is read off the job ids rather than off
/// the timestamps. The ids are minted by one counter under one lock and are
/// therefore monotonic; `submitted_at` is wall-clock time, which can step
/// backwards, and a queue that reordered itself because `ntpd` corrected the
/// host would be a queue nobody could explain.
///
/// Jobs that are not queued are not in the result at all: an attempt that is
/// under way is not waiting for a turn.
pub fn order(records: Vec<JobRecord>, now: Timestamp, aging_step: Duration) -> Vec<JobRecord> {
    let mut queued: Vec<JobRecord> = records
        .into_iter()
        .filter(|record| record.state == crate::job::JobState::Queued)
        .collect();
    // Stable, and the tie-break is explicit anyway: the id. `unfinished_jobs`
    // already gives them oldest-id first, so the sort only moves what a
    // priority or an aged level says to move.
    queued.sort_by(|left, right| {
        let left_rank = standing(left, now, aging_step).effective.rank();
        let right_rank = standing(right, now, aging_step).effective.rank();
        left_rank
            .cmp(&right_rank)
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });
    queued
}

/// What a queued job's `waiting_for` says, with its figures (E-04, E-18).
///
/// Written for the person who runs `herdctl wait` on a job that is not
/// moving, or reads its `job.json`. It answers three questions in one
/// sentence: where the job is in the queue, what it is being weighed at, and
/// when that will change. An aged job says both levels, because "priority
/// producer" alone would be a true word inside a false sentence — the queue
/// is not treating it as a producer any more.
///
/// `why` is the whole clause naming the cause, and it is the caller's
/// because there are two different causes with two different subjects: this
/// job's own capabilities, or the job in front of it that cannot start.
/// Without one of them the sentence would say a job is second in a queue that
/// is not moving, and not say why nothing is moving.
pub fn waiting_sentence(place: usize, total: usize, standing: &Standing, why: &str) -> String {
    let mut sentence = format!(
        "queued {} of {total} at priority {}",
        ordinal(place),
        standing.declared
    );
    if standing.aged() {
        sentence.push_str(&format!(
            ", weighed as {} after waiting {}",
            standing.effective,
            roughly(standing.waited)
        ));
    }
    match standing.rises_to {
        Some((next, left)) => {
            sentence.push_str(&format!(", rising to {next} in {}", roughly(left)));
        }
        None => sentence.push_str(", at the highest priority there is"),
    }
    sentence.push_str("; ");
    sentence.push_str(why);
    sentence
}

/// What is in front of this job in the queue, by name and level (E-04).
///
/// E-04 asks a queued stream for three things: the position, **what is
/// ahead**, and why. The position alone answers none of the questions an
/// operator actually has — a job that is third of forty behind two nightly
/// passes is in a different situation from one that is third behind two
/// mergers, and the figure is the same. So the jobs ahead are named, with the
/// level each is weighed at, and an aged one says both its levels for the
/// reason [`waiting_sentence`] does.
///
/// `place` is this job's index in `queue`, zero-based.
pub fn what_is_ahead(
    queue: &[JobRecord],
    place: usize,
    now: Timestamp,
    aging_step: Duration,
) -> String {
    let ahead = &queue[..place.min(queue.len())];
    if ahead.is_empty() {
        return "nothing is ahead of it".to_owned();
    }
    let named: Vec<String> = ahead
        .iter()
        .take(AHEAD_NAMED)
        .map(|record| {
            let standing = standing(record, now, aging_step);
            if standing.aged() {
                format!(
                    "{} ({} weighed as {})",
                    record.id, standing.declared, standing.effective
                )
            } else {
                format!("{} ({})", record.id, standing.declared)
            }
        })
        .collect();
    let mut said = format!("{} ahead of it: {}", ahead.len(), named.join(", "));
    if ahead.len() > AHEAD_NAMED {
        said.push_str(&format!(" and {} more", ahead.len() - AHEAD_NAMED));
    }
    said
}

/// How many of the jobs ahead are named one by one before the rest are
/// counted.
///
/// Three, because this string lands in `waiting_for` — a field the store
/// writes a line of `events.jsonl` for whenever it changes — and a sentence
/// naming all of a four-hundred-job queue would be a kilobyte of history per
/// job per position. Three is enough to see *what kind* of work is in front,
/// which is the question E-04's "what is ahead" is asked for.
const AHEAD_NAMED: usize = 3;

/// The same sentence, with what is ahead of the job in it (E-04).
pub fn waiting_sentence_behind(
    place: usize,
    total: usize,
    standing: &Standing,
    ahead: &str,
    why: &str,
) -> String {
    let mut sentence = waiting_sentence(place, total, standing, why);
    sentence.push_str("; ");
    sentence.push_str(ahead);
    sentence
}

/// A place in the queue, as a person says it.
fn ordinal(place: usize) -> String {
    let suffix = match (place % 10, place % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{place}{suffix}")
}

/// A duration as a sentence carries it: coarse on purpose.
///
/// Rounded to the minute above a minute, because this string lands in
/// `waiting_for`, and the store writes that field only when it *changes*
/// (and writes a line of `events.jsonl` with it). A countdown to the second
/// would make every dispatch pass a write and a job that waits an hour an
/// unreadable history — the very thing `record_waiting`'s "only when it is
/// news" rule exists to prevent.
pub(crate) fn roughly(how_long: Duration) -> String {
    let seconds = how_long.as_secs();
    if seconds < 60 {
        return "under a minute".to_owned();
    }
    let minutes = (seconds + 30) / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    match minutes % 60 {
        0 => format!("{hours}h"),
        rest => format!("{hours}h{rest:02}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{JobState, NewJob};
    use herd_proto::{JobId, JobMetrics, LabelSet};

    /// A queued record as the store would have written it, with the id and
    /// the submission moment the test wants. Nothing here touches a disk or
    /// a clock: this is the queue's order, which is arithmetic.
    fn queued(id: &str, priority: Priority, submitted_at: Timestamp) -> JobRecord {
        JobRecord {
            id: JobId::new(id.to_owned()),
            command: vec!["true".to_owned()],
            labels: LabelSet::default(),
            priority,
            build_thread_mib: None,
            memory_mib: None,
            tmpfs_mib: None,
            deadline: None,
            after: None,
            requeue_exit_code: None,
            parallelism: None,
            submissions: 1,
            state: JobState::Queued,
            waiting_for: None,
            submitted_at,
            updated_at: submitted_at,
            attempts: Vec::new(),
            metrics: JobMetrics::default(),
        }
    }

    const STEP: Duration = Duration::from_secs(30 * 60);

    fn ids(records: &[JobRecord]) -> Vec<&str> {
        records.iter().map(|record| record.id.as_str()).collect()
    }

    #[test]
    fn the_highest_priority_goes_first_and_fifo_breaks_a_tie() {
        let now = Timestamp::now();
        let queue = vec![
            queued("j0000000001", Priority::Producer, now),
            queued("j0000000002", Priority::Step, now),
            queued("j0000000003", Priority::Merger, now),
            queued("j0000000004", Priority::Step, now),
            queued("j0000000005", Priority::Nightly, now),
        ];
        assert_eq!(
            ids(&order(queue, now, STEP)),
            [
                "j0000000003",
                "j0000000002",
                "j0000000004",
                "j0000000005",
                "j0000000001"
            ]
        );
    }

    /// E-43: the merger is submitted last and is served first.
    #[test]
    fn a_merger_submitted_last_is_served_first() {
        let now = Timestamp::now();
        let queue = vec![
            queued("j0000000001", Priority::Step, now),
            queued("j0000000002", Priority::Step, now),
            queued("j0000000003", Priority::Merger, now),
        ];
        assert_eq!(ids(&order(queue, now, STEP))[0], "j0000000003");
    }

    /// The attack: a producer behind an endless stream of step jobs. Ninety
    /// minutes of simulated time, a new step job every five, and the
    /// producer still has to run.
    #[test]
    fn the_lowest_priority_job_reaches_the_front_within_the_aging_bound() {
        let submitted = Timestamp::now();
        let producer = queued("j0000000001", Priority::Producer, submitted);
        let bound = STEP * Priority::Producer.steps_to_the_top();
        assert_eq!(bound, Duration::from_secs(90 * 60));

        // While its effective level is still *below* the stream's, it is
        // behind the stream: this is the queue being honest about what aging
        // costs, not a failure. (It in fact reaches the front at sixty
        // minutes, two steps up, where it draws level with `step` and FIFO
        // breaks the tie in favour of the older job. The bound is the
        // guarantee, not the measurement.)
        for minutes in [0u64, 5, 29, 31, 59] {
            let now = submitted.after(Duration::from_secs(minutes * 60));
            let mut queue = vec![producer.clone()];
            for later in 1..=(minutes / 5 + 1) {
                queue.push(queued(
                    &format!("j{:0>10}", 100 + later),
                    Priority::Step,
                    now,
                ));
            }
            let first = order(queue, now, STEP)[0].id.as_str().to_owned();
            assert_ne!(
                first, "j0000000001",
                "the producer should not reach the front {minutes} minutes in, \
                 before the bound"
            );
        }

        // At the bound it is at the top level, and FIFO inside that level
        // puts it in front of every step job in the stream — however long
        // the stream goes on.
        let now = submitted.after(bound);
        let mut queue = vec![producer];
        for later in 1..=40 {
            queue.push(queued(
                &format!("j{:0>10}", 100 + later),
                Priority::Step,
                now,
            ));
        }
        let ordered = order(queue, now, STEP);
        assert_eq!(
            ordered[0].id.as_str(),
            "j0000000001",
            "a producer submitted 90 minutes ago must be served before a step \
             stream that never stops"
        );
        // And it is at the top because it aged there, not because the record
        // was rewritten: the record still says what the requester asked for.
        assert_eq!(ordered[0].priority, Priority::Producer);
    }

    #[test]
    fn aging_climbs_one_level_per_step_and_stops_at_the_top() {
        let submitted = Timestamp::now();
        let record = queued("j0000000001", Priority::Producer, submitted);
        for (steps, expected) in [
            (0, Priority::Producer),
            (1, Priority::Nightly),
            (2, Priority::Step),
            (3, Priority::Merger),
            (4, Priority::Merger),
            (400, Priority::Merger),
        ] {
            let now = submitted.after(STEP * steps + Duration::from_secs(1));
            let standing = standing(&record, now, STEP);
            assert_eq!(standing.effective, expected, "after {steps} steps");
        }
    }

    #[test]
    fn a_job_at_the_top_has_nothing_left_to_rise_to() {
        let now = Timestamp::now();
        let standing = standing(&queued("j0000000001", Priority::Merger, now), now, STEP);
        assert_eq!(standing.rises_to, None);
        assert!(!standing.aged());
    }

    #[test]
    fn what_is_left_of_this_step_shrinks_as_the_job_waits() {
        let submitted = Timestamp::now();
        let record = queued("j0000000001", Priority::Producer, submitted);
        let ten_minutes_in = standing(&record, submitted.after(STEP / 3), STEP);
        assert_eq!(
            ten_minutes_in.rises_to,
            Some((Priority::Nightly, Duration::from_secs(20 * 60)))
        );
        let thirty_five_minutes_in =
            standing(&record, submitted.after(Duration::from_secs(35 * 60)), STEP);
        assert_eq!(thirty_five_minutes_in.effective, Priority::Nightly);
        assert_eq!(
            thirty_five_minutes_in.rises_to,
            Some((Priority::Step, Duration::from_secs(25 * 60)))
        );
    }

    /// A clock that steps backwards must not make a job older than it is —
    /// which, with a negative age read as a huge unsigned one, would age it
    /// to the top instantly.
    #[test]
    fn a_clock_that_steps_backwards_does_not_age_a_job() {
        // `now` is a day *before* the moment the record says it was
        // submitted: what a host whose clock was corrected backwards looks
        // like.
        let now = Timestamp::now();
        let submitted = now.after(Duration::from_secs(60 * 60 * 24));
        let record = queued("j0000000001", Priority::Producer, submitted);
        let standing = standing(&record, now, STEP);
        assert_eq!(standing.waited, Duration::ZERO);
        assert_eq!(standing.effective, Priority::Producer);
        assert_eq!(standing.rises_to, Some((Priority::Nightly, STEP)));
    }

    #[test]
    fn an_aging_step_of_zero_is_no_aging_rather_than_instant_aging() {
        let submitted = Timestamp::now();
        let record = queued("j0000000001", Priority::Producer, submitted);
        let standing = standing(
            &record,
            submitted.after(Duration::from_secs(60 * 60 * 24)),
            Duration::ZERO,
        );
        assert_eq!(standing.effective, Priority::Producer);
        assert_eq!(standing.rises_to, None);
    }

    /// Only queued jobs are in the queue.
    #[test]
    fn a_job_being_run_is_not_waiting_for_a_turn() {
        let now = Timestamp::now();
        let mut running = queued("j0000000001", Priority::Merger, now);
        running.state = JobState::Running;
        let queue = vec![running, queued("j0000000002", Priority::Producer, now)];
        assert_eq!(ids(&order(queue, now, STEP)), ["j0000000002"]);
    }

    // --- the deduplication key (E-05) ---

    fn key(command: &[&str], labels: &[&str]) -> String {
        NewJob::at(
            command.iter().map(|piece| (*piece).to_owned()).collect(),
            LabelSet::new(labels.iter().copied()).expect("valid labels"),
            Priority::Step,
        )
        .dedup_key()
    }

    #[test]
    fn the_same_work_has_the_same_key() {
        assert_eq!(
            key(&["cargo", "xtask", "build"], &["build", "kvm"]),
            key(&["cargo", "xtask", "build"], &["build", "kvm"])
        );
        // The label set is canonical, so the order they were named in is not
        // part of the work.
        assert_eq!(
            key(&["cargo", "xtask", "build"], &["kvm", "build"]),
            key(&["cargo", "xtask", "build"], &["build", "kvm"])
        );
    }

    /// The attack: one byte of difference anywhere must not collapse, and a
    /// key built by concatenation collides on the classic pair.
    #[test]
    fn work_that_differs_anywhere_has_a_different_key() {
        let base = key(&["cargo", "xtask", "build"], &["build"]);
        let others = [
            // one byte of one argument
            key(&["cargo", "xtask", "buildd"], &["build"]),
            key(&["cargo", "xtask", "buil"], &["build"]),
            // one byte of the command itself
            key(&["cargoo", "xtask", "build"], &["build"]),
            // an argument more, and one fewer
            key(&["cargo", "xtask", "build", "--release"], &["build"]),
            key(&["cargo", "xtask"], &["build"]),
            // an empty argument is an argument
            key(&["cargo", "xtask", "build", ""], &["build"]),
            // one byte of one label
            key(&["cargo", "xtask", "build"], &["builds"]),
            // a label more, and none at all
            key(&["cargo", "xtask", "build"], &["build", "kvm"]),
            key(&["cargo", "xtask", "build"], &[]),
        ];
        for other in &others {
            assert_ne!(&base, other, "two different pieces of work share a key");
        }
        // And every one of them differs from every other, not just from the
        // base.
        for (i, left) in others.iter().enumerate() {
            for right in &others[i + 1..] {
                assert_ne!(left, right);
            }
        }
    }

    /// The classic collision: a key made by joining the pieces with any
    /// separator makes these two equal, and they are not the same work.
    #[test]
    fn a_shifted_space_is_not_the_same_work() {
        assert_ne!(key(&["a b", "c"], &[]), key(&["a", "b c"], &[]));
        assert_ne!(key(&["a", "b", "c"], &[]), key(&["a b c"], &[]));
        // Whatever separator a naive key used, the piece can contain it.
        for separator in ["\u{0}", "\u{1f}", ":", "\n", "\t", "|"] {
            assert_ne!(
                key(&[&format!("a{separator}b"), "c"], &[]),
                key(&["a", &format!("b{separator}c")], &[])
            );
        }
    }

    /// The priority is not part of the key: two agents asking for one build
    /// at two levels are asking for one build (E-05).
    #[test]
    fn the_level_asked_for_is_not_part_of_the_work() {
        let at = |priority| {
            NewJob::at(
                vec!["cargo".to_owned(), "build".to_owned()],
                LabelSet::default(),
                priority,
            )
            .dedup_key()
        };
        assert_eq!(at(Priority::Merger), at(Priority::Producer));
    }

    // --- the sentence an operator reads ---

    #[test]
    fn a_waiting_job_says_where_it_is_what_it_counts_as_and_when_that_changes() {
        let submitted = Timestamp::now();
        let record = queued("j0000000001", Priority::Producer, submitted);
        let fresh = standing(&record, submitted.after(Duration::from_secs(60)), STEP);
        assert_eq!(
            waiting_sentence(
                3,
                7,
                &fresh,
                "the job at the front of the queue is waiting because no runner \
                 offers the capability kvm"
            ),
            "queued 3rd of 7 at priority producer, rising to nightly in 29m; \
             the job at the front of the queue is waiting because no runner \
             offers the capability kvm"
        );

        let aged = standing(&record, submitted.after(Duration::from_secs(35 * 60)), STEP);
        assert_eq!(
            waiting_sentence(1, 2, &aged, "it is waiting because the runner is busy"),
            "queued 1st of 2 at priority producer, weighed as nightly after \
             waiting 35m, rising to step in 25m; it is waiting because the \
             runner is busy"
        );

        let topped = standing(
            &record,
            submitted.after(Duration::from_secs(4 * 60 * 60)),
            STEP,
        );
        assert_eq!(
            waiting_sentence(
                1,
                1,
                &topped,
                "the job at the front of the queue is waiting because no runner \
                 is connected"
            ),
            "queued 1st of 1 at priority producer, weighed as merger after \
             waiting 4h, at the highest priority there is; the job at the \
             front of the queue is waiting because no runner is connected"
        );
    }

    #[test]
    fn a_duration_in_a_sentence_is_coarse_enough_not_to_churn_the_record() {
        assert_eq!(roughly(Duration::from_secs(0)), "under a minute");
        assert_eq!(roughly(Duration::from_secs(59)), "under a minute");
        assert_eq!(roughly(Duration::from_secs(60)), "1m");
        assert_eq!(roughly(Duration::from_secs(89)), "1m");
        assert_eq!(roughly(Duration::from_secs(90)), "2m");
        assert_eq!(roughly(Duration::from_secs(60 * 60)), "1h");
        assert_eq!(roughly(Duration::from_secs(90 * 60)), "1h30m");
        assert_eq!(roughly(Duration::from_secs(61 * 60)), "1h01m");
        // The same wait a second apart gives the same words, so the record is
        // not rewritten for a ticking clock.
        assert_eq!(
            roughly(Duration::from_secs(600)),
            roughly(Duration::from_secs(601))
        );
    }

    #[test]
    fn a_place_in_the_queue_reads_as_a_person_says_it() {
        for (place, expected) in [
            (1, "1st"),
            (2, "2nd"),
            (3, "3rd"),
            (4, "4th"),
            (11, "11th"),
            (12, "12th"),
            (13, "13th"),
            (21, "21st"),
            (22, "22nd"),
            (103, "103rd"),
            (111, "111th"),
        ] {
            assert_eq!(ordinal(place), expected);
        }
    }
}
