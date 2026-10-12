//! The hub: one listener, one runner, any number of requesters.
//!
//! Every connection is dialled by the peer (invariant 3) and starts with a
//! `Hello` whose protocol version must match. A requester's `SubmitJob` is
//! minted into a job, handed to the connected runner, and everything the
//! runner says about that job is relayed back to the requester that submitted
//! it. There is no scheduling yet: with no runner connected, a submission is
//! refused by name (M6 gives the hub a queue to put it in instead).
//!
//! Everything the hub knows is on its disk before it is acted on, through the
//! [`JobStore`] it was given. Two consequences run through this file. A result
//! is acknowledged to the runner only once it is recorded, because the runner
//! is holding it until then (E-44). And a job that is still queued when a
//! runner joins is handed to it, whether that job was submitted a second ago
//! or inherited from the hub that ran before this one.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use herd_proto::{
    from_frame, to_frame, version_mismatch_reason, FromHub, JobId, Role, ToHub, PROTOCOL_VERSION,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::job::{JobState, JobStore};

/// Loopback by default: the hub is reached through the reverse proxy, which
/// terminates TLS; the hub itself only ever speaks plain `ws://` (D4).
pub const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:8787";

/// The hub's own version, reported in `Welcome`.
pub fn hub_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

type Socket = WebSocketStream<TcpStream>;
type Sink = SplitSink<Socket, Message>;
type Source = SplitStream<Socket>;

/// Queue of frames waiting to go out on one connection. Unbounded so that a
/// relay never blocks the task that reads from the runner; M3 revisits this
/// when it adds backpressure and heartbeats.
type Outbox = UnboundedSender<FromHub>;

/// A bound hub, not yet serving.
pub struct Hub {
    listener: TcpListener,
    state: Arc<State>,
}

impl Hub {
    /// Bind the listener. Port 0 binds a free port; ask [`Hub::local_addr`]
    /// which one it got.
    pub async fn bind(addr: SocketAddr, store: Arc<dyn JobStore>) -> Result<Self> {
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding {addr}"))?;
        Ok(Self {
            listener,
            state: Arc::new(State {
                store,
                peers: Mutex::new(Peers::default()),
            }),
        })
    }

    /// The address actually bound.
    pub fn local_addr(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("a bound listener has a local address")
    }

    /// A read-only handle on the hub's live peers, kept by the caller after
    /// [`Hub::serve`] has taken the hub. M13 grows this into the status
    /// surface; M1 needs only "is the runner here yet".
    pub fn handle(&self) -> HubHandle {
        HubHandle {
            state: Arc::clone(&self.state),
        }
    }

    /// Serve until the listener fails. Dropping the returned future releases
    /// the port.
    pub async fn serve(self) -> Result<()> {
        loop {
            let (stream, peer) = self.listener.accept().await.context("accepting")?;
            let state = Arc::clone(&self.state);
            tokio::spawn(async move {
                if let Err(error) = serve_peer(state, stream).await {
                    eprintln!("herd-hub: connection from {peer} ended: {error:#}");
                }
            });
        }
    }
}

/// What can still be asked of a hub that `serve` has taken ownership of.
#[derive(Clone)]
pub struct HubHandle {
    state: Arc<State>,
}

impl HubHandle {
    /// Whether a runner has finished its handshake and can be dispatched to.
    pub fn runner_connected(&self) -> bool {
        self.state.with_peers(|peers| peers.runner.is_some())
    }
}

struct State {
    store: Arc<dyn JobStore>,
    peers: Mutex<Peers>,
}

/// What a requester gets when it attaches to a job by its id.
struct Attached {
    record: crate::job::JobRecord,
    /// The output the job had already produced.
    log: Vec<u8>,
    /// Why the job is not moving, if this hub knows that it is not (D19).
    stalled: Option<String>,
}

