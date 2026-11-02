# joining-drain-and-three-owed — handoff 2026-11-02

- **Item:** ORCH-358, plus four undocketed debts the brief named
- **Agent:** herd-step
- **Branch / commit:** `joining-drain-and-three-owed` / `7dca107` plus
  the follow-up commit this file is amended in, based on origin/main
  `c824fc8`. `7dca107` was verified by the orchestrator, which re-ran
  both gates in this worktree itself and kept the branch; the follow-up
  is three further prose edits on the same branch, not a rewrite
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Five prose edits, no code. Each fact below was read off the tree in this
worktree, not taken from the brief.

**1. `docs/joining.md` no longer says that a `drain` leaves the runner
running.** The paragraph that began *"Neither position stops the
runner"* sat eleven lines under the table row saying a `drain` *"leaves
once it holds nothing"*, and the table row is the correct one:
`herd-runner/src/lib.rs:4203` polls
`switch.leaves_when_empty() && session.jobs_running() == 0` and, when
both hold, stops the process through the path a `SIGTERM` stops it
through — the comment at `:4191`-`:4195` says so in those words, and
`Switch::Drain.leaves_when_empty()` is asserted `true` by
`herd-runner/src/switch.rs`'s `each_position_gates_exactly_what_it_should`
(`:380`). The replacement paragraph, now `docs/joining.md:739`-`:758`,
is true of each position separately and carries three things:

- `off` leaves the process up — claiming nothing, serving nothing, but
  still reading the switch file every second, which is what lets `on`
  put the runner back with nobody starting anything.
- `drain` **exits the process** once the runner holds nothing, so it is
  the position for *take this machine out of the herd* and not for
  *stop claiming for a while*.
- under a unit with `Restart = "always"` a `drain` is therefore a leave
  followed by a start, and that start reads the persisted `drain` as
  **`off`** with the owner's reason kept (**D164**,
  `herd-runner/src/switch.rs::at_startup` at `:115`), the runner saying
  one sentence on its own stderr when it converts. The outage that rule
  closed — eight restarts in two minutes, and a runner never connected
  long enough to be switched back `on` — is recorded in that file's test
  `a_persisted_drain_starts_a_runner_off_and_not_leaving` (`:279`).

The sentence about `systemctl --user stop herd-runner` is kept: it is
true, and it is the third way out.

**2. `docs/operating.md:329`-`:335`, one sentence.** Appended to the
bullet *"The runner figures are read at its startup too"*, which is one
bullet under *"Stopping the hub is cheap and loses nothing"* and is
therefore where a reader meets a hub recreate: for a window where the
runner must stay reachable throughout, the position is `off` and never
`drain`, because `off` claims nothing, serves no content and leaves work
in flight alone while keeping the process up, so
`herdctl switch <runner> on` reaches it from anywhere afterwards, where
`drain` exits the process and is for withdrawing the machine. One
sentence and its wrap; no section.

**3. ORCH-358 — `docs/orchestration/gotchas/docs-only-gates.md`.** Three
edits. **The closure covers both of that file's PR-era clauses, not one,
so the `done` row is not read against a file that still has the
defect.**

- *Expected now* carried the pre-ORCH-357 five-entry exempt list. It now
  reads `` `CLAUDE.md`, `README.md`, `docs/`, `.claude/`, `.github/*.md`
  and only the markdown under `deploy/` ``, quoted from
  `CLAUDE.md:84`-`:85` in this tree and word for word with it. *"a
  change touching only"* became *"a change confined to"* so that the
  clause's own *"only the markdown under `deploy/`"* is not read against
  a second *only*; the list itself is unaltered. Glob-free, as that
  document has no engine to evaluate a glob.
- *Verify* asked for a green `check` job *"on the PR"*. It now names the
  handoff's *Gates* section and the batch's one landing —
  `./scripts/gate.sh` on the gated sha, and `herd-merger`'s filled
  landing record — and says in a clause that there is no pull request to
  look at, nobody opening one but `herd-merger`, once, for the batch's
  one landing (Alexa, 2026-10-28).
