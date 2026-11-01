//! **A job is not offered to the machine that is already running it**
//! (invariant 2, one machine runs one job once).
//!
//! Measured on the first consumer's instance on 2026-10-31, v1.0.0. A
//! runner's link went quiet for 71.28 s at host load 172; the hub's watchdog
//! wrote attempt 1 of `j0000000655` off while that machine was **still
//! running it**, and the requeue offered the job back to that same machine.
//! The machine refused, correctly. Then the hub offered it again, and again:
//! attempts 2 to 9 between 20:28:37.160Z and 20:28:37.671Z — **eight
//! refusals in 0.51 s** — until the budget for a machine that is full ran out
//! and the job ended `refused_as_full`, exit **125**, with **no verdict**,
//! for work no machine had failed at. The runner then discarded attempt 1's
//! output: *"nothing of it is recorded; nothing of it is sent again"*.
//!
//! **Why nothing stopped it.** The hub has one throttle for this shape and it
//! is a count: `herd-hub/src/matcher.rs`'s `out_there >= at_once`, fed by what
//! the machine itself said it was holding. A refusal at the ceiling reports
//! `holding == most`, so that guard fires and the machine is offered nothing
//! more until its next health report squares the two counts. A refusal for a
//! second attempt of one job reports `holding < most` **by construction** —
//! `herd_runner::Session::take_job` answers the ceiling first whenever
//! `running >= most_at_once` — so it is reachable only *below* the announced
//! count. Their figures were holding 1, two places. `1 >= 2` is false. The
//! throttle is guaranteed inert exactly where the refusal is guaranteed to
//! repeat, and `give_the_job_another_go` ends on `dispatch_queued_jobs` with
//! no delay anywhere on the path.
//!
//! **What the suite had, and why it was green through all of this.**
//! `announced_job_ceiling.rs` asserts that no job is lost to an outage at a
//! machine's ceiling, and it is right: its runner refuses at its *announced
//! count*, so the ceiling branch answers and this one is unreachable, and it
//! releases the stale holder as soon as one refusal lands. Its oracle also
//! cannot see this refusal — `ceiling_refusals` filters on
//! `herd_runner::A_RUNNERS_OWN_CEILING`, which this sentence does not carry.
//! And `a_job_refused_past_the_bound_ends_and_names_it` drives the budget to
//! exhaustion deliberately and tolerates either branch. So the suite was not
//! missing a test of the behaviour; the behaviour was the defect. **What had
//! no test anywhere was the rate**, and that is what this file asserts.
//!
//! **Why this test needs no clock, no cut socket and no `Policy` seam.** The
//! premise is built rather than timed. What the watchdog leaves behind is a
//! record whose attempt 1 ended `TheRunnerWentAway` while the machine goes on
//! running it — and `ToHub::AttemptAbandoned` with no count reaches
//! `give_the_job_another_go` with exactly that ending, through the same call
//! the watchdog makes. So the peer hands attempt 1 back and then goes on
//! saying, in every health report, that it is still running it: the hub's
//! state is the one a 71 s silence produces, reached by a frame instead of by
//! `lost_after`. No sleep, no `tokio::time::pause`, and no wall-clock flake of
//! the ORCH-244 / ORCH-321 / ORCH-324 family.
//!
//! **And the barrier that replaces the absence timeout.** The hub answers
//! every health report with `FromHub::HealthRecorded`, and it sends that
//! answer **last** — after the report has been recorded and after the queue
//! has been judged on it. So the frames up to one `HealthRecorded` are the
//! whole of what that report caused, and "the hub did not offer this job
//! again" is a positive assertion over a bounded list rather than a wait on
//! nothing happening.

use futures_util::{SinkExt, StreamExt};
use herd_proto::{from_frame, to_frame, FromHub, JobId, Role, RunningJob, ToHub};
use tokio_tungstenite::tungstenite::Message;

mod support;

use support::{shell, Harness, PATIENCE};

type Link =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const RUNNER: &str = "a-machine-that-is-already-running-it";
const ONE_LIFE: &str = "the-process-that-kept-the-written-off-attempt";

/// The machine the first consumer ran: **20 GiB lent**, 6 GiB kept as its own
/// floor, so 14 GiB usable, and **two** places at the 7 GiB it caps a job at.
///
/// Declared rather than read off whatever host the suite runs on, and the two
/// places are the figure the whole defect turns on: the refusal under test is
/// answered at *one* of them. `LENT_MIB` is the figure the machine **lends**
/// and not the figure left after its floor — an earlier draft of this file
/// held 14 GiB here while the paragraph said 20, which is the same number
/// meaning two different things in one declaration.
const LENT_MIB: u64 = 20 * 1024;
const FLOOR_MIB: u64 = 6 * 1024;
const PLACES: u32 = 2;

