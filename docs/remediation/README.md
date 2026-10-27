# The remediation dossier — index

The audit dossier required by §10 of
`MANDAT_HERD_AUDIT_ARCHITECTURE_DECISION_2026-10-24`. One short page per
piece, written separately; no monolithic journal.

**Base read by every piece:** `origin/main` at `0a2cf73`. That commit
carries **no tag**; tag `v0.3.7` is `3da48f5`, two merges earlier. No Rust,
`Cargo.*` or Nix file differs between the two, so the product audited here
is `v0.3.7`'s product — see `00-baseline.md §2`.

**Date of this index:** 2026-10-25, refreshed at the C0b integration and
again at the integrations of PR #90 (`11`) and PR #91 (`10`).

---

## The pieces

| Piece | Branch | What is in it, and what it settles |
|---|---|---|
| [`00-baseline.md`](00-baseline.md) | `remediation-dossier-index` | The reference state at `0a2cf73`: what Herd is, the four crates and their sizes, the four Rust gates plus `check-docs.sh` and `check-orchestration.sh`, the loud-skip machinery (D27), `PROTOCOL_VERSION` 2 and `RECORD_VERSION` 1 with the three `!=` checks that block the move up *and* the move back, the nine repository invariants, the two known-unstable tests, and the two real incidents with their figures. Settles **what commit was audited and what it contained**, and separates the mandate's label `v0.3.7` from the base `0a2cf73`. |
| [`03a-identity-and-storage.md`](03a-identity-and-storage.md) | `audit-identity-storage` | Audit A, mandate §4.1: who owns which state and where it persists, the job/attempt automaton and its commit points, what `JobResultAck` / `LogAck` / `JobFilesAck` each actually guarantee, the identity chain from a requester's commit to an artefact, and six integrity risks. Settles that the hub's durability discipline is real and is one module (`durable.rs`); that Herd is **at-least-once execution with at-most-once recording** and claims nothing stronger; that the **commit link of the identity chain is open-loop** — declared on the way out, never confirmed on the way back; and that one inline path can let a short collection read as whole. |
| [`03b-concurrency-and-progression.md`](03b-concurrency-and-progression.md) | `audit-concurrency-progression` | Audit B, mandate §4.2: the hub's tasks and locks, where a blocking call is made from an async context, why a deep queue or a slow store stops progression, backpressure, fairness and starvation, stop/drain/restart, host failure modes, both incidents re-examined, and the two unstable tests. Settles that the hub's **persistence and its scheduler share the reactor with nothing bounding a pass**; that a pass is O(queue depth) in reads and runs once per event; that invariant 2 is enforced only in the hub; that the hub has **no stop path**; and that one unwritable record aborts a whole dispatch pass. |
| [`03c-contradictory-review.md`](03c-contradictory-review.md) | `audit-contradictory-review` | The contradictory review of A and B, by a third reader who wrote neither: every **F** in both reports taken back to the code at `0a2cf73`. Settles that the "frontal contradiction" between A and B on D171 **does not exist** — the two were set against each other on a misreading; that the mandate's base label is wrong; that all of A's and B's checked findings hold, two of them more strongly than stated and one citation off by 39 lines; and it names the blind spot the A/B split created. Ends in one ranked list across both audits and a minimal four-run experiment sequence. |
| [`04a-cost-surface.md`](04a-cost-surface.md) | `experiment-cost-surface` | Experiment 6.1: of `N` (open jobs), `T` (closed job directories) and `W` (workers), which one the cost of a dispatch pass follows. Measured, not read. Settles that `T` is walked on a runner junction and that the walk is O(whole volume) — 815 ms worst turn at `T` = 20 000 against 33 ms at `N` = 1 000. **Read it with `05`:** that 25x compares the `T` walk against the `N` *reads only*, and leaves out the durable writes that are the larger half of the same junction. Two of its four recommendations did not survive review. |
| [`04b-integrity-and-containment.md`](04b-integrity-and-containment.md) | `experiment-integrity-containment` | Experiment 6.3: the inline digest drop and a runner's admission under flood. Settles that a verdict carrying an inline file whose bytes do not match its sha256 exited **0** while saying nothing was missing — the defect C2 later fixed — and sets out the `#[serde(default)]` argument that a new `StepRun` field needs neither a protocol nor a record break, which C3 later used and proved. |
| [`05-experiments-review.md`](05-experiments-review.md) | `review-experiments` | Adversarial review of 04a and 04b by a third reader who ran both again. **This is the piece that reversed 04a's ranking**: counting the durable writes a pass ends with — 8.3 s and 9.2 s for 1 000 of them, against 13 ms for the two reads and ~200 ms for the whole `T` = 20 000 walk — the order reverses by about an order of magnitude. Also: 04a's recommendation 1 cannot be implemented as written (an owed collection is on a *finished* job, which `unfinished_jobs` never holds), recommendation 4 is refuted by measurement, and one O(`T`) walk 04a never named. |
| [`06-junction-breakdown.md`](06-junction-breakdown.md) | `measure-junction-breakdown` | C1: the whole cost of one runner junction, split into the five posts Alexa asked for — historical reads, open-job reads, durable writes, lock wait, dispatch compute — summing to the window by construction. Settles the ranking `05` reversed, with a number on each post: **durable writing is 68–99.5 % of a junction**, the two reads together are under 0.3 % once the queue is deep, dispatch compute is a fifth of a percent, and lock wait is zero until a second runner dials — at which point it is the whole of the first junction's writes. Device-bound and proved so: debug against release moves the three CPU posts by ×3.7–×8.0 and the write post by ×1.00. Touches no file under `herd-hub/src`. |
| [`07-target-architecture.md`](07-target-architecture.md) | `arch-target-decision` | C4: six architectures compared on durability, progression, crash recovery and migration cost, with one named recommendation — "the keeper": one owned thread performs every store mutation, the reactor wakes it by message and never waits on a disk, its unit of work is bounded and re-armed, and the hub's writes split into two durability tiers. **It is a recommendation and not an adoption.** Its load-bearing claim — that `waiting_for` has no crash-durability requirement — is a question for Alexa, board row ORCH-052. |
| [`08-s0-unsynced-write-cost.md`](08-s0-unsynced-write-cost.md) | `s0-unsynced-write-cost` | S0: what those durable writes would cost if they were not synchronous — the measured size of the prize behind the tiering. Also names the one ordering invariant that must hold before any `fsync` is dropped: D103 forbids `events.jsonl` being ahead of `job.json`, the `fsync`s hold that today, and without them it is the filesystem's to hold. Safe on ZFS, whose transaction groups commit atomically and in order; **not guaranteed elsewhere**, which is why the server's state root must be named — board row ORCH-053. |
| [`10-ordering-on-ext4.md`](10-ordering-on-ext4.md) | `plan-orch-074` | ORCH-074: the mechanism that holds D103's ordering — `events.jsonl` never ahead of `job.json` — on **ext4**, where `08`'s ZFS argument does not transfer. Settles that **D103 needs no new ordering mechanism**, because the writes `07` de-syncs are not the writes D103 binds: `JobEvent::state()` is `None` for `Waiting`, `move_to` and `raise` already leave `events.jsonl` naming a reason the record does not, `recover` never reads `events.jsonl`, `job_events` already drops a torn tail, and timestamps are not comparable by decision. What holds the order for a **decisive** write is the record's own two syncs returning before the event append is issued — a syscall-return guarantee, independent of the filesystem. **A larger hazard was found in its place:** a `job.json` that comes back empty or torn makes the hub refuse to start on the *whole* state root, D52's quarantine covering labels only. ext4's cross-file ordering could not be established from documentation on this host and is marked `I`; the plan is built so as not to need it. **Amended against `11-sync-prices.md`, which refuted the plan's own hypothesis H-1:** the single durability tier splits into three independent steps — **A** the event `fdatasync` (~31 %, needing no crash evidence of any kind), **B** the directory `fsync` (~65 % cumulative), **C** the record `fsync` (~98 %, deferred and possibly never taken) — and the directory sync's standing *rises* rather than falls: only it makes the record's **rename** durable, so on a decisive write it stays unconditionally. Produces the ordered task list ORCH-079 to ORCH-090. Touches no file under `herd-hub/src`. |
| [`11-sync-prices.md`](11-sync-prices.md) | `orch-077-price-the-syncs` | ORCH-077: the three durable operations of `record_waiting` — the record `fsync`, the directory `fsync` and the event `fdatasync` — priced **separately**, in all eight combinations, on **both** filesystem classes in one session on one host. Settles that one `fsync` on ext4/NVMe costs **989 µs** and not the tens-to-low-hundreds of microseconds that was hoped, so the class the deployment runs on is **1.2x** faster per sync than the 1.19 ms `08` derived, and 1.3-1.7x faster than this harness's own ZFS figures — not 10-100x; that the three syncs are **additive, not redundant**, so keeping one and dropping two collects **65 %** of the prize and not nearly all of it, the kept sync still costing 21x the unsynced floor; and that `08`'s **9.970 s** junction at `N` = 1 000 becomes **3.128 s** on ext4 — 3x smaller, not an order of magnitude, and still seconds. Corrects `06`'s `sync_dir` figure of 6.6 µs, which was measured with no `rename` between the file sync and the directory sync and so understates the real directory sync by about two hundred times. Touches no file under `herd-hub/src`. |

