# delegation-measured — handoff 2026-10-30

- **Item:** no row of its own. It is an instance of **ORCH-241**'s class
  — a measurement that nothing obliged to reach the tree — and it files
  **ORCH-269** and **ORCH-270**.
- **Agent:** herd-step
- **Branch / commit:** `delegation-measured`; the prose at `b9bd34c`, the
  board at `d2c8860`, and this file in the branch tip that carries it —
  a handoff cannot name its own sha. Based on
  origin/main **`73c2dc8`**. The brief named `b99d32d`; batch 21 landed
  while this task ran and `origin/main` moved. The branch was started on
  `b99d32d` and rebased onto `73c2dc8`, which is a descendant, so the
  base the brief named is an ancestor of this work and no edit was lost.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing and fills the record of `templates/landing.md`.

## Done

Four findings existed only in a chat message. Three are now in the tree
and the fourth was already there, filed by somebody else.

**1. The number, with its invocation — `docs/runner-confinement.md`.**
The section *Tests, and which of them a host can make* said **"Two
assertions need cgroup v2 delegation"** where this workstation skips
**twelve**, and gave a recipe — `systemd-run --user --scope -p
Delegate=yes target/debug/deps/confinement-<hash>` — that cannot reach
twelve however it is run. That stale sentence is why the finding had a
home: it is ORCH-241's class, a figure corrected nowhere when ten
assertions joined the first two. The section now carries, with the
superseded figure named as superseded:

- the measured table of five invocations, 0 / 0 / 10 / 12 / 12 of
  twelve;
- the two distinct refusals behind one count — `Permission denied
  (os error 13)` with no scope, `EBUSY (os error 16)` on the first
  controller write under a wrapper, because the wrapper shell process
  sits directly in the scope;
- the structural cap at ten: two of the twelve are unit tests in
  `herd-runner/src/confine.rs` and so live in the `herd_runner` lib
  test binary, not in `confinement`. **Verified here by name**, not
  taken from the brief: ten of the twelve names resolve to
  `herd-runner/tests/confinement.rs` and exactly two to
  `herd-runner/src/confine.rs`, with no overlap;
- a corrected recipe that runs **both** binaries, each as its scope's
  only process, with `--test-threads 1` and `-p MemoryMax=12G`;
- the `systemd-run` `${…}` expansion trap, as one line on the recipe
  rather than a gotcha of its own, because it is about how to invoke
  the command the recipe gives and is unreadable away from it;
- a subsection on what twelve are worth: **+11 and not +12**, and
  `pressure_refuses_nothing_and_delays_nothing` in one of three runs.

**2. The in-scope list is not `local.txt` minus twelve —
`scripts/expected-skips/README.md`.** A second home, and the reason is
that file's own sentence: *"Updating a list is a decision, not a
chore."* Whoever blesses an in-scope list reads that file and not
`runner-confinement.md`, and the arithmetic they would do is wrong. The
note gives them both facts and points at the invocation.

**3. The risk that changed the brief's mind — a gotcha.**
`docs/orchestration/gotchas/a-delegated-scope-blinds-the-preflight.md`:
an uncapped delegated user scope reads no ceiling at any level, so
`bench-preflight.sh` check 4 prints *"reading unavailable"* and
**clears**, silencing the clause added on 2026-10-29 after five agents
lost a suite run. Gotcha shape fits exactly — a date, an observation, a
mechanism, a rule, a verify step. The rule (`-p MemoryMax=…` on any
such scope) is also on the recipe, because a gotcha is read on demand
and a recipe is read while acting.

**4. Corrections recorded as the orchestrator's — a gotcha.**
`docs/orchestration/gotchas/delegation-is-a-property-not-a-file.md`:
`Delegate` and `DelegateControllers` on the unit answer *does this
delegate*; `cgroup.subtree_control` and the presence of a knob do not.
Both halves re-measured here at rest, and the second half **refutes the
brief** — see *Risks and findings*.

