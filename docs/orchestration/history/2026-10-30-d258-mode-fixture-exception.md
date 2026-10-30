# d258-mode-fixture-exception — handoff 2026-10-30

- **Item:** no board row — a prose step briefed by the orchestrator on
  2026-10-30, to put D258's measured exception beside the directive in
  prose rather than leave the next agent to meet it in a red gate
- **Agent:** herd-step
- **Branch / commit:** `d258-mode-fixture-exception` / the commit this
  file is in, based on origin/main `1df941b`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**One file changed, `docs/orchestration/workflow.md`, 48 lines added and
none removed.** Four bullets at the end of *Execution goes through
Herd*, after *What the rule does not buy* and before *Concurrency and
host rules*. Nothing else in the tree is touched: no product file, no
test file, no `docs/status.md`, no `docs/decisions.md`, no
`docs/orchestration/board.md`, no `CLAUDE.md`.

The four bullets, by what each is for:

1. **The cost, stated and attributed to the rule it belongs to.** A job
   workspace is walked every 200 ms and the walk hands itself read
   access back on any directory it cannot list,
   `set_permissions(mode | 0o700)`, which invariant 2 and D166 require;
   `tmp` is inside the walked base and is the suite's `$TMPDIR`, so no
   `tempfile::tempdir()` in the suite is outside it. **The rule, the
   mechanism and the proof are left to D268** rather than restated, and
   the bullet says plainly that nothing in `herd-*/src` is a defect for
   this.
2. **How an agent recognises it and what the guard does.** `0o711` where
   the fixture set `0o111` is the fingerprint, because it is
   `mode | 0o700` and never the fixture's own mode nor a guard's
   `0o755`. Then the shape: ask whether the premise still holds at the
   moment a measurement is judged, re-establish it if not, treat a
   measurement taken on a dissolved premise as **void** and retake from
   nothing, bound the attempts, be **red with a named sentence**, and
   never **skipped** — the `HERD-SKIPPED` list is compared by name and
   not counted (D27, D187). **No sleep, no `#[ignore]`, no widened
   assertion.** The shape to copy is named by path:
   `herd-hub/tests/a_failed_listing/mod.rs`.
3. **The silent direction, written as the one to look for.** The loud
   half — premise dissolves, test fails — is pointed at **ORCH-291**.
   The dangerous half is a test that **passes** because access was
   restored when it should have failed, and **ORCH-292** is named with
   its one known case,
   `herd-runner/tests/confinement.rs::a_job_cannot_hide_what_it_wrote`
   `_from_its_disk_budget`, `chmod 300 h; sleep 6` against a 200 ms
   restorer. Batch 26's landing record is quoted verbatim for the
   consequence, and the fix is called **in flight, not done**, with the
   reason: a branch is cut and carries no commit yet. The bullet tells
   the reader to read those two rows rather than re-derive either list,
   because a list kept in two places drifts.
4. **Provenance.** The direction is Alexa's (D258, 2026-10-30), the
   wording of the rule is the orchestrator's, and this exception is its
   measured cost, found the same day in batch 26's red gate. Stated as
   provenance and not as blame, and stated at all so a later reader does
   not assume the exception was always known.

**The sentence the passage exists for is in bullet 1 of D268's own
citation and in bullet 2's close**, and it is not softened: a gate run
through Herd is not equivalent to a gate run locally for the class of
assertions that depend on a filesystem mode. It is carried by D268's
wording — *"the same tree is sound locally and unsound through Herd"* —
which this file points at instead of paraphrasing a second time.

**Nothing in the drafted text was weakened, and the two figures I could
not check were replaced by figures a reader can.** See *Risks and
findings*.

## Not done, and why

- **No decision entry.** The brief asked for none and none is needed:
  **D268** landed with batch 26 (`d62e2cb`) and says the rule. Writing a
  second entry would be the drift this task exists to prevent.
- **No board row for new work.** One row is drafted below, and it is a
  correction to a landed entry's citations, not new work. The class's own
  work is ORCH-291 and ORCH-292, both already allocated and owned by a
  `herd-step` that has not run.