#[derive(Default)]
struct Peers {
    /// M1 accepts exactly one runner. M5 onwards holds a set.
    runner: Option<Outbox>,
    /// Who to relay a job's logs and outcome to. More than one is possible
    /// from M2: a `herdctl run` follows the job it submitted, and a
    /// `herdctl wait` on the same id attaches beside it rather than taking
    /// its place.
    followers: HashMap<JobId, Vec<Outbox>>,
    /// Per job, why the hub could not write down what it was told about it.
    ///
    /// A job whose end the hub cannot store keeps a record that reads
    /// `running` while nothing at all is happening to it (D19): the runner is
    /// holding the result, and this hub has stopped trying. Nothing about that
    /// is visible from the outside, which is why the reason is kept here —
    /// what a wait on such a job may not do is say nothing. Cleared the moment
    /// the hub does write the job down.
    stalled: HashMap<JobId, String>,
}

impl State {
    fn with_peers<T>(&self, f: impl FnOnce(&mut Peers) -> T) -> T {
        let mut peers = self
            .peers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut peers)
    }

    /// Returns the reason to refuse, if the single runner slot is taken.
    fn register_runner(&self, outbox: &Outbox) -> std::result::Result<(), String> {
        self.with_peers(|peers| match &peers.runner {
            Some(_) => Err("a runner is already connected; M1 accepts one".to_owned()),
            None => {
                peers.runner = Some(outbox.clone());
                Ok(())
            }
        })
    }

    /// Give up the runner slot, and tell whoever was following a job of this
    /// runner that it is not coming back. M3 requeues instead of refusing.
    fn release_runner(&self, outbox: &Outbox) -> Vec<Outbox> {
        self.with_peers(|peers| {
            if !peers
                .runner
                .as_ref()
                .is_some_and(|r| r.same_channel(outbox))
            {
                return Vec::new();
            }
            peers.runner = None;
            peers.followers.drain().flat_map(|(_, f)| f).collect()
        })
    }

    fn runner(&self) -> Option<Outbox> {
        self.with_peers(|peers| peers.runner.clone())
    }

    fn follow(&self, id: JobId, outbox: &Outbox) {
        self.with_peers(|peers| peers.followers.entry(id).or_default().push(outbox.clone()));
    }

    /// Record one chunk of a job's output and take the list of who is
    /// following that job, in one breath.
    ///
    /// The two happen under one lock on purpose. A requester attaching to a
    /// running job is given the output recorded so far and registered as a
    /// follower under the same lock ([`State::attach`]), so every chunk is
    /// carried by exactly one of the two paths: it is in the log a waiter
    /// reads, or it is relayed to it, never both and never neither.
    fn record_chunk(
        &self,
        id: &JobId,
        stream: herd_proto::LogStream,
        seq: u64,
        data: &[u8],
    ) -> (crate::job::Result<()>, Vec<Outbox>) {
        self.with_peers(|peers| {
            let written = self.store.append_log(id, stream, seq, data);
            let followers = peers.followers.get(id).cloned().unwrap_or_default();
            (written, followers)
        })
    }

    /// Remember that the hub cannot write a job down, and take the list of who
    /// is waiting for it.
    fn note_a_stall(&self, id: &JobId, reason: String) -> Vec<Outbox> {
        self.with_peers(|peers| {
            peers.stalled.insert(id.clone(), reason);
            peers.followers.get(id).cloned().unwrap_or_default()
        })
    }

    /// The hub wrote the job down after all: it is moving again.
    fn stall_is_over(&self, id: &JobId) {
        self.with_peers(|peers| peers.stalled.remove(id));
    }

    /// Attach a requester to a job by its id: its record, the output recorded
    /// so far, why the job is not moving if it is not, and — unless the job is
    /// already over — a place in the list of followers, all under one lock.
    ///
    /// Reading the record and registering have to be one step too: between
    /// them the job could reach its end, and a requester that registered
    /// afterwards would wait for a frame that has already been sent.
    fn attach(&self, id: &JobId, outbox: &Outbox) -> crate::job::Result<Option<Attached>> {
        self.with_peers(|peers| {
            let Some(record) = self.store.job(id)? else {
                return Ok(None);
            };
            let log = self.store.job_log(id)?.unwrap_or_default();
            let stalled = peers.stalled.get(id).cloned();
            if !record.state.is_terminal() {
                peers
                    .followers
                    .entry(id.clone())
                    .or_default()
                    .push(outbox.clone());
            }
            Ok(Some(Attached {
                record,
                log,
                stalled,
            }))
        })
    }

    fn stop_following(&self, id: &JobId) -> Vec<Outbox> {
        self.with_peers(|peers| peers.followers.remove(id).unwrap_or_default())
    }

    /// A requester went away: nobody is waiting for its jobs any more.
    fn forget_follower(&self, outbox: &Outbox) {
        self.with_peers(|peers| {
            for following in peers.followers.values_mut() {
                following.retain(|f| !f.same_channel(outbox));
            }
            peers.followers.retain(|_, following| !following.is_empty());
        });
    }
}