- **The second touch, in the same sentence block and disclosed rather
  than folded in — reported as beyond the brief, and accepted by the
  orchestrator rather than reversed:** *Expected now* also required the
  report to say *"that CI covers them"*. Rewriting *Verify* to the
  landing would have left two different coverers one bullet apart, so
  that clause now reads *"that the landing's `gate.sh` covers them"*,
  matching `CLAUDE.md:87`-`:88`.
- **The third touch, and the one that completes the class: the *Why*
  clause at `:9`-`:10`.** It read *"CI runs all four on every PR anyway
  (`ci.yml` has no path filter)"* and now reads *"on the batch's one
  landing anyway"*, the parenthesis kept because it is still true. It
  was first reported as deliberately left, on ORCH-358's instruction not
  to restate the warrant in the *Why*; the orchestrator read that
  instruction as forbidding a restatement of the **warrant** and not the
  repair of a **stale clause**, and that reading is the better one —
  closing a row about PR-era language in this file while PR-era language
  survived nine lines up would have made the closure partial and left
  the next reader the same drift in the same place. One clause changed,
  no explanation added to the bullet. **No clause in that file now
  attaches a gate run to a pull request**, which is what makes
  ORCH-358's `done` safe to read against the file.

ORCH-358's row names two edits; these are three, each inside or beside
the block that row sent me to, and all three are recorded in that row.

**4. ORCH-291's *Evidence* cell and *Next action*,
`docs/orchestration/board.md:179`.** The cell cites two files, not one,
because the gate log is not the richest trace the red run left. The
gate log:

```
.claude/worktrees/cut-v1.1.0/target/herd-gate/gate-20261101T203402Z-bfd238e3df87.log
```

for the two lines that make it the sighting — `test
a_real_runner_condemns_nothing_when_the_hub_cannot_list_its_jobs ...
FAILED`, and the tail `gate: RED
sha=bfd238e3df870d4a2db64a7e6add0483ecfdbf4a failed=6:test-suite
skips=at-rest`. And the suite's own output:

```
.claude/worktrees/cut-v1.1.0/target/herd-gate/test-output-bfd238e3df87.txt
```