The mandate's §10 skeleton also names `01-contract.md` and
`02-consumer-boundary.md`. They do not exist yet. The numbering above
keeps their places rather than renumbering what is written, so a gap in
this table means *not written*, never *lost*.

**`09` is the one number absent from this table.**
`09-runner-self-limits.md` is written and is on an unmerged branch
(`runner-holds-its-own-limits`, PR #85); it gets its row when that
branch lands. **`10` was unclaimed on every ref until PR #91**, which is
why `11` was written and merged before it; the two are now both here and
the table is in number order rather than in the order they landed.
Whether `09` is then reused deliberately a second time, or `10` stands
as written, is **ORCH-091**.

**One number was reused, deliberately.** The skeleton's `05` was
`05-invariants.md`; the `05` that exists is the adversarial review of the
two experiments, and `06`, `07` and `08` follow the order the work was
actually done in rather than the skeleton's. The skeleton's invariants
piece is unwritten and now has no number reserved for it.

---

## How to read the dossier

**The dossier is now half constat and half mesure, and each piece says
which it is.** It did not start that way: until 2026-10-25 nothing here
had been run.

- **Constats** — `00-baseline.md`, `03a`, `03b`, `03c`. Every one of them
  was produced by reading the repository at `0a2cf73`. No build, no test,
  no experiment, no load on any host, no process touched. Where one of them
  carries a figure from a running system — the 214 jobs, the 255 cgroups,
  the 775 files, the 20 683 `stays queued` lines, the 1.8-3.9 % of a CPU —
  that figure was **measured by the first consumer or on the host D171 was
  written on**, and the document says so at the point of use. It is
  evidence of an incident; it is not a measurement any of these authors
  made.
- **Mesures** — `04a`, `04b`, `05`, `06`, `08`, `11`. These ran on the shared
  workstation, each in its own worktree, under `nix develop`, with `-j 4`
  and `/proc/loadavg` watched. They are not an isolated host, which
  `00-baseline.md §11` asked for and which this campaign never got; every
  one of them characterises its own noise before reporting its signal and
  names the limits of what it reproduces. Read a figure from any of them
  with the host in mind.
- **Neither** — `07`. It is a design comparison and a recommendation. It
  ran nothing and it changed nothing; every figure in it was measured by
  `04a`, `05` or `06` and is attributed there.

**Where a later piece overturns an earlier one, the later one wins, and
the earlier one is not edited.** This has now happened twice. The second
time is narrower than the first: `11` found that `06`'s `sync_dir` figure
of 6.6 µs was measured with no `rename` between the file sync and the
directory sync, so the file sync had already committed the directory's
only change and the directory sync found nothing owed. The real
`write_file_with` renames *between* the two, so its `sync_dir` always has
dirty metadata and always pays a flush — 1.32 ms on ZFS and 991 µs on
ext4, about two hundred times `06`'s figure. `06` still reads as it was
signed, and `08` recorded the resulting discrepancy without diagnosing
it. The first time is the one that reordered the work:
`04a` ranked the `T` walk as the dominant cost of a dispatch pass, `05`
showed that ranking left out the durable writes that are the larger half
of the same junction, and `06` measured all five posts and put the walk at
0.16 % of the window against 99.5 % for the writes. `04a` still reads as
it was signed. The board's ORCH-047 carries the corrected conclusion, and
a reader who takes 04a's recommendations without `05` and `06` beside them
will reach the wrong order of work.

**The three marks, used throughout.** **F** = observed at `0a2cf73`, with a
path and a line, a test name, or a decision entry. **H** = a hypothesis the
code suggests and nothing here settles. **I** = unknown. A figure nobody
verified is marked **I** rather than repeated; `00-baseline.md §5` is the
worked example — the repository stated both 12 and 13 loud skips and the
honest answer at the time was that no one here had run the suite. It has
been run since, repeatedly: **12**, split 10 cgroup-v2 delegation and 2
ZFS ceiling. D151's 13 is not reproducible on this host.

**Where the pieces disagree, `03c` is the arbiter of fact** — it re-checked
both of the others against the code — but it is not an arbiter of what to
do. Where a question is open, it stays open and is listed as such
(`03a §9`, `03b §11`, `03c §8`).

---

## What this dossier does and does not authorize

**No piece of this dossier authorizes a modification of the software.**
The pieces are readings and measurements. No finding here is a work order,
no ranked list is a backlog, and no recommendation — `03c §7`'s or `07`'s
— becomes a plan by being written down.

Every change — to the code, to the protocol, to the gates, to the
architecture — is **Alexa's decision, one at a time, each with its own
recommendation and its own consequences**, as `CLAUDE.md` and the mandate
both require. The audit's job is to make each of those decisions
answerable; it is not to pre-empt one.

**Two changes to the product have since been made, and neither came from a
piece of this dossier.** They came from Alexa's word on a remediation
programme, and they are tracked on the board, not here:

- **C2** (ORCH-055, **D175**) — the false success `04b` found: a verdict
  carrying an inline file whose bytes did not hash to the sha256 beside
  them exited 0 and said nothing was missing. A refused file is now named
  and never erased.
- **C3** (ORCH-056, **D176**) — the open-loop commit link `03a` found: a
  verdict could be attributed to a commit the tree was never at. A runner
  now reports the revision it really ran.

Both were kept inside the limit Alexa set: **`PROTOCOL_VERSION` stays 2
and `RECORD_VERSION` stays 1**, asserted by test and verified again by the
integrator on both branch tips and on the merged tree.

Concretely, and for the avoidance of doubt: no test was changed to go
green, no tag was cut, no release was published, and nothing was deployed
to the server or to the first consumer's instance.