async fn serve_peer(state: Arc<State>, stream: TcpStream) -> Result<()> {
    // Log chunks are small and must not wait for a full segment.
    let _ = stream.set_nodelay(true);
    let socket = tokio_tungstenite::accept_async(stream)
        .await
        .context("websocket handshake")?;
    let (mut sink, mut source) = socket.split();

    let Some(hello) = next_message(&mut source).await? else {
        bail!("the peer closed before saying hello");
    };
    let role = match hello {
        ToHub::Hello {
            protocol_version,
            role,
            token,
            name,
        } => {
            if protocol_version != PROTOCOL_VERSION {
                return refuse(&mut sink, version_mismatch_reason(protocol_version)).await;
            }
            // The token is carried from M1 and validated from M15 (D6, Q8).
            let _ = token;
            let _ = name;
            role
        }
        other => {
            let reason = format!(
                "the first message must be hello, not {}",
                message_kind(&other)
            );
            return refuse(&mut sink, reason).await;
        }
    };

    send(
        &mut sink,
        &FromHub::Welcome {
            protocol_version: PROTOCOL_VERSION,
            hub_version: hub_version().to_owned(),
        },
    )
    .await?;

    let (outbox, inbox) = mpsc::unbounded_channel();
    let writer = tokio::spawn(write_frames(sink, inbox));
    let outcome = match role {
        Role::Runner => serve_runner(&state, &outbox, &mut source).await,
        Role::Requester => serve_requester(&state, &outbox, &mut source).await,
    };
    drop(outbox); // ends the writer, which then closes the socket
    let _ = writer.await;
    outcome
}

async fn serve_runner(state: &State, outbox: &Outbox, source: &mut Source) -> Result<()> {
    if let Err(reason) = state.register_runner(outbox) {
        let _ = outbox.send(FromHub::Refused { reason });
        return Ok(());
    }
    // Whatever happens from here, the runner slot is given back before this
    // returns: a failure that kept the slot would mean no runner could ever
    // join this hub again.
    let outcome = match dispatch_recovered_jobs(state, outbox) {
        Ok(()) => relay_from_runner(state, outbox, source).await,
        Err(error) => Err(error),
    };
    for follower in state.release_runner(outbox) {
        let _ = follower.send(FromHub::Refused {
            reason: "the runner was lost while the job was running".to_owned(),
        });
    }
    outcome
}

/// Hand a newly joined runner the jobs that are still queued.
///
/// After a restart the queue is whatever the job directories say it is, and a
/// job that was queued when the hub went down is simply a job waiting for a
/// runner. Nobody follows these: the requester that submitted them is long
/// gone, which is exactly the detached case (E-03), and the result is recorded
/// whether anyone is listening or not.
///
/// Only *queued* jobs are dispatched. A job that was already assigned or
/// running belongs to an attempt this hub did not see end; deciding that such
/// an attempt is lost, and requeueing it once, is M3's — doing it here would
/// risk running a job twice.
fn dispatch_recovered_jobs(state: &State, runner: &Outbox) -> Result<()> {
    let waiting = state
        .store
        .unfinished_jobs()
        .context("reading the queue the hub recovered")?;
    for record in waiting {
        if record.state != JobState::Queued {
            continue;
        }
        state
            .store
            .record_state(&record.id, JobState::Assigned)
            .with_context(|| format!("recording {} as assigned", record.id))?;
        if runner
            .send(FromHub::AssignJob {
                job_id: record.id.clone(),
                command: record.command,
            })
            .is_err()
        {
            return Ok(());
        }
        println!(
            "herd-hub: {} was still queued and has been handed to the runner",
            record.id
        );
    }
    Ok(())
}