**2359 lines**, which holds the failure itself: the panic at
`herd-hub/tests/owed_scan_fails_against_the_real_runner.rs:431:5` —
that file's `:766`-`:767` — and `test result: FAILED. 0 passed; 1
failed` at `:887`. Both are cited because a reader sent only to the gate
log has to go looking for the panic.

`gate.sh` writes no receipt on RED, deliberately — `scripts/gate.sh:73`-
`:75`, *"a run that cannot stand behind its own verdict writes NO
receipt, rather than a receipt with a caveat in it"*, proved red by
mutation at `scripts/check-orchestration.sh:1400` — so these two files
are what a red sha leaves, which is why they are cited rather than left
to be rediscovered. `.claude/worktrees/cut-v1.1.0` is recorded as
**deliberately retained** on the orchestrator's word, for them and for
the cut's receipt, and not a worktree left behind by an oversight: a
clean-up of stale worktrees would take the only evidence of this
defect's one sighting in the wild.

**The panic names its own mechanism, so the row stops being a bare flake
and becomes a hypothesis with a named check** — *"If the jobs directory
is 0o711 rather than the 0o111 the fixture set, the host gave the access
back before the hub scanned: see AListingThatFails::still_binds."*
**Written with its qualifier, which is the part that matters:** for
neither sighting is that mechanism measured. Nobody captured the mode at
the scan for the v1.1.0 red or for batch 40's green, and the panic's own
sentence is conditional, so the cell says the mechanism is **offered by
the fixture, written in advance, and unconfirmed for both sightings** —
never that it is the cause.

What *is* measured, and the cell separates it from the two sightings: the
mechanism in the abstract, on a third and earlier occasion.
`herd-hub/tests/a_failed_listing/mod.rs:101`-`:104` records job
`j0000000147`, 2026-10-30 — `mode=2111 read_dir=EACCES` at t=0.10 s,
`mode=2711 read_dir=ok` at t=0.20 s, and *"`0o711` where the fixture set
`0o111` — never the guard's own `0o755` — is the fingerprint"*. That job
id is the one this row already carried.

**Who hands the mode back is named in the tree and was not a lead to
chase:** the runner carrying this very suite, every
`herd_runner::confine::POLL_INTERVAL` — 200 ms — because `measure_tree`
over `Workspace::base_dir` holds a job to its disk budget and
`read_dir_restoring` calls `restore_access`,
`set_permissions(mode | 0o700)`. That is invariant 2 and D166 working as
required, a job not `chmod`-ing its way out of a figure the runner
enforces (`a_failed_listing/mod.rs:17`-`:25` and `:84`-`:99`). Cited by
name and not by line for the four symbols, per ORCH-301.

The red run's state root corroborates that guard's **precondition** and
nothing further: the hub's listening line,
`test-output-bfd238e3df87.txt:775`, puts the run inside a Herd job's
workspace under `$TMPDIR`:

```
/mnt/ssd_980/herd/queue/runner-data/jobs/j0000000232/tmp/nix-shell.NbZC5U/nix-shell.OzbGO6/.tmptkoFv9
```

which is exactly where that restoration applies. The cell offers it as a
lead, not a finding.

**The discriminator added to *Next action*, to run first, is the
instrument the fixture already ships:** `AListingThatFails::still_binds()`
(`a_failed_listing/mod.rs:105`), one call, which `owed_scan_fails.rs:340`
and `:500` already make and this member does not — taken **at the scan's
instant**, because that file's comment at `:418`-`:430` says reading the
mode afterwards proves nothing, the hub's own sentence being the only
honest witness of *when* the scan ran, and says in the same breath why
this member cannot simply retake its measurement. The rest of *Next
action* stands as it was. **No reproduction was attempted.**

**5. `docs/coverage.md:233`, E-46's note**, which was empty while E-45
above it carried its caveat. The boundary existed only in
`herd-runner/src/privilege.rs:20`-`:28` and is now in the row, in the
register of its neighbours: a uid is not the whole of E-46, a user in
`docker` is root-equivalent through the daemon's socket and a user in
`wheel` can become root with one password, the runner cannot refuse
those without refusing to run on the machines Herd exists for — the
owner's own server is one — so it names them at startup as a warning and
lets the machine's owner decide, *"saying nothing would be the failure:
it would let a deployment read as unprivileged when it is not"*.

**6. ORCH-358's state is `done`** in the same row, dated 2026-11-02 to
this branch. **Its closure covers both of the gotcha's PR-era clauses**
— *Verify* and *Why* — and the row says so, so the merger does not read
a `done` row against a file that still carries the defect. The row also
records the three edits against the two it names, and which of them the
orchestrator accepted and which it instructed.

## Not done, and why

- **No row was added to `board.md` for either new finding.** Number
  allocation is `herd-merger`'s at integration (batch 40's precedent:
  `Allocate D296 and ORCH-358..360`), so both rows are below in full,
  ready to paste, with no id.
- **No `docs/decisions.md` entry.** The brief asked for none, and
  nothing here decides anything: every edit replaces a false or missing
  statement with what the code already does, under rules already
  numbered (D164, D34, invariant 2, Alexa 2026-10-28).
- **`herdctl` is untouched.** The second finding is being fixed on
  `herdctl-stopping-names-no-cause` this batch; it is filed below as
  context so the two rows cross-reference, as the brief asked.
- **`docs/status.md` is untouched**, as instructed; it is at exactly 90
  lines, its cap (`scripts/check-docs.sh:40`).
- **The reproduction of ORCH-291's red is not attempted**, as
  instructed. No suite ran; the mode was not captured, and the row says
  for both sightings that it was not.
- **The four Rust gates were NOT RUN**, see *Gates*.

## Gates

Prose only: no `.rs`, no `Cargo.toml`, no script, nothing outside
`docs/`. **The four Rust gates — `./scripts/test.sh`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all --check` and the suite's skip accounting — were NOT RUN
locally.** They are not reported as passed, because they did not run.
What covers them is the batch's own `./scripts/gate.sh` run at
integration, which is `herd-merger`'s and not this agent's. No `cargo`
was invoked, no job was submitted and no compile was started on this
host.

