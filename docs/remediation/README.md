# The remediation dossier — index

The audit dossier required by §10 of
`MANDAT_HERD_AUDIT_ARCHITECTURE_DECISION_2026-10-24`. One short page per
piece, written separately; no monolithic journal.

**Base read by every piece:** `origin/main` at `0a2cf73`. That commit
carries **no tag**; tag `v0.3.7` is `3da48f5`, two merges earlier. No Rust,
`Cargo.*` or Nix file differs between the two, so the product audited here
is `v0.3.7`'s product — see `00-baseline.md §2`.

**Date of this index:** 2026-10-24.

---

## The pieces

| Piece | Branch | What is in it, and what it settles |
|---|---|---|
| [`00-baseline.md`](00-baseline.md) | `remediation-dossier-index` | The reference state at `0a2cf73`: what Herd is, the four crates and their sizes, the four Rust gates plus `check-docs.sh` and `check-orchestration.sh`, the loud-skip machinery (D27), `PROTOCOL_VERSION` 2 and `RECORD_VERSION` 1 with the three `!=` checks that block the move up *and* the move back, the nine repository invariants, the two known-unstable tests, and the two real incidents with their figures. Settles **what commit was audited and what it contained**, and separates the mandate's label `v0.3.7` from the base `0a2cf73`. |
| [`03a-identity-and-storage.md`](03a-identity-and-storage.md) | `audit-identity-storage` | Audit A, mandate §4.1: who owns which state and where it persists, the job/attempt automaton and its commit points, what `JobResultAck` / `LogAck` / `JobFilesAck` each actually guarantee, the identity chain from a requester's commit to an artefact, and six integrity risks. Settles that the hub's durability discipline is real and is one module (`durable.rs`); that Herd is **at-least-once execution with at-most-once recording** and claims nothing stronger; that the **commit link of the identity chain is open-loop** — declared on the way out, never confirmed on the way back; and that one inline path can let a short collection read as whole. |
| [`03b-concurrency-and-progression.md`](03b-concurrency-and-progression.md) | `audit-concurrency-progression` | Audit B, mandate §4.2: the hub's tasks and locks, where a blocking call is made from an async context, why a deep queue or a slow store stops progression, backpressure, fairness and starvation, stop/drain/restart, host failure modes, both incidents re-examined, and the two unstable tests. Settles that the hub's **persistence and its scheduler share the reactor with nothing bounding a pass**; that a pass is O(queue depth) in reads and runs once per event; that invariant 2 is enforced only in the hub; that the hub has **no stop path**; and that one unwritable record aborts a whole dispatch pass. |
| [`03c-contradictory-review.md`](03c-contradictory-review.md) | `audit-contradictory-review` | The contradictory review of A and B, by a third reader who wrote neither: every **F** in both reports taken back to the code at `0a2cf73`. Settles that the "frontal contradiction" between A and B on D171 **does not exist** — the two were set against each other on a misreading; that the mandate's base label is wrong; that all of A's and B's checked findings hold, two of them more strongly than stated and one citation off by 39 lines; and it names the blind spot the A/B split created. Ends in one ranked list across both audits and a minimal four-run experiment sequence. |
| `04a-cost-surface.md` | — | **Expected; being written now.** Its subject is the cost surface. Nothing about its content or its conclusions is stated here, and nothing in this dossier should be read as anticipating them. |
| `04b-integrity-and-containment.md` | — | **Expected; being written now.** Its subject is integrity and containment. Same rule: its findings are its own, and this index does not guess them. |

The mandate's §10 skeleton also names `01-contract.md`,
`02-consumer-boundary.md`, `05-invariants.md` and the pieces after them.
They do not exist yet. The numbering above keeps their places rather than
renumbering what is written, so a gap in this table means *not written*,
never *lost*.

---

## How to read the dossier

**Everything here is a constat. Nothing here is a mesure.**

- **Constats** — `00-baseline.md`, `03a`, `03b`, `03c`. Every one of them
  was produced by reading the repository at `0a2cf73`. No build, no test,
  no experiment, no load on any host, no process touched. Where one of them
  carries a figure from a running system — the 214 jobs, the 255 cgroups,
  the 775 files, the 20 683 `stays queued` lines, the 1.8-3.9 % of a CPU —
  that figure was **measured by the first consumer or on the host D171 was
  written on**, and the document says so at the point of use. It is
  evidence of an incident; it is not a measurement any of these authors
  made.
- **Mesures** — none, yet. The experiments that would turn the hypotheses
  into measurements are specified and not run: `03a §8`, `03b §10` and the
  minimal four-run sequence in `03c §6`. `00-baseline.md §11` lists what a
  campaign needs before the first of them can be started — an isolated
  host above all, because this machine is shared with a system in
  production.

**The three marks, used throughout.** **F** = observed at `0a2cf73`, with a
path and a line, a test name, or a decision entry. **H** = a hypothesis the
code suggests and nothing here settles. **I** = unknown. A figure nobody
verified is marked **I** rather than repeated; `00-baseline.md §5` is the
worked example — the repository states both 12 and 13 loud skips, and the
honest answer is that no one here ran the suite.

**Where the pieces disagree, `03c` is the arbiter of fact** — it re-checked
both of the others against the code — but it is not an arbiter of what to
do. Where a question is open, it stays open and is listed as such
(`03a §9`, `03b §11`, `03c §8`).

---

## What this dossier does not authorize

**Nothing in it authorizes a modification of the software.** It is a
reading. No finding here is a work order, no ranked list is a backlog, and
no recommendation in `03c §7` becomes a plan by being written down.

Every change — to the code, to the protocol, to the gates, to the
architecture — is **Alexa's decision, one at a time, each with its own
recommendation and its own consequences**, as `CLAUDE.md` and the mandate
both require. The audit's job is to make each of those decisions
answerable; it is not to pre-empt one.

Concretely, and for the avoidance of doubt: no Rust was changed, no test
was changed to go green, no tag was cut, no release was published, and
`main` was not touched by any piece of this dossier.