async fn relay_from_runner(state: &State, outbox: &Outbox, source: &mut Source) -> Result<()> {
    while let Some(message) = next_message(source).await? {
        match message {
            ToHub::JobStarted { job_id } => {
                // A failure here is the hub's disk, not the runner's doing, so
                // it is said out loud and the connection goes on (D19) — and
                // whoever is waiting for the job is told, because a record
                // that cannot move is not something a wait can see by itself.
                match state.store.record_state(&job_id, JobState::Running) {
                    Ok(()) => state.stall_is_over(&job_id),
                    Err(error) => {
                        eprintln!("herd-hub: recording {job_id} as running failed: {error}");
                        say_it_is_stalled(state, &job_id, error.to_string());
                    }
                }
            }
            ToHub::LogChunk {
                job_id,
                stream,
                seq,
                data,
            } => {
                let (written, followers) = state.record_chunk(&job_id, stream, seq, &data);
                if let Err(error) = written {
                    eprintln!("herd-hub: recording a log chunk of {job_id} failed: {error}");
                }
                // Relayed whether or not it could be recorded: a requester
                // watching the job must not go blind because the hub's disk
                // did.
                for follower in followers {
                    let _ = follower.send(FromHub::LogChunk {
                        job_id: job_id.clone(),
                        stream,
                        seq,
                        data: data.clone(),
                    });
                }
            }
            ToHub::JobFinished {
                job_id,
                outcome,
                metrics,
            } => {
                // A result for a job this hub has no record of — a state root
                // that was replaced under it, say. There is nothing to write
                // it to, and refusing it would wedge the runner, which would
                // redeliver it on every connection for ever. So it is
                // acknowledged and said out loud.
                match state.store.job(&job_id) {
                    Ok(None) => {
                        eprintln!(
                            "herd-hub: a runner reported how {job_id} ended, and this hub \
                             has no record of that job: {outcome:?} is dropped"
                        );
                        let _ = outbox.send(FromHub::JobResultAck { job_id });
                        continue;
                    }
                    // Not knowing whether there is a record is not knowing
                    // anything: the result stays with the runner (D19).
                    Err(error) => {
                        eprintln!("herd-hub: reading the record of {job_id} failed: {error}");
                        continue;
                    }
                    Ok(Some(_)) => {}
                }
                // Recorded first, acknowledged second. The runner is holding
                // this result until the acknowledgement arrives, so the order
                // of these two steps is the whole of E-44's promise: nothing
                // tells the runner to forget a result that is not yet on a
                // disk.
                if !record_the_end(state, &job_id, &outcome, &metrics).await {
                    // The hub could not write it. Nothing is acknowledged, so
                    // the runner keeps holding it and redelivers it on its
                    // next connection; the job stalls, and a stalled job is
                    // recoverable where a destroyed outcome is not (D19).
                    continue;
                }
                let _ = outbox.send(FromHub::JobResultAck {
                    job_id: job_id.clone(),
                });
                for follower in state.stop_following(&job_id) {
                    let _ = follower.send(FromHub::JobFinished {
                        job_id: job_id.clone(),
                        outcome: outcome.clone(),
                        metrics: metrics.clone(),
                    });
                }
            }
            other => {
                let _ = outbox.send(FromHub::Refused {
                    reason: format!("a runner may not send {}", message_kind(&other)),
                });
                return Ok(());
            }
        }
    }
    Ok(())
}

/// How many times the hub tries to write a result before leaving it with the
/// runner, and how long it waits between tries.
///
/// Deliberately short and dumb: the point is to ride out the kind of failure
/// that passes — a directory being replaced, a brief ENOSPC — and then to stop
/// and say so, rather than to turn one unwritable result into a connection
/// that stalls for ever. Nothing here decides a job is lost; that is M3's.
const RECORDING_ATTEMPTS: u32 = 4;
const BETWEEN_ATTEMPTS: std::time::Duration = std::time::Duration::from_millis(250);