Verbatim, from the run made on the edits before this file was committed.
The orchestrator also re-ran both gates in this worktree itself at
`7dca107`, the first commit pair, and got the same two verdicts:

```
$ ./scripts/check-orchestration.sh
check-orchestration: 361 passed, 0 failed
```

```
$ ./scripts/check-docs.sh
docs ok
```

**The last run** is `./scripts/check-docs.sh`, on the tree with this
file committed, run from this worktree before the push; it writes no
file, so its output is in the agent's report and not here, as this
template requires. `check-orchestration.sh` is re-run with it, since
this change edits `docs/orchestration/`.

**Skips:** no skip. No suite ran, so there is no `HERD-SKIPPED` line to
copy and no skip count to account for.

## Risks and findings
- **Two counts in the briefs were refuted by the tree, and the row
  carries the counted figures. Both are settled.** (1) The first brief
  described `.claude/worktrees/cut-v1.1.0` as retained for the red log
  *"and for v1.1.0's two receipts"*. Counted on 2026-11-02, that
  directory holds **one** receipt —
  `19ef7ebca8eb84ce8eccf3707b607cc8f63ad5f4.receipt`, verdict GREEN,
  branch `merge-batch-39` — beside **two** run logs and their skip and
  ignore lists; `find` over the whole repository returns eleven
  receipts, one per worktree that gated, and only that one under
  `cut-v1.1.0`. The orchestrator has dropped its figure and confirmed
  the counted one, noting that *"two receipts"* contradicted its own
  statement that `gate.sh` writes none on RED — it had counted the two
  logs. (2) The follow-up said the red run left **11** files; counted in
  the same directory on 2026-11-02 it left **ten**, all suffixed
  `bfd238e3df87`: the gate log, the suite output, and the eight skip and
  ignore lists. The cell says ten. Neither figure changes anything that
  follows from it: the red run left no receipt, the two cited files are
  what a reader needs, and the worktree is worth keeping for them.
- **The journal line in the row material is a trimmed excerpt, and the
  row says so.** `herd-runner/src/lib.rs:4305` prints
  `herd-runner: draining ({reason}); {n} job(s) to end, no new job
  accepted`. The line relayed from the consumer carries neither the
  `herd-runner: ` prefix nor `, no new job accepted`. Nothing in it is
  contradicted — the reason inside the parentheses is `:4203`'s
  leave-when-empty sentence carrying their own switch reason, exactly as
  `switch.sentence()` renders a `Drain` — but the excerpt is quoted as
  an excerpt, so that nobody later greps for it as a whole line.
- **No test asserts the sentence `docs/joining.md` now carries.** The
  three facts are each asserted in Rust (`:4203`'s poll, D164's
  conversion, the position table), but nothing ties the document to
  them: a future change to `at_startup` would not go red on this
  paragraph. That is the ordinary state of this repository's prose and
  not a regression; it is written down because this is the second
  document in two days found stating the opposite of its own code.
- **The gotcha is still a second live copy of the gate rule.** ORCH-358
  closes today's drift, not the mechanism: `CLAUDE.md:84`-`:85` and this
  file still both carry the list in prose, and ORCH-359 is the sized
  question of whether `check-docs.sh` should hold it mechanically. The
  third live holder, the ORCH-357 row, is unchanged and consistent.
