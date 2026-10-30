# herd-is-the-execution-path — handoff 2026-10-30

- **Item:** ORCH-271 (proposed below; the highest id in any tree today is
  ORCH-270, on the pushed and unlanded `delegation-measured`)
- **Agent:** herd-step
- **Branch / commit:** `herd-is-the-execution-path` / `f68eb22`, based on
  origin/main `73c2dc8`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Alexa's execution directive of 2026-10-30 is written, black on white, in
`CLAUDE.md` *Commands*, in seven lines:

- `cargo` is not on PATH and no heavy run belongs on this host:
  **execution goes through Herd** (Alexa, 2026-10-30).
- Submit the command fence's lines with
  `/mnt/ssd_980/herd/queue/bin/herdctl`; bare `herdctl` targets the
  consumer's hub.
- `--label build` **and `--memory-mib`** on every build, suite or gate.
- `./scripts/bench-preflight.sh` first anyway: Herd records load and
  does not gate on it (E-32), so the queue bounds jobs, not cores.
- A job that fails is **reported, never re-run locally**; a local run is
  named a control.

**`CLAUDE.md` is still exactly 150 lines of 150.** The ceiling was not
raised (D251), and **no invariant's words were touched**: the gotcha
`invariants-are-quoted-not-reflowed.md`'s own verify command,
`diff <(git show origin/main:CLAUDE.md | sed -n '/^## Invariants/,/^##
Layout/p') <(sed -n '/^## Invariants/,/^## Layout/p' CLAUDE.md)`, prints
nothing — the section is byte-identical. Nothing in it needed quoting
before and after, because nothing in it changed.

**What paid for the seven lines, four payers for four lines** (the
*Commands* section grew from 17 lines to 21):

1. **The Vocabulary translation sentence, −1.** *"Translate the first
   consumer's 'client' (our runner) and 'agent' (requester)."* stood as
   its own paragraph after a blank line; it is now the second line of
   the `runner` bullet, as *"Translate the first consumer's 'client'
   (our runner), 'agent' (requester)."* One word removed, `and`,
   replaced by a comma. The imperative and both translations are kept.
2. **Layout, −1.** Two lines became one:
   `` `herd-proto/`, `herd-hub/`, `herd-runner/`, `herdctl/`, `docs/`,
   `.claude/` ``. All six directories survive; the annotations do not —
   *"messages"* is invariant 4 (*"All messages live in `herd-proto`"*),
   *"status, decisions, plan, coverage, orchestration/"* is invariant 8
   naming each of those files, and *"`.claude/` agents, hooks"* is the
   Roles bullet's `.claude/agents/herd-*.md`. The `## Layout` **header
   is kept on purpose**: the gotcha's verify command above anchors its
   `sed` range on it, so deleting the section would have broken the one
   recipe that protects the invariants.
3. **`nix develop --command cargo build --workspace`, −1**, dropped from
   the command fence. It was the one line of the five no gate needs:
   `test.sh` and `cargo clippy --all-targets` both build the workspace,
   and invariant 5 says *"It compiles" is not done*. The other four
   lines are unchanged, comments included.