/// Write a job's end to the disk, retrying, and say whether it is written.
///
/// A result is the one thing a runner cannot reproduce, and the hub failing to
/// store it is the hub's problem, never the runner's: it is never acknowledged
/// unless it is written, the runner is never dropped for it, and the reason is
/// said in full so that an operator can act on it (D19).
async fn record_the_end(
    state: &State,
    job_id: &JobId,
    outcome: &herd_proto::JobOutcome,
    metrics: &herd_proto::JobMetrics,
) -> bool {
    let end = JobState::Finished {
        outcome: outcome.clone(),
    };
    let mut why = String::new();
    for attempt in 1..=RECORDING_ATTEMPTS {
        // The measurements go down before the transition, so that a reader
        // who sees `finished` is entitled to find the figures beside it
        // (E-15) — and inside the same retry, because a hub that could not
        // write what a runner measured must not acknowledge the result
        // either. Failing to store either half is the hub's problem, never
        // the runner's, and never a reason to drop it (D19).
        match state
            .store
            .record_metrics(job_id, metrics.clone())
            .and_then(|()| state.store.record_state(job_id, end.clone()))
        {
            Ok(()) => {
                state.stall_is_over(job_id);
                return true;
            }
            Err(error) => {
                why = error.to_string();
                eprintln!(
                    "herd-hub: recording the outcome of {job_id} failed \
                     (attempt {attempt} of {RECORDING_ATTEMPTS}): {error}"
                );
                if attempt < RECORDING_ATTEMPTS {
                    tokio::time::sleep(BETWEEN_ATTEMPTS).await;
                }
            }
        }
    }
    eprintln!(
        "herd-hub: {job_id} ended with {outcome:?} and this hub cannot write it. \
         The result is NOT acknowledged: the runner still holds it and will \
         redeliver it. Make the state root writable."
    );
    // The record will go on reading `running`, or `assigned`, with nothing
    // happening to it at all. Whoever is waiting for this job hears why, and
    // so does whoever waits for it later: being mute about a job that is
    // going nowhere is not a policy (D19).
    say_it_is_stalled(state, job_id, why);
    false
}

/// Keep the reason a job cannot be written down, and tell whoever is waiting.
fn say_it_is_stalled(state: &State, job_id: &JobId, why: String) {
    let note = stalled_note(&why);
    for follower in state.note_a_stall(job_id, why) {
        let _ = follower.send(FromHub::JobNote {
            job_id: job_id.clone(),
            note: note.clone(),
        });
    }
}

/// What a wait is told about a job the hub cannot write down.
///
/// Neither a refusal nor a verdict: the result exists, the runner is holding
/// it, and this hub cannot put it on its disk (D19). How long a hub tries
/// before leaving it there is M3's policy; saying nothing at all never was
/// one. One sentence in one place, so a test can rely on it.
fn stalled_note(why: &str) -> String {
    format!(
        "this job is not over and the hub cannot write it down: {why}. \
         Nothing is happening to its record until that is fixed; the runner \
         holds the result and redelivers it on its next connection."
    )
}

async fn serve_requester(state: &State, outbox: &Outbox, source: &mut Source) -> Result<()> {
    let outcome = accept_submissions(state, outbox, source).await;
    state.forget_follower(outbox);
    outcome
}

