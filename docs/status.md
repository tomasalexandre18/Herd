# Status — 2026-10-13

State of the project right now. Replaced every session, never appended to.

## Where we are

**M1 is written and green on branch `m1-end-to-end`, awaiting a merge.**
CI and the pull-request flow are live on `main`. The first consumer's
requirement sheet (issue #1) is mapped end to end in `docs/coverage.md`.

## What works

Verified by the orchestrator, not taken on report: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` all green through `nix develop` — 13 tests, no
failures. Driven by hand with the three real binaries: a job streams its
output incrementally, exit code 0 and exit code 7 both reach `herdctl`
unchanged, and neither a process nor a bound port is left behind.

- A hub accepting a runner and requesters over `ws://`, dispatching a job,
  relaying its log and its outcome.
- A runner that dials out, executes, streams stdout and stderr as produced.
- `herdctl run -- <cmd>` printing the log live and wearing the job's code.
- `herd-proto` v1: two tagged enums (`ToHub`, `FromHub`), a versioned `Hello`,
  per-job log sequence numbers, framing in the contract crate.
- A `JobStore` trait with a memory implementation, so M2 swaps in disk.
- CI on every pull request: `check` (nix, fmt, clippy, test) and `docs` (the
  wrapping and length rules, and every `must` in `coverage.md` having a home).
  `main` requires a green pull request; direct pushes are a discipline, not a
  barrier, by Alexa's choice (`enforce_admins` is off).

## What is not done, and why

- **One finding to settle before merging M1** (see "Next action"): the
  protocol reports a job's end as a bare `exit_code: i32`, with 127 meaning
  "could not start" and -1 meaning "signalled". Sentinel numbers inside a
  field that must carry the consumer's own codes untouched (E-02 relies on 0,
  1 and 75) will collide, and M3's "lost", M9's "cancelled (deadline)" and
  M8's refusals all need to live there too.
- **M1's own limits, by design.** One runner at a time; a restarted runner is
  refused until the hub's socket dies (M3's heartbeat fixes it); no
  reconnection, no persistence, no capabilities, no admission, and the
  handshake's `token` is carried but not validated.
- **The requester never learns that a job started** — `JobStarted` is
  runner-to-hub only. Fine for now, worth knowing before M13's status.
- **Nothing is deployed.** First exposure over `wss://` is M16, so that the
  public formats do not freeze on a surface still moving.
- **Q10, the licence**, is still Alexa's to decide; it blocks nothing before
  M16.

## Environment facts

- Server: NixOS, kernel 6.18, i5-10600K, 12 threads, 31 GB RAM, no swap,
  `/dev/kvm`, Intel iGPU at `/dev/dri/renderD128`, nix with flakes, docker
  29.7. Rust 1.98.1 comes from the flake (D12); nothing is installed globally.
- GitHub: `tomasalexandre18` via `gh`. `main` protected (a pull request,
  `check` + `docs` green, no force push, no deletion). Issue #1 open and
  unanswered — replying is outward-facing and waits for Alexa.
- This machine's SSH key is a deploy key for another repository; git uses the
  HTTPS remote with the `gh` credential helper.

## Next action

Alexa rules on the exit-code finding above. Recommended: fix it inside M1's
pull request — replace the bare exit code with an outcome (`exited`,
`signalled`, `could-not-start`) so later states have somewhere to go and the
consumer's codes stay untouched. Then merge M1 and start **M2** on branch
`m2-job-store`: the hub's state on disk, a directory per job (D8).