**5. The two product defects — nothing recorded, deliberately.** They
are already in the tree as **ORCH-267** (`--outputs-to` on an ad-hoc
command line returns nothing, in silence) and **ORCH-268** (a record's
resource figures never say whether anything measured them, with the
382-second job, the 3,141,632-byte peak and the 287 MB against ~11 GB).
Both landed on `main` in batch 21, at `73c2dc8`, filed by the
orchestrator from the same gate run. ORCH-268 also records that the
*unmeasured* question is Alexa's and open, which the brief asked for.
Writing them again would have put two rows on the board for each
defect, which is worse than a message.

**6. Board.** ORCH-269 and ORCH-270 added; **ORCH-116** answered where
its question was asked — its second candidate was literally *"whether
CI can run the `confinement` target under `systemd-run --user --scope
-p Delegate=yes"*, and leaving that answered only in a handoff would
have repeated the defect this task exists to fix.

## Not done, and why

- **The preflight clause is not changed.** `scripts/bench-preflight.sh`
  is a script, this was a prose task, and the fix has a real choice in
  it. Filed as ORCH-269 with the three candidate shapes.
- **Nothing re-measured inside a scope.** The brief forbids creating a
  cgroup or a scope. Every in-scope figure in the tree is attributed to
  the 2026-10-30 probe and dated; what was verified independently here
  is listed under *Risks and findings*.
- **`docs/status.md` untouched.** It is at its 90-line limit with no
  slack, and none of this is product state.
- **`docs/decisions.md` untouched.** The entry is drafted below for
  `herd-merger`, which allocates the number.
- **No `cargo`, no `./scripts/test.sh`, no `bench-preflight.sh`.** No
  heavy phase ran, so there was nothing to preflight, and no
  `CARGO_TARGET_DIR` was created.

## Gates

Prose only. The four Rust gates were **NOT RUN** locally — `cargo build`,
`cargo test`, `cargo clippy`, `cargo fmt` — because the brief forbids
`cargo` while a merger and a step are at work on this host. What covers
them is the batch's own `./scripts/gate.sh` run at integration, which is
`herd-merger`'s and not this agent's.

Run before this file was committed, on `d2c8860`:

```
$ ./scripts/check-docs.sh
docs ok

$ ./scripts/check-orchestration.sh
[…]
ok    check-orchestration.sh redirects its stdin before its first capture
ok    gate.sh redirects its stdin before its first step
ok    pre-push keeps the stdin git hands it: the ref list is its input