async fn accept_submissions(state: &State, outbox: &Outbox, source: &mut Source) -> Result<()> {
    while let Some(message) = next_message(source).await? {
        match message {
            ToHub::SubmitJob { command } => {
                if command.is_empty() {
                    let _ = outbox.send(FromHub::Refused {
                        reason: "a job needs a command to run".to_owned(),
                    });
                    return Ok(());
                }
                let Some(runner) = state.runner() else {
                    let _ = outbox.send(FromHub::Refused {
                        reason: "no runner is connected".to_owned(),
                    });
                    return Ok(());
                };
                let job = state
                    .store
                    .create_job(command.clone())
                    .context("minting a job")?;
                // JobAccepted first, so the requester has the id before any
                // chunk of its log can reach it.
                let _ = outbox.send(FromHub::JobAccepted {
                    job_id: job.id.clone(),
                });
                state.follow(job.id.clone(), outbox);
                state
                    .store
                    .record_state(&job.id, JobState::Assigned)
                    .with_context(|| format!("recording {} as assigned", job.id))?;
                if runner
                    .send(FromHub::AssignJob {
                        job_id: job.id.clone(),
                        command,
                    })
                    .is_err()
                {
                    state.stop_following(&job.id);
                    let _ = outbox.send(FromHub::Refused {
                        reason: "the runner disconnected before the job was assigned".to_owned(),
                    });
                    return Ok(());
                }
            }
            ToHub::WaitForJob { job_id } => {
                let attached = state
                    .attach(&job_id, outbox)
                    .with_context(|| format!("reading the record of {job_id}"))?;
                let Some(Attached {
                    record,
                    log,
                    stalled,
                }) = attached
                else {
                    // A terminal no, as D13 means it: this id will not come
                    // into being by waiting for it.
                    let _ = outbox.send(FromHub::Refused {
                        reason: format!("no such job: {job_id}"),
                    });
                    return Ok(());
                };
                // The output the job has already produced, before anything
                // else about it. A detached requester has no other way to get
                // a job's log through the CLI (D22).
                if !log.is_empty() {
                    let _ = outbox.send(FromHub::JobLog {
                        job_id: job_id.clone(),
                        data: log,
                    });
                }
                let metrics = record.metrics.clone();
                match record.state {
                    // Answered from the record, which is the point of writing
                    // one: a wait works an hour later, on another machine,
                    // across a restart of the hub. The measurements come from
                    // the record too, so a wait an hour later reads the same
                    // figures the requester that followed the job read (E-15).
                    JobState::Finished { outcome } => {
                        let _ = outbox.send(FromHub::JobFinished {
                            job_id,
                            outcome,
                            metrics,
                        });
                    }
                    // Waiting for a job whose attempt was interrupted is
                    // waiting for a runner that may never come back, so the
                    // wait says so rather than being silent (D17). It is not a
                    // refusal: the runner may still redeliver, and then this
                    // same connection carries the result.
                    JobState::Interrupted { reason } => {
                        let _ = outbox.send(FromHub::JobNote {
                            job_id,
                            note: format!(
                                "this job is not over and its last attempt was \
                                 interrupted: {reason}. If the runner comes back, \
                                 its result arrives here."
                            ),
                        });
                    }
                    // Queued, assigned or running — and if this hub knows that
                    // the attempt is not progressing because it cannot write
                    // the job down, the wait is told so rather than left
                    // watching a record that says `running` and never moves.
                    _ => {
                        if let Some(why) = stalled {
                            let _ = outbox.send(FromHub::JobNote {
                                job_id,
                                note: stalled_note(&why),
                            });
                        }
                    }
                }
            }
            other => {
                let _ = outbox.send(FromHub::Refused {
                    reason: format!("a requester may not send {}", message_kind(&other)),
                });
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Pump one connection's outbox onto its socket, then close it.
async fn write_frames(mut sink: Sink, mut inbox: UnboundedReceiver<FromHub>) {
    while let Some(message) = inbox.recv().await {
        match to_frame(&message) {
            Ok(frame) => {
                if sink.send(Message::text(frame)).await.is_err() {
                    break;
                }
            }
            Err(error) => eprintln!("herd-hub: a message would not encode: {error}"),
        }
    }
    let _ = sink.close().await;
}

async fn send(sink: &mut Sink, message: &FromHub) -> Result<()> {
    let frame = to_frame(message).context("encoding a message")?;
    sink.send(Message::text(frame)).await.context("sending")
}

/// Say no, in one frame, and close. Refusal is never silent.
async fn refuse(sink: &mut Sink, reason: String) -> Result<()> {
    send(sink, &FromHub::Refused { reason }).await?;
    let _ = sink.close().await;
    Ok(())
}

/// The next protocol message, or `None` once the peer is done.
async fn next_message(source: &mut Source) -> Result<Option<ToHub>> {
    while let Some(frame) = source.next().await {
        match frame.context("reading a frame")? {
            Message::Text(text) => {
                let message = from_frame(text.as_str())
                    .map_err(|error| anyhow!("undecodable frame {text}: {error}"))?;
                return Ok(Some(message));
            }
            Message::Close(_) => return Ok(None),
            Message::Binary(_) => bail!("the protocol carries text frames only (D4)"),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        }
    }
    Ok(None)
}

/// A message's wire tag, for refusal texts.
fn message_kind(message: &ToHub) -> &'static str {
    match message {
        ToHub::Hello { .. } => "hello",
        ToHub::SubmitJob { .. } => "submit_job",
        ToHub::WaitForJob { .. } => "wait_for_job",
        ToHub::JobStarted { .. } => "job_started",
        ToHub::LogChunk { .. } => "log_chunk",
        ToHub::JobFinished { .. } => "job_finished",
    }
}