/// A peer driven frame by frame, so the test decides exactly which refusal
/// the hub is given and when the place comes free.
struct Peer {
    socket: Link,
}

impl Peer {
    async fn runner(hub_url: &str, name: &str, token: &str) -> Self {
        let (mut socket, _response) = tokio_tungstenite::connect_async(hub_url)
            .await
            .expect("the hub accepts a runner");
        let hello = ToHub::Hello {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            peer_version: herd_runner::runner_version().to_owned(),
            role: Role::Runner,
            token: Some(token.to_owned()),
            name: Some(name.to_owned()),
            instance: Some(ONE_LIFE.to_owned()),
            state_root: None,
            health_every_ms: None,
            capabilities: Default::default(),
            host: herd_proto::HostSize {
                memory_mib: Some(LENT_MIB),
                threads: Some(12),
                memory_floor_mib: Some(FLOOR_MIB),
                tmpfs_mib: Some(4096),
            },
            preference: 0,
            load_milli: None,
            switch: herd_proto::Switch::On,
            // The machine's own arithmetic, announced as a real runner
            // announces it: this is the figure the hub's one throttle is
            // held against, and the refusal under test is answered under it.
            jobs_at_once: Some(PLACES),
            mesh_endpoint: None,
        };
        socket
            .send(Message::text(to_frame(&hello).expect("hello encodes")))
            .await
            .expect("hello is sent");
        let mut joined = Self { socket };
        match joined.next().await {
            Some(FromHub::Welcome { .. }) => joined,
            other => panic!("an approved runner is welcomed, got {other:?}"),
        }
    }

    async fn say(&mut self, message: ToHub) {
        self.socket
            .send(Message::text(
                to_frame(&message).expect("the frame encodes"),
            ))
            .await
            .expect("the frame is sent");
    }

    async fn next(&mut self) -> Option<FromHub> {
        loop {
            let frame = self.socket.next().await?.expect("the frame is readable");
            if frame.is_close() {
                return None;
            }
            let Ok(text) = frame.to_text() else { continue };
            if text.is_empty() {
                continue;
            }
            return Some(from_frame(text).expect("the hub speaks the protocol"));
        }
    }

    /// Wait for this machine to be handed the job, and say which attempt.
    async fn assigned(&mut self) -> u32 {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .expect("the hub hands the job out inside the suite's patience");
            match frame {
                Some(FromHub::AssignJob { attempt, .. }) => return attempt,
                Some(_) => continue,
                None => panic!("the hub never handed this runner the job"),
            }
        }
    }

    /// Say what this machine is holding, and come back with **every** frame
    /// that report caused.
    ///
    /// The barrier of this file. `FromHub::HealthRecorded` is sent after the
    /// report has been recorded and after the queue has been judged on it, so
    /// anything the hub decided to send off the back of this report is in the
    /// list below — an offer included.
    async fn through_a_health_report(&mut self, holding: &[(JobId, u32)]) -> Vec<FromHub> {
        self.say(ToHub::Health {
            jobs: holding
                .iter()
                .map(|(job_id, attempt)| RunningJob {
                    job_id: job_id.clone(),
                    attempt: *attempt,
                    chunks_sent: 0,
                })
                .collect(),
            draining: false,
            load_milli: Some(172_700),
            switch: herd_proto::Switch::On,
            pack_store_bytes: None,
            pack_store_budget_bytes: None,
            packs_held: None,
        })
        .await;
        let mut seen = Vec::new();
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .expect("the hub answers a health report inside the suite's patience")
                .expect("the link stays up");
            let done = matches!(frame, FromHub::HealthRecorded {});
            seen.push(frame);
            if done {
                return seen;
            }
        }
    }
}

/// What the machine says when the hub offers it a second attempt of a job it
/// is still running — **the frame, with the sentence taken from the runner**.
///
/// `at_its_ceiling` travels because it is still true: the machine is holding
/// one job, and the hub's records may well say it is holding none. It is not
/// what refused, and that is the point — one place of two reads as room.
fn the_refusal_of_a_machine_already_running_it(job_id: &JobId, attempt: u32, held: u32) -> ToHub {
    ToHub::AttemptAbandoned {
        job_id: job_id.clone(),
        attempt,
        reason: herd_runner::testing::a_refusal_for_a_second_attempt(),
        at_its_ceiling: Some(1),
        still_running_attempt: Some(held),
    }
}