check-orchestration: 324 passed, 0 failed
```

Baseline on `b99d32d` before any edit, for comparison: `docs ok` and
`check-orchestration: 324 passed, 0 failed`. The expected 324 of the
brief is confirmed, before and after.

**The last run** happens on the tree with this file committed:
`./scripts/check-docs.sh && ./scripts/check-orchestration.sh`, output to
`<scratchpad>/dm/docs-final.txt` and `<scratchpad>/dm/orch-final.txt`,
quoted verbatim in the agent's report and not added to the tree.

**Skips:** no skip. No test binary ran. `HERD-SKIPPED` lines quoted in
the prose above are copied from `scripts/expected-skips/local.txt` and
from the probe's report; none is counted as a pass.

## Risks and findings

**The brief is wrong in six places, and two of them matter.**

**(a) `ORCH-263` is not the highest row id in play, and `origin/main`
moved under the task.** The brief said to verify, and the verification
refutes it. At `b99d32d`, `main`'s highest was indeed ORCH-263 — but
`batch-21-integration` already held **ORCH-264 to ORCH-268**, and that
branch landed as `73c2dc8` while this task ran. New rows therefore
start at **ORCH-269**, which is what this branch uses. An agent that
had trusted the brief's number would have written ORCH-264 and
collided with five rows.

**(b) Finding 3 was already in the tree, filed by its own author.**
ORCH-267 and ORCH-268 carry the two product defects in substance and in
the brief's own figures. They were on `batch-21-integration` when the
brief was written and are on `main` now. **Nothing was recorded for
finding 3** and nothing should be.

**(c) Finding 4's second bullet is itself wrong, in the same way as the
bullet it corrects.** The brief says *"`io` is not in
`DelegateControllers` and reaches the scope anyway, because `io` is
enabled in `app.slice`'s own `subtree_control`"*. Measured here at rest,
2026-10-30, on this host:

| read | value |
|---|---|
| `app.slice/cgroup.subtree_control` | `memory pids` |
| `app.slice/cgroup.controllers` | `cpu memory pids` |
| `user@1002.service/cgroup.subtree_control` | `cpu memory pids` |
| `user-1002.slice/cgroup.subtree_control` | `cpu io memory pids` |

`io` is **not enabled in `app.slice` and not even available there**. The
chain stops one level up: `user-1002.slice` passes `io` down,
`user@1002.service` does not — exactly as its `DelegateControllers=cpu
memory pids` says. So the correction of a file-reading error was made by
reading another file, and the brief's *"my conclusion held and my reason
was wrong"* is itself a reason that does not hold. The known-bad fixture
is on this host and needs nothing built: `app.slice` holds `io.pressure`
while `io` is absent from its `cgroup.controllers`, so *"an `io.*` file
is here"* is the reading that manufactures both errors. Whether `io` ever
reached the scope **cannot be settled without creating one**, which this
task may not do; the gotcha says so rather than guessing.

**(d) The "10 of 12" row is confounded and no cause is recorded for it.**
Ten is exactly `confinement`'s share of the twelve — verified by name
here — *and* the brief attributes that row to twelve threads and the
no-internal-process rule. Those are two different causes for one number,
and the rows that reached twelve differ from the ten row in at least two
variables at once: pre-warming or `--test-threads 1`, **and** whether
the second test binary ran at all. The table is recorded as measured,
the structural cap is recorded as a separate verified fact, and **no
causal claim is made about why the number was ten**. Anybody re-running
this should vary one thing at a time.

**(e) "All twelve cgroup-delegation skips" needs its qualifier.** Two of
the twelve also need a **block device** under the job's directory;
`/mnt/ssd_980` is one here, which is why twelve is reachable on this
machine and would not be on a delegated host whose data root is ZFS. The
board's ORCH-070 splits the same twelve as *"10 cgroup v2 delegation, 2
ZFS `io.max`"*. Both framings describe one set of names; the prose
carries the qualifier so the next host does not expect twelve.

**(f) `a_fallback_is_shouted_not_mentioned` does not become unassertable
silently.** It skips with its own written reason — *"this host enforces
the real cgroup ceiling, so there is no fallback line to assert here"* —
at `herd-runner/src/confine.rs:1061`, and its doc comment already names
the host that enforces the real ceiling. The **+11** is right; the
mechanism is the design working, not a hole, and the prose says so.

Minor: the brief reports load 10.71; it was **5.64** at this task's
start, and no heavy phase ran.

**What was verified here, independently of the probe's report:** the
twelve skip names in `scripts/expected-skips/local.txt`; which source
file each resolves to; the existence of
`a_fallback_is_shouted_not_mentioned` and
`pressure_refuses_nothing_and_delays_nothing` and what the first does on
a non-falling-back host; `memory.max` and `memory.high` reading `max` at
`user.slice`, `user-1002.slice` and `user@1002.service` and **absent**
at `/sys/fs/cgroup`; `Delegate` and `DelegateControllers` on
`user-1002.slice` and `user@1002.service`; the whole
`cgroup.subtree_control` chain; `io.pressure` present in `app.slice`
without the `io` controller; and that
`scripts/bench-preflight.sh`'s `elif` arm at `:369`-`:371` prints and
falls through with `refused` untouched. **What was not verified here:**
every figure taken *inside* a scope — the five-row table, the `EBUSY`,
the `12281 MiB under memory.max` reading, and the one-of-three
pressure flap. Those are the probe's, dated and attributed in the prose
as such, because re-measuring them needs a scope.

**Collision risk.** This branch edits `docs/orchestration/board.md`,
which batch 22's other branches will also edit, and it rewrites one
existing line (ORCH-116). Expect a conflict in the Owed and Open tables
at integration; the three edits are additive and the rows are below.
`docs/runner-confinement.md`, the two gotchas,
`docs/orchestration/gotchas/README.md` and
`scripts/expected-skips/README.md` were untouched by batch 21 and are
unlikely to collide.

## Drafted for `herd-merger`

### The `docs/decisions.md` entry

Title measured **unfenced**, as it will land, with the command from
`a-drafted-title-is-measured-unfenced.md`: `## 2026-10-30 — D258 A
delegated scope carries its own memory ceiling` is **69 columns**, and
still fits if the allocated number is wider.

```markdown
## 2026-10-30 — D-NEXT A delegated scope carries its own memory ceiling

