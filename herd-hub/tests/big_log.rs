//! A job's log comes back whatever its size, and a `herdctl wait` on a loud
//! job gets a verdict (D148).
//!
//! The defect this suite exists for was latent and had the worst possible
//! shape. `FromHub::JobLog` carried a job's **whole** recorded log in one
//! frame; the runner's default log cap is 64 MiB; base64 makes that about
//! 85 MiB; and Herd's own frame ceiling is 64 MiB. So a job that printed more
//! than about 48 MiB — a verbose build, a test looping on a failure — ended
//! with a frame that could not go through the socket, and what a requester
//! attaching afterwards saw was the link closing with **no verdict**: exit
//! 125 for a job that had passed. That is D117's exact shape, on the one path
//! nothing had ever exercised, and it is the same class of mistake D137 found
//! at 16 MiB and fixed only for the *output* frame.
//!
//! Three assertions, in the order they matter:
//!
//! * the premise, against the real encoder: a whole log cap in one frame is
//!   genuinely past the ceiling, so this is arithmetic that was wrong and not
//!   a worry about one that might be;
//! * one bounded frame is comfortably inside the ceiling, with the room
//!   deliberate, so raising the log cap does not silently reopen it;
//! * end to end, on a real hub with a real runner and the real requester
//!   code: a job that prints past the old bound returns its verdict and its
//!   log, byte for byte.

use std::time::Duration;

use herd_proto::{to_frame, FromHub, JobId, JobOutcome, FRAME_CEILING, JOB_LOG_FRAME_BYTES};
use herd_runner::confine::DEFAULT_LOG_CAP;

mod support;

use support::{shell, Harness};

/// A `JobLog` frame carrying `bytes` bytes of a job's log, as the hub really
/// encodes one.
fn frame_for(bytes: usize) -> usize {
    let frame = to_frame(&FromHub::JobLog {
        job_id: JobId::new("j0000000007"),
        data: vec![b'x'; bytes],
    })
    .expect("a job log frame encodes");
    frame.len()
}

/// The premise: a whole log in one frame does not fit, and never did.
///
/// Asserted against `to_frame` rather than reasoned about, for the reason
/// D137 put its cap through the real encoder: the figure that matters is what
/// the socket is handed, and base64 plus the JSON around it is not a thing to
/// estimate.
#[test]
fn a_whole_log_cap_in_one_frame_is_past_the_ceiling() {
    let whole = frame_for(DEFAULT_LOG_CAP as usize);
    assert!(
        whole as u64 > FRAME_CEILING,
        "a {} MiB log encodes to {whole} bytes, which has to be past the {} MiB ceiling for \
         this suite to be about anything — if it is not, the log cap or the ceiling moved and \
         the bound below needs deriving again",
        DEFAULT_LOG_CAP >> 20,
        FRAME_CEILING >> 20,
    );
}

/// And the bound chosen for it fits, with room on purpose.
///
/// The margin is the assertion. A frame that merely squeezed under the
/// ceiling would put the next operator who raises `--job-log-cap` back where
/// this started, so the bound is checked to leave most of the ceiling unused
/// rather than merely to pass.
#[test]
fn one_bounded_frame_fits_with_room() {
    let piece = frame_for(JOB_LOG_FRAME_BYTES);
    assert!(
        (piece as u64) < FRAME_CEILING / 2,
        "a bounded log frame encodes to {piece} bytes against a {} MiB ceiling, which is not \
         the room this figure is supposed to leave",
        FRAME_CEILING >> 20,
    );
}

/// The log of a job that printed more than one frame can hold comes back
/// whole, and so does its verdict.
///
/// A real hub, a real runner, a real job, and `herdctl`'s own `wait_for_job`
/// — the requester code, dialling with the same frame ceiling the deployed
/// binary dials with, which is where the old defect actually bit. The job
/// prints past the 48 MiB bound the one-frame backlog died at, so this test
/// fails on the previous hub rather than merely exercising the new one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_loud_job_returns_its_verdict_and_its_whole_log() {
    // Past the ~48 MiB where base64 of one whole backlog overran the
    // ceiling, and under the 64 MiB log cap so nothing is truncated: the test
    // is about the frame, not about the cap.
    const LINES: usize = 56 << 10;
    const LINE: usize = 1 << 10;
    const PRINTED: usize = LINES * LINE;

    let harness = Harness::with_hub_and_runner().await;
    let requester = harness.requester();

    // `yes` with a fixed-width line is the cheapest way to make a precise
    // number of bytes without writing a program to do it: 1 KiB a line, head
    // bounding the count, so the total is exact and the assertion can be an
    // equality rather than a range.
    let printed = shell(&format!("yes '{}' | head -n {LINES}", "x".repeat(LINE - 1)));

    let mut streamed = 0usize;
    let finished = tokio::time::timeout(
        Duration::from_secs(180),
        support::run_job(&requester, printed, |_, data| streamed += data.len()),
    )
    .await
    .expect("a loud job finishes inside three minutes")
    .expect("a loud job returns a verdict rather than a closed link");

    assert_eq!(
        finished.outcome,
        JobOutcome::Exited { code: 0 },
        "the job itself passed; a frame it could not fit in is not a verdict about it (D20)"
    );
    assert_eq!(
        streamed, PRINTED,
        "a follower gets every byte the job printed, in 8 KiB log chunks"
    );

    // And the case the defect was in: attaching **afterwards**, when the
    // stream is over and the backlog is all there is. This is `herdctl wait
    // <id>`, which is the requester the first consumer actually uses for a
    // job it did not follow.
    let mut attached = 0usize;
    let again = tokio::time::timeout(
        Duration::from_secs(180),
        herdctl::wait_for_job(
            &requester,
            finished.job_id.clone(),
            |_, data| attached += data.len(),
            |_| {},
        ),
    )
    .await
    .expect("a late attach to a loud job finishes inside three minutes")
    .expect("a late attach to a loud job returns a verdict rather than a closed link");

    assert_eq!(
        again.outcome,
        JobOutcome::Exited { code: 0 },
        "the verdict is read off the record, and the record does not care how loud the job was"
    );
    assert_eq!(
        attached, PRINTED,
        "the backlog arrives in full, however many frames it takes — this is the assertion \
         that fails on a hub sending it whole, where the frame never arrives at all"
    );
}