- **ORCH-291's mechanism is measured on a third occasion and
  unconfirmed on both sightings, and the row must keep saying so.** The
  two are easy to collapse into *"the host handed the mode back"*, which
  would be a true mechanism in a word it has not earned. The 2026-10-30
  measurement in job `j0000000147` establishes that the restoration
  happens and how fast; it establishes nothing about the v1.1.0 red or
  about batch 40's green, for which the mode at the scan was never
  captured. Whoever edits that cell next should keep the two apart.
- **The lead about *"whatever hands access back"* was already answered
  in the tree, which is why the cell names it instead of pointing at a
  temp directory.** The follow-up offered the run's nix-shell state-root
  path as the place to look. The restorer is `read_dir_restoring` →
  `restore_access`, driven by `measure_tree` over `Workspace::base_dir`
  every `POLL_INTERVAL`, and it is documented at
  `a_failed_listing/mod.rs:17`-`:25` and `:84`-`:99`. The path is kept
  in the cell as corroboration of that guard's precondition — the run
  really was inside a Herd job's workspace under `$TMPDIR` — and
  labelled a lead, not a finding. ORCH-301 asks for those four symbols
  by name, and the cell complies.
- **Not reproduced, and not attempted:** ORCH-291's red. This is a prose
  task; no suite ran. *Next action* keeps everything it had and gains
  the one-call discriminator in front of it.

## Drafted for `herd-merger`

No `docs/decisions.md` entry. Two `board.md` rows, both for **Owed —
agreed, not started** (`| ID | Item | Origin | Owner | State | Next
action | Depends on | Evidence |`), ids to be allocated at integration.
Paste verbatim; replace `ORCH-NEXT-A` and `ORCH-NEXT-B` with the two
numbers allocated, and replace the cross-reference in each row's *Item*
cell with the other's allocated number.

Row A, the false sentence itself:

