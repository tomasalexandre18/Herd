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

#[derive(Default)]
struct Peers {
    /// M1 accepts exactly one runner. M5 onwards holds a set.
    runner: Option<Outbox>,
    /// Who to relay a job's logs and outcome to. More than one is possible
    /// from M2: a `herdctl run` follows the job it submitted, and a
    /// `herdctl wait` on the same id attaches beside it rather than taking
    /// its place.
    followers: HashMap<JobId, Vec<Outbox>>,
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

    fn followers(&self, id: &JobId) -> Vec<Outbox> {
        self.with_peers(|peers| peers.followers.get(id).cloned().unwrap_or_default())
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
    dispatch_recovered_jobs(state, outbox)?;
    let outcome = relay_from_runner(state, outbox, source).await;
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
                state
                    .store
                    .record_state(&job_id, JobState::Running)
                    .with_context(|| format!("recording {job_id} as running"))?;
            }
            ToHub::LogChunk {
                job_id,
                stream,
                seq,
                data,
            } => {
                state
                    .store
                    .append_log(&job_id, stream, seq, &data)
                    .with_context(|| format!("recording a log chunk of {job_id}"))?;
                for follower in state.followers(&job_id) {
                    let _ = follower.send(FromHub::LogChunk {
                        job_id: job_id.clone(),
                        stream,
                        seq,
                        data: data.clone(),
                    });
                }
            }
            ToHub::JobFinished { job_id, outcome } => {
                // Recorded first, acknowledged second. The runner is holding
                // this result until the acknowledgement arrives, so the order
                // of these two steps is the whole of E-44's promise: nothing
                // tells the runner to forget a result that is not yet on a
                // disk.
                state
                    .store
                    .record_state(
                        &job_id,
                        JobState::Finished {
                            outcome: outcome.clone(),
                        },
                    )
                    .with_context(|| format!("recording the outcome of {job_id}"))?;
                let _ = outbox.send(FromHub::JobResultAck {
                    job_id: job_id.clone(),
                });
                for follower in state.stop_following(&job_id) {
                    let _ = follower.send(FromHub::JobFinished {
                        job_id: job_id.clone(),
                        outcome: outcome.clone(),
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
                let record = state
                    .store
                    .job(&job_id)
                    .with_context(|| format!("reading the record of {job_id}"))?;
                let Some(record) = record else {
                    // A terminal no, as D13 means it: this id will not come
                    // into being by waiting for it.
                    let _ = outbox.send(FromHub::Refused {
                        reason: format!("no such job: {job_id}"),
                    });
                    return Ok(());
                };
                match record.state {
                    // Answered from the record, which is the point of writing
                    // one: a wait works an hour later, on another machine,
                    // across a restart of the hub.
                    JobState::Finished { outcome } => {
                        let _ = outbox.send(FromHub::JobFinished { job_id, outcome });
                    }
                    _ => state.follow(job_id, outbox),
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
