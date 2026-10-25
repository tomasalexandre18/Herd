# Status — 2026-10-25

State now. Replaced every session, never appended to. Detail is in
`docs/coverage.md`, reasoning in `docs/decisions.md`, the audit in
`docs/remediation/`, open items in `docs/orchestration/board.md`.

## Who operates what

**The first consumer operates the running system** (D161, invariant 9):
the hub container and its volume, the runner units and their data roots,
the capability files and probes, the tokens, the disk, the version pins.
Ours is what a tag produces: protocol, crates, `deploy/`, releases, docs.

## What is running, and how old the reading is

Nothing in this project has looked at the consumer's hosts since
**2026-10-23**, and nothing here may (D161). Those are that day's figures.

- Last seen 2026-10-23: hub **`herd-hub:0.3.3`**, server runner **0.3.2**
  pinned declaratively at `/etc/nixos/flake.nix` line 15.
- Recorded rather than observed: **v0.3.6 reached the consumer's only
  runner on 2026-10-24 and regressed it; v0.3.7 undid it** the same day
  (D172). That runner is past 0.3.2; its version, and the hub's, are
  **not known here**.
- Herd is the consumer's only path: a job Herd cannot run fails their
  build. Long jobs, KVM and exclusive `rig-cpu` worked on 2026-10-23.
- On their disk 2026-10-23: about **90 GB across 1 571 jobs** against
  D152's 200 GB budget, the sweeper having **never evicted anything**,
  plus **7.4 GB of runner spool no runner can remove** (D158).

## Releases

**v0.3.7 is the newest** (2026-10-24); v0.3.4 onward all published, the
credential preflight that killed v0.3.1-v0.3.3 being gone (D162). A
**`v*.*.*-dev*` tag** publishes a pre-release through `release-dev.yml`
in four to six minutes, built exactly as a real tag builds.

## What `main` carries

M1-M11, M15 and M16; protocol version 2; a job may return 1 GiB. The four
Rust gates and `check-docs.sh` are green, with **12 loud skips** (D27):
**10** because cgroup v2 delegation is unavailable in this session's
scope, **2** because the data root is on ZFS and no hard `io.max` can be
carried — both in `herd-runner/src/confine.rs`. Measured here 2026-10-25;
D151's 13 is **not reproducible** (`05-experiments-review.md` §6).

**Two defects the audit found are fixed** (2026-10-25): **D175**, a
verdict carrying an inline file whose bytes did not hash to the digest
beside them exited 0 and said nothing was missing; **D176**, a verdict
could name a commit the tree was never at, and a runner now reports the
revision it really ran. `PROTOCOL_VERSION` 2 and `RECORD_VERSION` 1 both
unchanged.

`docs/remediation/` holds ten pieces: the baseline, audits A and B, the
contradictory review, the two experiments and their adversarial review,
`06` (one junction in five posts), `07` (target architecture, recommended
and **not** adopted), `08` (the unsynced-write cost).

## What is not done, and why

1. **The reaper's janitor list is unbuilt** (D134), the last of the
   consumer's four blocking items: a failing `up` leaves workspaces
   behind and Herd records only that the `down` step failed.
2. **A requirement reading still weighs nothing**: it declares no memory
   and no tmpfs (`herd-hub/src/matcher.rs:421`). D169 settled the other
   half: how many run at once is bounded, a lost one is read again.
3. **M11's pack half does not exist** — no registry or pack frame on
   either protocol enum — so E-12 and E-17 are undelivered; D161 cut it
   and `coverage.md` has not been restated (ORCH-020).
4. **Durable writing is 99.5 % of a runner junction** at `N` = 1 000 —
   9.633 s of 9.685 s (`06`), device-bound. The four whole-volume walks
   are real and **nothing ever deletes a job directory**, but the worst
   is 0.16 % of that window: 04a's "walk first" was reversed by `05` and
   measured away by `06` (ORCH-047, rewritten).
5. **The hub has no stop path**, and invariant 2 is enforced in the hub
   only, not at the runner (`03b`).
6. **M17's second machine is prepared, not joined** (D149): the unknown
   is whether a WSL2 user scope gets a delegated subtree with `memory`.
7. **One shared token**: any requester may ask for the merger level, and
   `wss://` is configured and never dialled.
8. **Two tests are known unstable**, product side and untouched:
   `herd-hub/tests/slots.rs:206` and `follower_alive.rs:230`.

## Next action

**Two questions for Alexa, in this order.** ORCH-052: may a power cut cost
a stale wait explanation, between a hub restart and its first pass? It is
the one claim `07`'s recommendation rests on. ORCH-053: which filesystem
holds the hub's state root on the server? Once the `fsync`s go, D103's
ordering is the filesystem's — safe on ZFS, not guaranteed elsewhere.