4. **The shared-host `cargo` bullet, 5 lines to 2, −3.** It keeps its
   rule — *"No `cargo` unless the task changes code; one integration at
   a time"*, a build tree off `/`, deleted once pushed — and gives up
   three things that are **already in `workflow.md` *Concurrency and
   host rules*, in more detail than CLAUDE.md held**: the `~12–14 GB`
   debug-tree figure with its seven measurements and the `--release`
   283 MiB counter-figure (that section's first bullet, verbatim);
   *"`/mnt/ssd_980` is the consumer's production disk"* (its second
   bullet: *"the same filesystem the first consumer's production writes
   to"*); and *"More than two heavy agents may run while the first
   consumer is stopped (Alexa, 2026-10-27), bounded by load, memory and
   disk"* (first bullet, verbatim, with the five-at-load-1.21
   measurement). That last one is also **superseded in direction** by
   Alexa's *« tu peux donc relancer plus d'agent une fois que tu est
   sur »* of 2026-10-30, which the new rule carries. CLAUDE.md now says
   *"the sizes are in workflow.md"*.

**`docs/orchestration/workflow.md`** gains `## Execution goes through
Herd` (76 lines, before *Concurrency and host rules*), which has no line
ceiling and is where the detail belongs: Alexa's three sentences
verbatim; the wrapper and why bare `herdctl` reaches 8787; `--label
build` and `--memory-mib` with the 0 MiB admission; **the measured slot
count of our instance**; the preflight clause with E-32 and the slice
room; `--verdict-to` and the product gap behind it; the two frictions;
the named control; and what the rule does not buy. *Concurrency and host
rules* gains one leading bullet saying the queue, not a hand count,
answers *"how many at once"* since 2026-10-30, and that the bullet below
still holds for anything run outside a job.

## Not done, and why

- **`docs/decisions.md`: nothing appended, no number allocated.** It is
  `herd-merger`'s alone. The full entry is drafted below. This is not
  only the standing rule: `check-docs.sh` **fails** on a `D-NEXT` header
  left in that file (*"still has a D-NEXT placeholder; herd-merger
  allocates the number at integration"*), so the handoff is the only
  green place for a drafted entry.
- **`docs/orchestration/board.md`: not edited.** The row is drafted
  below, in full, for the merger to paste.
- **The Rust gates were not run**, and no `cargo` was invoked: this is a
  prose-only change to `CLAUDE.md` and `docs/`. What covers them is the
  batch's own `./scripts/gate.sh` at integration, which is
  `herd-merger`'s run and not mine.
- **No job was submitted to Herd.** The task changes prose only, so
  there was nothing heavy to submit; `bench-preflight.sh` was not run
  for the same reason. The rule is written from the code and the
  configuration, read.

## Gates

Run **before** this file was committed, on the tree carrying `f68eb22`,
verbatim:

```
$ ./scripts/check-docs.sh
exit=0
docs ok
```

```
$ TMPDIR=<scratchpad>/orch-tmp ./scripts/check-orchestration.sh
exit=0
check-orchestration: 324 passed, 0 failed
```

`check-orchestration.sh` is **extra evidence, not a gate this brief
named**. It was run because it executes `check-docs.sh` 38 times over
full copies of the real corpus, so it is what would catch a width or
structure regression that one pass over one tree misses. Its `TMPDIR`
was redirected onto `/mnt/ssd_980` to keep 38 repository copies off the
root filesystem, and the directory was deleted afterwards.

**The detector was proven before any of its silence was trusted**, as
the brief required. One line appended to `CLAUDE.md` in this worktree:

```
$ printf 'DETECTOR PROBE LINE\n' >> CLAUDE.md; wc -l < CLAUDE.md
151
$ ./scripts/check-docs.sh
CLAUDE.md is 151 lines, limit 150
docs check failed
exit=1
```

The probe line was reverted with `git checkout -- CLAUDE.md` before any
real edit, and `wc -l` confirmed 150 again. The 79-column count was
measured with the gate's **own** awk program under `LC_ALL=C`, fence-
and table-aware, over the edited `CLAUDE.md`: no line over 79.

**The last run**, on the tree that includes this handoff, is
`./scripts/check-docs.sh` from the worktree root; its output goes to the
agent's report, not into the tree it gates.

**Skips: no skip.** No `HERD-SKIPPED` line was produced or copied: the
suite was not run.

## Risks and findings

1. **The brief's "its own ceiling is 24 against the preflight's 9" is
   wrong, and the true figure is 2.** Our instance declares one runner,
   `queue-local`, with `build` as `{"type":"counted","slots":2}`
   (`/mnt/ssd_980/herd/queue/etc/capabilities.json`, read 2026-10-30),
   under `--job-memory-max 6GiB`, `--host-memory 16GiB`,
   `--host-memory-floor 4GiB` (read from the runner's own `args`, PID
   607643 — read, never signalled). So the hub admits **two** build jobs
   at once, which is **below** the preflight's load ceiling of 9, not
   above it. 24 is almost certainly the brief's own *"twenty-four
   `rustc` on twelve cores"* — two slots times twelve cores — a count of
   compilers, not a hub ceiling. **This does not weaken clause 5; it
   sharpens it.** *"The queue bounds jobs, not cores"* is exactly right:
   the queue's bound is 2 jobs, and two gates that declared no memory
   are 24 `rustc` on 12 cores. It does mean the queue is **tighter**
   than a reader of the brief would expect, which is the safer error.
2. **The 376-second, twelve-core, ~13 GB, 0 MiB gate job is in no tree.**
   `git grep` over `origin/main`, `origin/delegation-measured` and every
   remote branch finds neither that figure nor the string
   `queue/bin/herdctl` anywhere under `docs/`. It exists only in the
   proving agent's report to the orchestrator. I have written it into
   `workflow.md` **with that provenance stated in the sentence** rather
   than dropped or laundered, and paired it with ORCH-268's figures,
   which **are** in the tree and verifiable (382 s, `peak_memory_bytes`
   3,141,632, `bytes_written` 287 MB against about 11 GB). If the record
   still exists on our hub, somebody should put the numbers in the tree
   before the proving agent's report ages out.
3. **`herd-runner/src/cgroup.rs` exists, so "our runner has no cgroup"
   is about the instance, not the code.** The crate has a full cgroup
   module and `room.rs` walks `/proc/self/cgroup`; what our
   `queue-local` lacks is a delegated cgroup to write, so
   `--job-memory-max 6GiB` falls back to `RLIMIT_AS` (D23), per-process
   address space. `workflow.md` says it that way. A reader who sees
   `cgroup.rs` and concludes jobs are bounded would be wrong, and that
   is the misreading ORCH-268 is about.
4. **`CLAUDE.md` now has zero slack.** 150 of 150. That is the design
   (D251) and it is also the next agent's problem: the next rule needs
   its own payer, and the four payers I used were the cheap ones. The
   two next-cheapest I looked at and rejected: the *"Gates before a
   commit"* paragraph (substantive, stated nowhere else in the file, and
   its *"a gate that did not run is reported as not run, never as
   passed"* is D27 one level up) and the intro's line 6, *"Herd is
   **shipped, not operated** (D161): the consumer runs every instance"*
   — a near-duplicate of invariant 9's headline, but *"the consumer runs
   every instance"* is a positive statement invariant 9 only implies.
5. **One clause of the brief I did not write as given.** The brief's
   drafts say *"(E-32)"* after *"the queue bounds jobs, not cores"*.
   E-32 is the expectation that **load is recorded and never a gate**;
   it says nothing about slots. I put `(E-32)` on the load clause, where
   `herd-hub/src/matcher.rs` earns it (*"load … recorded and not a gate
   (E-32)"*, and the module's own comment: *"there is no `if load`
   anywhere, by design and by test"*), and left the slot sentence
   uncited in `CLAUDE.md`, with the measured figure in `workflow.md`.
6. **The rule names a path outside the repository.**
   `/mnt/ssd_980/herd/queue/bin/herdctl` is this host's, so `CLAUDE.md`
   now carries a line that is false on any other machine. That is
   already true of the *Shared host rules* section, and the alternative
   — naming no path — is the clause that stops a call reaching port
   8787. Worth knowing before Herd's own `CLAUDE.md` is read by anyone
   but us.

## Drafted for `herd-merger`

**For `docs/decisions.md`** — append verbatim, allocating the next free
number in place of `D-NEXT`:

```markdown
## 2026-10-30 — D-NEXT Execution goes through Herd; a local run is a control

Alexa, 2026-10-30, twice: *« a partir de maitenant que du herd, plus
aucune execution local, dois etre ecrit noir sur blanc dans CLAUDE.md et
si il marche pas il te remonte le problemme »*, *« execution local est une
exception pas la norme »*, *« tu peux donc relancer plus d'agent une fois
que tu est sur »*.

Every build, suite or gate is submitted to our own Herd instance with
`/mnt/ssd_980/herd/queue/bin/herdctl`, carrying `--label build` **and**
`--memory-mib`, after `./scripts/bench-preflight.sh`. A job that fails is
**reported**; it is never re-run locally to obtain a greener answer. A
local run is permissible only as a **named control** — the agent says it
is a control, and why. With the queue bounding the work, more agents may
run at once than a hand count allowed.

Three facts the rule is written against, so it is not read as more than
it is. The wrapper exists because bare `herdctl` has
`ws://127.0.0.1:8787` compiled in (`herdctl/src/lib.rs`,
`DEFAULT_HUB_URL`), which on this host is the **consumer's production
hub**; the wrapper exits 125 rather than falling back to a `herdctl` on
`PATH`. `--memory-mib` is required because a job that declares nothing is
admitted at **0 MiB**, leaving the slot count as the only bound — two
`build` slots on our instance, up to twenty-four `rustc` on a twelve-core
host. The preflight still runs first because Herd **records** load and
does not gate on it (E-32), and reads nothing equivalent to the room left
in the systemd slice.

What this does not decide: whether a job record should say *unmeasured*
where nothing measured it (ORCH-268, with Alexa). Our runner has no
delegated cgroup, so Herd today **orders** work and does not **bound** it.

The rule is in `CLAUDE.md` *Commands*, at no cost to the 150-line ceiling
(D251) and with no invariant reworded. The recipe, the two frictions and
the product gap are in `docs/orchestration/workflow.md` *Execution goes
through Herd*.
```

**For `docs/orchestration/board.md`**, section *Owed — agreed, not
started*, state `done` (renumber if ORCH-271 is taken by the batch; the
highest id in any tree on 2026-10-30 is ORCH-270, on
`delegation-measured`):

```markdown
| ORCH-271 | **Alexa's execution directive is written into `CLAUDE.md`, black on white.** Every build, suite or gate goes through our Herd instance with `/mnt/ssd_980/herd/queue/bin/herdctl`, `--label build` and `--memory-mib`, after `bench-preflight.sh`; a failed job is reported, never re-run locally; a local run is a named control. Seven lines in *Commands*, paid for by four reclaimed lines, ceiling untouched at 150 of 150 (D251), Invariants byte-identical to origin/main. The recipe, the two frictions, the product gap and what the rule does not buy are in `workflow.md` *Execution goes through Herd* | Alexa, 2026-10-30, asked twice; measured and written by `herd-step` the same day | herd-step | done | land with the batch; the decision entry is drafted in the handoff | nothing | branch `herd-is-the-execution-path`, commit `f68eb22`; `docs/orchestration/history/2026-10-30-herd-is-the-execution-path.md`; `check-docs.sh` green, `check-orchestration.sh` 324 passed 0 failed |
```

## Decisions needed

None new. One standing question is **touched and left where it is**:
ORCH-268, whether a job record should say *unmeasured* where nothing
measured it. It is Alexa's, it is open, and the new rule does not depend
on it — `workflow.md` now says so in terms, so that a reader does not
take the rule for protection it is not.

## Resume point

`herd-merger`, at integration: allocate the number for the drafted
`docs/decisions.md` entry above, paste the board row into *Owed*, and
run the batch's one `./scripts/gate.sh` for the gated sha. Nothing here
waits on anything.

If the figures of finding 2 matter to somebody, the next step is to read
that gate job's record off **our own** hub's state root — ours, never a
consumer's (D161) — and put the numbers in the tree, because `herdctl`
has no command that reads a finished job's record.

## Processes

**PIDs started and ended: none.** No build, no test, no job, no daemon.
Nothing was signalled: the two processes read were the consumer's
production (untouched) and our own runner, PID 607643, whose `args` were
read with `ps -o args= -p 607643` and nothing else.

**No `target/` to delete**: no `cargo` ran, `CARGO_TARGET_DIR` was never
set, and no build directory was created. The worktree
`<scratchpad>/wt-exec-path` holds prose only; the scratchpad's
`orch-tmp`, which held `check-orchestration.sh`'s 38 corpus copies, was
removed when that run finished.