Making the twelve cgroup-delegation assertions on this host needs a
delegated user scope. Measured on 2026-10-30: inside an uncapped
`systemd-run --user --scope -p Delegate=yes`, every level
`scripts/bench-preflight.sh` walks reads `memory.max=max` and
`memory.high=max` — `user.slice`, `user-1002.slice`,
`user@1002.service`, `app.slice` and the scope itself — and
`/sys/fs/cgroup` has neither file. Check 4 then prints *"nothing from …
up to the root of /sys/fs/cgroup carries a memory.high or a
memory.max, reading unavailable"* and **clears**, because that branch
says a sentence and never calls `refuse`.

**So the method that unlocks twelve assertions disables the check added
on 2026-10-29 after five agents each lost a suite run** to
`chatons.slice`'s 18 GiB `memory.high`. A user scope is outside
`chatons.slice`: it escapes the one ceiling bounding everything else
this project does on this host, and the guard that would have noticed
is the guard it silenced.

**The rule: any scope or unit installed to get cgroup delegation
carries its own memory ceiling.** Measured, not reasoned: adding
`-p MemoryMax=12G` restored a real reading — *"12281 MiB under
memory.max on the scope, floor 8192"* — and check 4 then cleared on its
merits instead of on an absence. It is safe for the tests themselves,
because the assertion about a tighter enclosing ceiling asserts its own
512 MiB child, and the baseline already passes under an 18 GiB
enclosure: an enclosing ceiling is this host's ordinary condition, not
a variable the scope introduces.

**What this entry does not decide.** The *"reading unavailable"* branch
now covers two situations with opposite risk — a host with no cgroup v2
at all, which is honest, and a process that put itself outside every
ceiling, which is not — and refuses in neither. Which of the three
shapes it becomes is **ORCH-269** and is a script change, not a rule.
This entry fixes the rule that does not wait for it.