| ORCH-NEXT-A | **`docs/joining.md` told an owner that a `drain` does not stop the runner, and an operator chose `drain` on it.** Fixed on `joining-drain-and-three-owed`; the row exists for the class, not the sentence. `:740` read *"Neither position stops the runner"* eleven lines under the table row saying a `drain` *"leaves once it holds nothing"* — the document contradicted itself on one screen, and the half a reader reaches second was the false one. **The chain is measured end to end, not inferred.** The consumer set `drain` to make a hub recreate safe; their server runner's journal, at **01:11:04 CET on 2026-11-02**, reads `draining (the owner's switch is draining (hub upgrade to 1.1.0 …) and this runner holds nothing, so it leaves); 0 job(s) to end` — a trimmed excerpt of `herd-runner/src/lib.rs:4305`, which also prints a `herd-runner: ` prefix and `, no new job accepted`, carrying `:4203`'s leave-when-empty reason with their own switch reason inside it. Then their unit's `Restart` started it again, and `at_startup` read the persisted `drain` as `off` (D164). **No signal was ever sent**, which is why they came to us asking what had restarted their runner. `0 job(s) to end` matters: nothing was in flight, so the leave abandoned no work — **the defect is that the document let them pick the position that exits, not that anything was lost**. They have corrected their own playbook to `off` for a hub recreate, which is now the sentence at `docs/operating.md:329`-`:335`. **What is owed is the class, not this line:** no gate ties a sentence about runner behaviour to the code asserting it, so the next such contradiction is found the same way — by an operator. Sized as a question, not a fix: whether anything mechanical can hold a claim like this, and if not, that the answer is written down once rather than rediscovered. See ORCH-NEXT-B, the same failure one layer out: a sentence a program emits about a cause it cannot know | the consumer, 2026-11-02, via the orchestrator: they used `drain` to make a hub recreate safe, their runner left once idle, their unit restarted it, and they asked us what had restarted their runner. Herd is shipped, not operated (D161), so the journal line reached us as a report | herd-planner | owed | Size the class: whether a claim about runner behaviour in `docs/joining.md` or `docs/operating.md` can be held by anything mechanical, and if it cannot, record that once. **Do not reopen the sentence** — it is corrected and its three facts are cited to `herd-runner/src/lib.rs:4203`, `herd-runner/src/switch.rs::at_startup` and that file's `each_position_gates_exactly_what_it_should` | nothing | `docs/joining.md:739`-`:758` as corrected, and `c824fc8:docs/joining.md:739`-`:744` for what it said; `herd-runner/src/lib.rs:4191`-`:4205` and `:4305`; `herd-runner/src/switch.rs:91`-`:138` and its test `a_persisted_drain_starts_a_runner_off_and_not_leaving` at `:279`, which records the eight-restarts-in-two-minutes outage D164 closed; `history/2026-11-02-joining-drain-and-three-owed.md` |

Row B, filed as context so the two rows cross-reference. **It is being
fixed on `herdctl-stopping-names-no-cause` in this same batch** — if
that branch lands with the batch, this row lands `done` with its
evidence, and the merger should check that branch's handoff before
pasting it as `owed`:

| ORCH-NEXT-B | **`herdctl` names a cause it cannot know.** The same failure as ORCH-NEXT-A one layer out: there, a document asserted a runner behaviour the code refutes; here a program asserts a reason for a runner's state that nothing in its own inputs establishes. Filed from the prose side as context only — **this row was not investigated here and no claim about `herdctl`'s code is made in it**; the branch `herdctl-stopping-names-no-cause` holds the measurement and the fix, in this same batch. The pair is worth keeping visible together because the two were found a day apart from the same operator confusion: a runner that stopped, and nobody able to say from the outside what stopped it. A `drain` that converts to `off` at startup says its one sentence on the **runner's own stderr** (`herd-runner/src/switch.rs::at_startup`), which is the machine's journal and not anything a requester sees, so `herdctl` is where an operator looks and is the one place that cannot see it | the prose step of 2026-11-02 on `joining-drain-and-three-owed`, relaying the parallel branch; the finding itself is that branch's | herd-step | owed | Read `herdctl-stopping-names-no-cause`'s handoff first and close this row against it rather than redoing it; `herdctl` was deliberately not touched from the prose side | `herdctl-stopping-names-no-cause` landing in this batch | `herdctl-stopping-names-no-cause` and its handoff; `herd-runner/src/switch.rs:91`-`:138` for the sentence that goes to the runner's stderr and nowhere a requester reads; ORCH-NEXT-A |

Already in the tree on this branch and needing nothing from the merger
but the line-by-line board read (`workflow.md`, *The merged board is read
line by line*): ORCH-291's *Evidence* cell at `board.md:179`, and
ORCH-358 flipped to `done` with its closure at `:212`.

## Decisions needed

None. Nothing here is structural, nothing is irreversible, and nothing
touches the consumer's side. The one question this work raises — whether
a claim about runner behaviour in the operator-facing documents can be
held by anything mechanical — is sized as row A and is the planner's,
not Alexa's: it is *how*, not *what*.

## Resume point

The branch is pushed and ready for the batch; nothing further is owed
on it. For whoever continues the drain class: read
`docs/joining.md:739`-`:758` and `herd-runner/src/switch.rs:91`-`:138`
side by side, and size row A. For whoever continues ORCH-291: add
`still_binds()` at `owed_scan_fails_against_the_real_runner.rs:431` and
run that one test, which costs one compile and decides between a hub
defect and a fixture whose premise the host took back.

## Processes

No process was started and none was signalled. No `cargo`, no job, no
`herdctl`, no `pkill`. Nothing was read from or written to the
consumer's instance, `/etc/nixos`, `/srv`, a container or a service.

No `target/` of this task's own: the change is prose, nothing was
compiled, and `CARGO_TARGET_DIR` was never set, so there is nothing to
delete under `/mnt/ssd_980/herd/build/`. The worktree is
under this agent's own scratchpad:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt/joining-drain-and-three-owed
```

and
`.claude/worktrees/cut-v1.1.0` was read and not written.
