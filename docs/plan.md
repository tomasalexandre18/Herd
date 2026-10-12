# Plan

Short milestones. Each one is deliverable and testable in a single session,
and ends with `cargo test --workspace` green plus `docs/status.md` updated.
One branch per milestone. `main` moves only on Alexa's approval.

The chatOnS requirement sheet arrives as a GitHub issue; it may reorder M4–M6
but should not invalidate M1–M3.

---

## M0 — Method (done)

Working agreement, decisions, plan. No code.
**Done when:** `CLAUDE.md`, `docs/status.md`, `docs/decisions.md`,
`docs/plan.md` exist and Alexa has approved the stack.

## M1 — One command, end to end

Branch `m1-end-to-end`. The smallest thing that is really Herd.

- Nix flake with a dev shell (rust stable, clippy, rustfmt) and the cargo
  workspace with the four crates.
- `herd-proto`: `Hello`/`Welcome` handshake with a protocol version,
  `JobRequest`, `JobAccepted`, `LogChunk`, `JobFinished`.
- `herd-hub`: accepts one runner and one requester, dispatches the job to the
  single connected runner, relays log chunks, returns the outcome.
- `herd-runner`: executes the command, streams stdout and stderr, reports the
  exit code.
- `herdctl run -- <cmd>`: prints the logs as they arrive, exits with the
  job's exit code.

**Done when:** an in-process end-to-end test starts hub + runner + requester,
runs `echo hello`, asserts the output and exit code 0, and a second case
asserts a non-zero exit code propagates to `herdctl`. Plus protocol
round-trip unit tests and a version-mismatch rejection test.

## M2 — The link survives reality

Branch `m2-link-robustness`.

- Runner reconnects with bounded backoff; heartbeat with ping timeout both
  ways.
- Cancellation: requester interrupt → hub → runner kills the process tree.
- Graceful drain: the runner finishes or abandons, then exits in ≤ 5 s.
- A runner dying mid-job marks the job failed with a readable reason instead
  of hanging the requester.

**Done when:** tests kill the runner mid-job, cancel a long job, and drain a
busy runner, each asserting the requester gets a clear terminal outcome within
a bounded time.

## M3 — The runner is a good guest

Branch `m3-host-safety`. This is where invariant 2 becomes real.

- Concurrency limit per runner.
- Hard memory cap per job (cgroup v2 scope), with an explicit, logged fallback
  when delegation is unavailable — WSL to be verified here.
- Wall-clock timeout per job, log size cap, per-job workspace directory that
  is cleaned up.

**Done when:** a job that allocates past its cap is killed and reported as
`OutOfMemory` without the host swapping, a job that overruns is reported as
`Timeout`, and the runner's own memory stays flat while streaming a large log.

## M4 — Choosing a machine

Branch `m4-scheduling`. Needs the requirement sheet for the policy details.

- Runners declare capabilities: memory, CPUs, KVM, GPU, named exclusive
  hardware.
- Jobs declare requirements and a priority; the hub queues and matches.
- Exclusive resources are leased, never double-dispatched.

**Done when:** tests cover requirement filtering, priority ordering, exclusive
leasing under contention, and a job whose requirements no runner can ever
satisfy failing fast with a clear reason.

## M5 — Really deployed

Branch `m5-deploy`.

- Flake packages the hub as a container image; docker compose file; `wss://`
  through the existing reverse proxy; bearer-token auth from a file.
- Runner on the NixOS server (systemd user service) and on the WSL PC.
- `herdctl` on the PC against the real hub.

**Done when:** a job submitted from the PC runs on the server and returns its
logs and exit code over `wss://`, and killing the WSL runner mid-job behaves
as M2 says it should.

## M6 — The chatOnS job shape

Branch `m6-commit-jobs`. Driven by the requirement sheet.

- A job names a repository and a commit; the runner obtains that commit.
- Build / test / check entry points.
- Result and logs retrievable after the requester has disconnected.

**Done when:** a real chatOnS commit is built on the server from a request
made on the PC, and the result survives the requester disconnecting.