/// Every refusal of this kind on a job's record, read as the first consumer
/// reads it: raw `events.jsonl` lines, matched on the clause the **runner**
/// owns.
///
/// Not off `ceiling_refusals`-style counting, and the blindness is proven:
/// that oracle filters on `herd_runner::A_RUNNERS_OWN_CEILING` — *"this
/// runner holds at most"* — and this sentence does not contain it, which is
/// why a green suite and a dead job were both true.
fn refusals_for_a_second_attempt(
    state_dir: &std::path::Path,
    job_id: &JobId,
) -> Vec<(u32, String)> {
    support::events_of(state_dir, job_id)
        .into_iter()
        .filter(|event| event["event"] == "attempt_ended")
        .filter_map(|event| {
            let reason = event["reason"].as_str()?.to_owned();
            let attempt = u32::try_from(event["attempt"].as_u64()?).ok()?;
            reason
                .contains(herd_runner::A_RUNNERS_OWN_SECOND_ATTEMPT)
                .then_some((attempt, reason))
        })
        .collect()
}

/// **The first consumer's job, as a test.** One machine, two places, one job
/// whose first attempt the hub has written off and the machine is still
/// running: the job **waits** for the place it is already in, and gets its own
/// verdict when that place comes free.
///
/// **Where this was red, stated as it was really run.** This file *cannot
/// compile* on `9e8fdef`: it names `still_running_attempt`,
/// `herd_runner::A_RUNNERS_OWN_SECOND_ATTEMPT` and
/// `herd_runner::testing::a_refusal_for_a_second_attempt`, and none of the
/// three exists there — T4's field is a compile-time dependency of the
/// fixture, so there is no artefact in this tree that is red without the hub
/// change. What was run, in a throwaway worktree detached at `9e8fdef`, was
/// the **equivalent** fixture sending the frame a v1.0.0 runner really sends
/// — `at_its_ceiling: Some(1)` and no discriminant, the two refusals in one
/// match arm — and it failed on both halves: the hub answered the very first
/// health report with `AssignJob { attempt: 3 }` beside it, and when the peer
/// kept refusing it reached `refused_as_full` with **8 refusals, offers 2
/// through 9, 9 attempts on the record** and the hub's own sentence about two
/// counts that never agreed. Those figures are in
/// `docs/orchestration/history/2026-11-01-self-collision-no-reoffer.md`; that
/// worktree is gone. With this field in place and the exclusion alone
/// removed, this file fails on both halves too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_job_is_not_offered_to_the_machine_already_running_it() {
    let harness = Harness::with_hub().await;
    let token = harness.approve(RUNNER);
    let state_dir = harness.state_dir().to_owned();
    let mut machine = Peer::runner(&harness.hub_url, RUNNER, &token).await;

    let job_id = support::submit_job(&harness.requester(), shell("true"))
        .await
        .expect("the submission is accepted");
    let held = machine.assigned().await;
    assert_eq!(held, 1, "the first go at a job is attempt 1");

    // **The premise, built and not timed.** This is the record a 71 s silence
    // leaves behind: `give_the_job_another_go` with `TheRunnerWentAway`, which
    // is the same ending the watchdog writes and reached through the same
    // function. The machine goes on running the attempt — it says so in every
    // report below — and the hub, having written it off, offers the job again.
    machine
        .say(ToHub::AttemptAbandoned {
            job_id: job_id.clone(),
            attempt: held,
            reason: "the hub stopped hearing about this attempt".to_owned(),
            at_its_ceiling: None,
            still_running_attempt: None,
        })
        .await;
    let second = machine.assigned().await;
    assert_eq!(
        second, 2,
        "the job keeps its goes and comes back to the only machine there is, which is \
         where the first consumer's storm began"
    );

    // And the machine refuses it, correctly: one machine runs one job once.
    machine
        .say(the_refusal_of_a_machine_already_running_it(
            &job_id, second, held,
        ))
        .await;

    // ---- The oracle, first half: the hub does not offer it again while the
    // machine still says it is holding it.
    //
    // The report names attempt 1, which the hub has written off. It is
    // refused as superseded — that is right, and it is also the **only**
    // evidence the hub gets that the place is still taken, which is why the
    // exclusion is cleared on what the report named and not on what survived
    // being judged.
    let seen = machine
        .through_a_health_report(&[(job_id.clone(), held)])
        .await;
    let offers: Vec<&FromHub> = seen
        .iter()
        .filter(|frame| matches!(frame, FromHub::AssignJob { .. }))
        .collect();
    assert!(
        offers.is_empty(),
        "the machine said it is still running this job, so the hub must offer it no further \
         attempt of it: {offers:?} (all of it: {seen:?})"
    );
    assert!(
        seen.iter().any(|frame| matches!(
            frame,
            FromHub::StaleAttempt { job_id: of, attempt, .. }
                if *of == job_id && *attempt == held
        )),
        "and the attempt it named is still answered as the hub's own records see it: {seen:?}"
    );
    assert!(
        !harness.state_of(&job_id).is_terminal(),
        "a job whose place is taken by an attempt of itself waits; it does not end. It was \
         {:?}",
        harness.state_of(&job_id)
    );
    // And read off the file too, where `refused_as_full` really lives: it is
    // an **outcome** under a `finished` state and not a state, which is the
    // one thing a reader of this assertion has to get right — on `9e8fdef`
    // this job's record says `"state": {"type": "finished"}` with
    // `"outcome": {"type": "refused_as_full", "refusals": 8}`, so a test
    // watching the state alone would read it as a job that ended well.
    assert_ne!(
        support::record_of(&state_dir, &job_id)["state"]["outcome"]["type"],
        "refused_as_full",
        "and it is certainly not refused as full: nothing is full — one place of {PLACES} is \
         taken, by an attempt of this very job"
    );

    // ---- The oracle, second half: the place comes free and the hub offers it
    // at once, off the machine's own word and with nothing timed.
    let seen = machine.through_a_health_report(&[]).await;
    let offered: Vec<u32> = seen
        .iter()
        .filter_map(|frame| match frame {
            FromHub::AssignJob {
                job_id: of,
                attempt,
                ..
            } if *of == job_id => Some(*attempt),
            _ => None,
        })
        .collect();
    assert_eq!(
        offered,
        vec![3],
        "the report that stops naming the job is the reconciliation: the place is free, so \
         the next attempt goes out on that same report and not at whatever wakes the hub \
         next. All of it: {seen:?}"
    );

    // And it runs, which is the whole point: the job the first consumer lost
    // ends with its own verdict on the machine that refused it.
    machine
        .say(ToHub::JobFinished {
            job_id: job_id.clone(),
            attempt: 3,
            outcome: herd_proto::JobOutcome::Exited { code: 0 },
            chunks_sent: 0,
            metrics: herd_proto::JobMetrics::default(),
            steps: Vec::new(),
            outputs: herd_proto::JobOutputs::default(),
        })
        .await;
    support::wait_until("the job ends with its own verdict", || {
        harness.state_of(&job_id).is_terminal()
    })
    .await;
    let record = support::record_of(&state_dir, &job_id);
    assert_eq!(
        record["state"]["type"], "finished",
        "the job ends finished and not refused_as_full, which is the first consumer's \
         exit 125 turned into a verdict: {record:#}"
    );
    assert_eq!(
        record["state"]["outcome"]["code"], 0,
        "with the exit code of the attempt that really ran: {record:#}"
    );

    // ---- And the rate, which is what had no test anywhere.
    let refusals = refusals_for_a_second_attempt(&state_dir, &job_id);
    assert_eq!(
        refusals.len(),
        1,
        "one refusal for one written-off attempt, and no more: the first consumer's record \
         carries eight of these inside 0.51 s. What this one carries: {refusals:?}"
    );
    let (attempt, said) = &refusals[0];
    assert_eq!(
        *attempt, second,
        "and it is the attempt the hub offered after the write-off: {refusals:?}"
    );
    assert!(
        !said.contains(herd_runner::A_RUNNERS_OWN_CEILING),
        "the sentence must not carry the ceiling's clause, or an oracle counting ceiling \
         refusals would read this one as a machine that was full: {said}"
    );

    // Every job the record was ever given, so the count of attempts is read
    // and not inferred: three, not nine.
    let attempts = record["attempts"]
        .as_array()
        .expect("a record lists its attempts")
        .len();
    assert_eq!(
        attempts, 3,
        "three attempts: one written off, one refused by the machine running it, one that \
         ran. The first consumer's record carries nine: {record:#}"
    );
}