Reversible: one flag on one command line, and a sentence in
`docs/runner-confinement.md`. In the finder's words, kept verbatim
because the sentence is the lesson: *"on this host, 'nothing encloses
me' now means 'I am in a user scope', not 'I am on a bare machine'."*
```

### The `board.md` rows

Both are already committed on this branch, at the end of their sections.
Reproduced in full so the merger can paste either one if the tables
conflict.

ORCH-269, last row of *Owed*:

| ORCH-269 | **`bench-preflight.sh`'s cgroup clause says *reading unavailable* and clears, and a delegated user scope is now a way to reach that branch on purpose.** Check 4 walks from `/proc/self/cgroup` up to the root of `/sys/fs/cgroup` looking for a `memory.high` or a `memory.max`; inside an uncapped `systemd-run --user --scope -p Delegate=yes` every level reads `max` and the root has neither file, so the `elif` arm at `scripts/bench-preflight.sh:369`-`:371` prints its sentence and falls through to the verdict with `refused` untouched. The clause was added 2026-10-29 **because** five agents each lost a suite run to `chatons.slice`'s 18 GiB `memory.high`, and such a scope is outside `chatons.slice` — so the method that unlocks the twelve delegation assertions is the method that silences the check guarding those runs | measured 2026-10-30 by the delegation probe whose findings `delegation-measured` filed; the blind branch itself is verifiable from the script with no scope created | herd-step | owed | **The message now covers two situations with opposite risk and refuses in neither:** a host with no cgroup v2 at all, which is honest and fine, and a process that put itself outside every ceiling this host has, which is the dangerous one. Decide which of three the clause becomes — distinguish the two and refuse only the second; refuse both and make `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB=0` the explicit opt-out; or keep clearing and say loudly which of the two it saw. The detection is cheap and does not need a scope to test: a self cgroup under `user.slice` with no ceiling anywhere above it is the second case, a host with no `0::` line is the first. **The rule meanwhile is written and does not wait for this row:** any scope or unit installed for delegation carries its own `MemoryMax`, measured at `-p MemoryMax=12G` giving *12281 MiB under memory.max on the scope, floor 8192* | — | `scripts/bench-preflight.sh:357`-`:390`, check 4 and its `elif` arm; `docs/orchestration/gotchas/a-delegated-scope-blinds-the-preflight.md`; `docs/runner-confinement.md` *Making those twelve*; `history/2026-10-30-delegation-measured.md`; **D-NEXT of this branch** |

ORCH-270, last row of *Open*:

| ORCH-270 | **A brief that confines an agent to its scratchpad guarantees its findings never reach the tree — ORCH-241's defect, committed by the method rather than by a stale figure.** A probe agent was told to write nowhere but its scratchpad, which is correct for a measurement that creates cgroups on a shared host and wrong for what the measurement *learned*: seven refuted premises, a corrected recipe, a blinded gate clause and two product defects existed only in a chat message, with no branch, no handoff and no row. The orchestrator filed it against itself on the same day it ruled on ORCH-241, and the recovery cost a second task reading the message back out. Nothing in `templates/brief.md` or `workflow.md` obliges a brief to say where findings land | the orchestrator, 2026-10-30, naming its own defect in the brief that commissioned `delegation-measured` | the orchestrator | open | Decide where the obligation lives, and note it is cheap in all three shapes — a line in `templates/brief.md` that every brief names a path for findings even when it forbids every other write; a standing rule that an agent allowed no repository write still gets a branch for its handoff, which is the shape that cannot be forgotten because the handoff is already mandatory; or a `check-orchestration.sh` case that a brief template without a findings path is red, which checks the template and never the brief actually sent. **The second is the lean**, because a handoff on a branch is an artefact the merger can read and a scratchpad is not, and because it changes no permission the probe was given: a branch holding one prose file creates no cgroup. **Not a question for Alexa** — it is *how*, and it is reversible | ORCH-241 | the brief of 2026-10-30 that commissioned this branch; `docs/orchestration/templates/brief.md`, which names no findings path; `docs/orchestration/workflow.md` *Handoff and resumption*; `history/2026-10-30-delegation-measured.md`; ORCH-241 |

The ORCH-116 edit is an insertion into one existing row and is on this
branch; if that row conflicts, the text to keep is the sentence
beginning *"Second candidate answered, 2026-10-30"* appended to its
*Next action* cell, and five paths appended to its *Evidence* cell.

## Decisions needed

**None for Alexa.** Everything here is *how*: a recipe, a gate clause, a
brief template. The one product question in the brief's finding 3 — does
a record say *unmeasured* rather than a number — is already with him and
already recorded on **ORCH-268**, so it is not re-raised.

For the orchestrator, two: whether **ORCH-270** is accepted as filed,
since an agent filing a method defect against the brief that sent it is
a judgement call; and whether **ORCH-269** waits for the three shapes to
be chosen or is handed to a step with the lean built in.

## Resume point

Nothing is in flight. The next task on this material is **ORCH-269**: a
step that changes `scripts/bench-preflight.sh` check 4 so the two
situations behind *"reading unavailable"* are distinguished, with a test
that needs no scope — a fake self cgroup under a fake mount is the shape
`HERD_PREFLIGHT_CGROUP_MOUNT` and `HERD_PREFLIGHT_SELF_CGROUP` already
exist for. For the merger: pick up this branch for batch 22, allocate
the `D-NEXT` above, and expect a `board.md` conflict in *Owed* and
*Open*.

## Processes

No process started, none ended, no PID to report. No cgroup and no scope
created. No `cargo`, so no `CARGO_TARGET_DIR` and nothing to delete.
Reads of `/sys/fs/cgroup` and `systemctl show` only, which create
nothing. Load at start 5.64, well under any bound; no heavy phase, so
`./scripts/bench-preflight.sh` had nothing to gate and was not run.