- **`herd-runner/tests/confinement.rs` is untouched**, as briefed: it
  belongs to `settle-confinement-hollow`, and its fix is described here
  as in flight, never as done.
- **The four Rust gates were not run.** Prose only; see *Gates*.
- **No `CLAUDE.md` line was taken.** I considered whether the
  fingerprint belonged there and concluded it does not: *Commands*
  already sends the reader to workflow.md's *Execution goes through
  Herd*, which is exactly where the exception now sits, so the 150-line
  ceiling (D251) is not approached and no sentence is owed there.
- **`docs/status.md` untouched**, at its 90 of 90.

## Gates

**Prose only, so the four Rust gates were NOT RUN locally** — not
`cargo fmt --all --check`, not `cargo build --workspace`, not
`cargo clippy --workspace --all-targets -- -D warnings`, not
`./scripts/test.sh`. What covers them is the batch's own
`./scripts/gate.sh` at integration, which is `herd-merger`'s run and not
this agent's. A gate that did not run is reported as not run, never as
passed.

Run **before** this file was committed, on the tree carrying only the
`workflow.md` change:

```
$ ./scripts/check-docs.sh
docs ok

$ ./scripts/check-orchestration.sh
check-orchestration: 324 passed, 0 failed
```

**The last run**, on the committed tree with this file in it:
`./scripts/check-docs.sh` and `./scripts/check-orchestration.sh`, from
this agent's own worktree:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-mode-exception
```

Their verbatim output is in the agent's report and is deliberately not
added to the tree it gates.

**No skip.** Neither script skips anything, and no `HERD-SKIPPED` line
exists to copy: this branch ran no suite. No Herd job was submitted, so
no job id is owed.

## Risks and findings

### 1. Two figures from the brief I could not check, and what I wrote

- *"the first red gate in nineteen landings"* is **not checkable from
  the tree**: `main` carries 25 `Land batch` commits, and a count of
  green gate runs is not recorded anywhere a reader can reach. The
  prose says *"found the same day, in batch 26's red gate"* instead,
  which is checkable against D268 and the batch-26 record.
- The sentence quoted to me in capitals — *"A GREEN SUITE THROUGH HERD
  THEREFORE DOES NOT TODAY EXERCISE THAT ONE ASSERTION."* — **is not in
  the tree in that form**, and *"therefore"* is not in it at all. What
  landed, in `docs/orchestration/board.md` and in
  `docs/orchestration/history/2026-10-30-merge-batch-26-regate.md`, is:
  *"A green suite through Herd does not today exercise that one
  assertion, and no report may be read as if it did."* **That is what I
  quoted, verbatim**, with the history file named so a reader lands on
  it. The capitals were the rendering of whoever passed it to me.

### 2. D268's citations have drifted, and one of them only looks wrong

Read on `origin/main` `d62e2cb`, the tree D268 landed in:

| D268 cites | What is actually there | Verdict |
|---|---|---|
| `confine.rs:106` | `POLL_INTERVAL = 200 ms` | exact |
| `root.rs:317` | `let tmp = base.join("tmp")` | exact |
| `root.rs:443` | `pub fn measure(&self)`; the `measure_tree` call is `:444` | off by one, harmless |
| `job.rs:1555` | the polled `workspace.measure()` | **exact on `main`** |
| `root.rs:538` | a line of `read_dir_restoring`'s doc comment; the fn is `:541` | inside the right item |
| `root.rs:664` | a line inside `restore_access`'s body; the fn is `:658` | inside the right item |

**No fact in D268 is wrong** — every citation lands in or on the right
item, and the mechanism it describes is the mechanism in the code, which
I read end to end: `read_dir_restoring` calls `restore_ancestors` **and**
`restore_access`, and `restore_access` returns early on anything that is
not a directory, so only directories the walk cannot list are restored
and a file at `0o000` or a directory at `0o500` is outside the class.
`decisions.md` is append-only, so nothing is to be corrected in place;
the drafted row below only records the readings so the next agent does
not re-derive them. **`job.rs:1555` is worth knowing about**: it is
exact on `main` and was `job.rs:1499` at my base `1df941b`, because
batch 26 changed `job.rs`. A line number in that file is worth
re-reading before it is quoted again.

**The prose I wrote cites no line number at all**, by choice: D268 holds
them, and a second copy in a second file is a second thing to drift.

### 3. My base is one landing behind what my text cites

Base `1df941b`; `D268`, `ORCH-291`, `ORCH-292` and
`history/2026-10-30-merge-batch-26-regate.md` arrived with `d62e2cb`. So
**four references in the new prose do not resolve inside this branch's
own tree** and all four resolve once it sits on `main` or in a batch
based on it. `check-docs.sh` validates no cross-reference, which is why
both gates are green either way; the merger should know that the text is
true of the integrated tree and not of this branch alone. I stayed on
`1df941b` because the brief said to.

### 4. No conflict, and the text does not hedge on one

I own `docs/orchestration/workflow.md` for this change: nothing else in
my tree is touched, and the claim on `main` that
`orch-282-weigh-records` is in this file is wrong — that branch is in
`docs/operating.md`. Neither the prose nor this handoff hedges on a
conflict that does not exist.

### 5. What a sceptic should still check

The prose asserts that nothing in `herd-*/src` is a defect for this. I
read the mechanism and agree with D268, and the measurement is D268's
(job `j0000000147`), **not mine** — this branch ran no job and measured
nothing. If that measurement is ever refuted, the three bullets that
rest on it go with it.

## Drafted for `herd-merger`

`D268` and `ORCH-291`/`ORCH-292` are allocated, and a merger may be
allocating above them. The row below is a placeholder id; the merger
allocates it (D148, invariant 8).

### `docs/decisions.md` — no entry

**None.** D268 is the entry for this rule and landed with batch 26.

### `docs/orchestration/board.md` — one row for *Owed*

```markdown
| ORCH-NEXT | **D268's code citations, re-read against the tree it landed in, and one of them moves with `job.rs`.** Read on `d62e2cb`: `confine.rs:106` and `root.rs:317` are exact; `root.rs:443` is the `measure` signature and the `measure_tree` call is `:444`; `job.rs:1555` is exact on `main` and was `:1499` at `1df941b`, because batch 26 changed that file; `root.rs:538` is a line of `read_dir_restoring`'s doc comment (the fn is `:541`) and `root.rs:664` is a line inside `restore_access`'s body (the fn is `:658`). No fact in D268 is wrong and `decisions.md` is append-only, so nothing is corrected in place. The row exists so the next agent on ORCH-291 or ORCH-292 quotes the sites by name rather than re-deriving six line numbers, and so nobody quotes `job.rs` by line again without re-reading it. | the prose step of 2026-10-30, `history/2026-10-30-d258-mode-fixture-exception.md` | herd-step | owed | Cite `read_dir_restoring`, `restore_access`, `POLL_INTERVAL` and `measure_tree` by name in any new text; re-read a `job.rs` line before quoting it | D268 | `history/2026-10-30-d258-mode-fixture-exception.md` |
```

## Decisions needed

**None for Alexa.** This is prose about *how* a test is written and
changes nothing about what the product is or promises. Nothing is
escalated and no permission was refused.

## Resume point

`herd-merger`: carry `d258-mode-fixture-exception` into the next batch
off `main` (`d62e2cb` or later) and run the batch's own
`./scripts/gate.sh` on the integrated tip — the four Rust gates have not
been run on this branch and are owed there. Place the drafted board row.
Then, separately, the owed work of the class is **ORCH-292** first (the
hollow green, branch `settle-confinement-hollow`, cut and empty) and
**ORCH-291** after it.

## Processes

**None started, none ended, none signalled.** No Herd job was submitted,
no build was run, no `CARGO_TARGET_DIR` was created and there is no
`target/` to delete: this branch changed one prose file and ran two
shell scripts that need no toolchain. PID 662711 (the queue's runner),
565152 (our hub) and 3006543 and 3021002 (the consumer's) were never
read and never touched.
