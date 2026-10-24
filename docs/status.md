# Status — 2026-10-25

State now. Replaced every session, never appended to. Detail is in
`docs/coverage.md`, reasoning in `docs/decisions.md`, the audit in
`docs/remediation/`, open items in `docs/orchestration/board.md`.

## Who operates what

**The first consumer operates the running system** — the hub container and
its volume, the runner units and their data roots, the capability files
and probes, the tokens, the disk and the version pins (D161, invariant 9).
Ours is what a tag produces: the protocol, the four crates, `deploy/`, the
releases and these docs.

## What is running, and how old the reading is

Nothing in this project has looked at the consumer's hosts since
**2026-10-23**, and nothing here may (D161). Those are that day's figures.

- Last seen 2026-10-23: hub **`herd-hub:0.3.3`**, server runner **0.3.2**
  pinned declaratively at `/etc/nixos/flake.nix` line 15.
- Later, recorded rather than observed: **v0.3.6 reached the consumer's
  only runner on 2026-10-24 and regressed it; v0.3.7 undid it** the same
  day (D172). That runner is past 0.3.2; its exact version, and the
  hub's, are **not known here**.
- Herd is the consumer's only path: they removed their fallback, so a job
  Herd cannot run fails their build on purpose. Long jobs, KVM behind a
  busy probe and exclusive `rig-cpu` behaved as designed on 2026-10-23.
- Last measured on their disk, 2026-10-23: about **90 GB across 1 571
  jobs** against D152's 200 GB budget, the sweeper having **never evicted
  anything**; about **7.4 GB of runner spool no runner can remove**
  (D158), their jobs depositing trees under a foreign uid.

## Releases: the lane works, and there is a fast one

**v0.3.7 is the newest release** (2026-10-24), and v0.3.4 through v0.3.7
all published: the credential preflight that killed v0.3.1-v0.3.3 is gone
(D162) and D163-D165 are in a tag. A **`v*.*.*-dev*` tag on any branch**
now publishes a pre-release through `release-dev.yml`, four to six minutes
from push to a downloadable file; it calls `release.yml`, so it builds
what a real tag builds, and trades away the GitHub-side gate (D172).

## What `main` carries

M1-M11, M15 and M16; protocol version 2; a job may return 1 GiB. The four
Rust gates and `check-docs.sh` are green, with **12 loud skips** (D27):
**10** because cgroup v2 delegation is unavailable in this session's
scope, **2** because the data root is on ZFS and no hard `io.max` can be
carried. Those two are
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`
and `a_declared_ceiling_changes_what_the_inert_weight_line_says`, both in
`herd-runner/src/confine.rs`. Measured here 2026-10-25. D151 recorded 13
on 2026-10-16; that figure is **not reproducible** and stays an open
unknown rather than an error to rewrite (`05-experiments-review.md` §6).

`docs/remediation/` now holds the baseline, audits A and B, the
contradictory review, the two experiments and the review of them. **It is
a reading and a measurement, not a work order**: nothing in it authorizes
a change to the software. Its findings are on the board as `ORCH-04x`.

## What is not done, and why

1. **The reaper's janitor list is unbuilt** (D134), the last of the
   consumer's four blocking items: a failing `up` leaves workspaces
   behind and Herd records only that the `down` step failed.
2. **A requirement reading still weighs nothing**: it declares no memory
   and no tmpfs (`herd-hub/src/matcher.rs:421`). D169 settled the other
   half — how many run at once is bounded (`--readings-at-once`,
   default 2), and a lost one is read again.
3. **M11's pack half does not exist** — no registry or pack frame on
   either protocol enum — so E-12 and E-17 are undelivered; D161 cut it
   and `coverage.md` has not been restated (ORCH-020).
4. **The hub walks the whole volume on four hot paths** — D152's 200 ms
   poll, the runner junction's `collections_owed`, `sweep_outputs` and
   store startup, all O(jobs ever run), and **nothing ever deletes a job
   directory**. Worst turn measured at 815 ms, 20 000 jobs (`04a`).
5. **The hub has no stop path**, and invariant 2 is enforced in the hub
   only, not at the runner (`03b`).
6. **M17's second machine is prepared, not joined** (D149): the unknown
   is whether a WSL2 user scope gets a delegated subtree with `memory`.
7. **One shared token**: any requester may ask for the merger level, and
   `wss://` is configured and never dialled.
8. **Two tests are known unstable**, product side and untouched:
   `herd-hub/tests/slots.rs:206` and `follower_alive.rs:230`.

## Next action

Alexa's word on the dossier's first question (ORCH-047): take the `T`
walk out of `collections_owed` before any executor change (`03c §7`,
`04a`). Then the janitor list (D134), then M17's join.
