# stale-comments — handoff 2026-10-30

- **Item:** ORCH-207 (closed here); two new rows proposed below
- **Agent:** herd-step
- **Branch / commit:** `stale-comments` / see the push, based on
  `origin/main` `5f73c18`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Three jobs, one branch, four files.

### 1. A drafted decision now has to be in the committed handoff

`docs/orchestration/templates/handoff.md` gains two things.

A prose block before the `herd-merger` paragraph states the rule: a
drafted `docs/decisions.md` entry and a drafted `board.md` row go in the
handoff **in full**, headed `D-NEXT` for a decision, never as a pointer
and never as a summary — **because the merger reads the tree and nothing
else**, so a draft that lives only in an agent's hand-back report or in
the orchestrator's conversation does not reach integration. It names the
instance it comes from: `spool-offer-fix` deferred its entry to *"the
agent's report"* on 2026-10-29, `herd-merger` composed D221 out of that
handoff's *Done* section instead, and
`history/2026-10-29-merge-batch-10.md` says so.

The template's markdown block gains a section, **`## Drafted for
herd-merger`**, placed before `## Decisions needed`. The template said
nothing close before this; its only decision-shaped section was
*Decisions needed*, which is for a question somebody else must answer,
not for an entry already written. That section is unchanged and the new
one stands beside it, which the prose block also says in as many words.

`docs/orchestration/workflow.md` step 4 of *The loop* gains the matching
paragraph, with the same reason, the same instance and a pointer to the
new section name. It is the only other place in the tree that describes
what a handoff owes.

### 2. Two comments in `herd-runner/src/main.rs`, and a third

Line numbers below are the merged tree's at `5f73c18`.

`:470-471` (the `switch` subcommand) and `:523-524` (the output cap) are
the two the brief and `board.md` ORCH-207 name. A **third** carried the
same false claim as the first and is fixed with it: `:320-321`, the
`--switch-file` doc comment, *"A file rather than a socket, because
nothing on a runner's machine ever listens (invariant 3)"*. It is the
same sentence in the same file about the same mechanism; leaving it
would have left ORCH-207 half done.

Both no-listener comments now say that a runner listens on nothing with
no mesh address configured, that the one socket `--mesh-listen` permits
serves a job's artefacts to peers rather than taking an instruction, and
that `mesh.rs` holds the rule for which addresses may ever be served
from.

The output-cap comment now says there is no frame-shaped ceiling left,
that `herd_proto::INLINE_OUTPUT_MAX` (32 MiB) is the threshold between
the inline and streamed carriages rather than a cap (D148), that
`--job-disk-budget` is what bounds the figure, and that a cap above it
is **clamped** by `Confined::prepare` with both figures printed rather
than refused (D89's precedent) — which is what the `--job-output-cap`
help text beside it already says.

### 3. The 79-column rule now covers `deploy/`

`scripts/check-docs.sh`'s width loop gains `deploy/*.md` and
`deploy/release-notes/*.md`, and a comment above the list says why,
what it cost, and why the globs are `.md` only and named one directory
at a time rather than `deploy/*/*.md`.

## Not done, and why

- **`.claude/agents/herd-step.md` was not edited**, as the brief
  instructed. It says *"no write to `docs/decisions.md` except a new
  entry headed `D-NEXT` when the brief asks for one (the merger
  allocates the number)"* and, in its closing paragraph, that the agent
  reports *"decisions needed"* to its caller. **Neither statement is
  wrong and neither is complete**: the role file never says where a
  drafted entry goes, so an agent reading it alone could put the entry
  in its report and satisfy every word of it. That is the hole this
  branch closes in the template and in `workflow.md`, and closing it in
  the role file too is one sentence. It is a finding, not an edit.
- **Nothing outside the four allowed paths.** The stale no-listener
  claims found in `herd-proto/src/lib.rs` and `herd-hub/tests/events.rs`
  are under *Risks and findings*.
- **No `docs/decisions.md` write, no `board.md` write, no `main`, no
  merge, no tag, no pull request.**

## Gates

All four Rust gates ran before this file was committed, plus
`./scripts/check-docs.sh` and `./scripts/check-orchestration.sh`.
Verbatim output is in the agent's report; their verdict lines:

```
cargo fmt --all --check                     exit 0, no output
cargo build --workspace                     Finished `dev` profile in 28.07s
cargo clippy --workspace --all-targets      Finished `dev` profile in 19.64s
                                 -- -D warnings
./scripts/test.sh                           test suite: PASSED
                                            14 HERD-SKIPPED lines
./scripts/check-docs.sh                     docs ok
./scripts/check-orchestration.sh            291 passed, 0 failed
```

**`check-orchestration.sh` is 291 passed, 0 failed — no delta.** That is
the same figure `main` measures at `5f73c18`, and this branch edits
`scripts/check-docs.sh`, which that script has cases about. No case
moved, so there is nothing to account for case by case.

**The 14 skips are the blessed `local-under-load` shape, not a new
number.** `scripts/expected-skips/local.txt` lists 12 at rest and
`local-under-load.txt` adds exactly the two load-gated flood benches
(`flood-fifty`, `flood-two-fifty`, skipped here at load 9.84 against
their ceiling of 8). 12 + 2 = 14. The other 12 are the cgroup-delegation
refusals this host always makes. Every one is printed as a skip and
counted as a skip (D27, invariant 5); the full list is in the report.

`./scripts/bench-preflight.sh` **refused on its first run and was
waited out, not lifted.** The refusal was on **load**, not on the cgroup
floor, so the brief's permitted inline lift never applied:

```
load (1m):   14.55 on 12 cores, ceiling 9.00
slice room:  8900 MiB under memory.high on /chatons.slice, floor 8192 MiB
             charge 17041 MiB, of which 7509 MiB is reclaimable page cache
bench-preflight: REFUSED, do not start the heavy phase.
```

The slice-room line **passed** on that very run — 8900 MiB against a
floor of 8192 — so the preflight's arithmetic, fixed in the batch this
branch is built on, cleared the cgroup check on its own.
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set, never lifted inline
and never exported into any gate's environment.

A backgrounded loop re-ran the preflight every 30 s and the heavy phase
started only once it cleared, after nine waits:

```
load (1m):   4.42 on 12 cores, ceiling 9.00
slice room:  8197 MiB under memory.high on /chatons.slice, floor 8192 MiB
bench-preflight: ok, safe to start.
```

The last thing run on this branch is the bare `./scripts/gate.sh`
(never inside `nix develop`), after this file was committed, with its
output in the agent's report and its receipt where `gate.sh` writes it.
The output of that run cannot be in this file, which is the tree it
gates.

Skips: 14, named above and listed in full in the report, each printed as
a skip and counted as a skip, never as a pass (D27).

## Risks and findings

1. **The brief's premise about `:470-471` is half right, and the
   correction matters.** D222 landed the mesh-address **rule**, not a
   listener: `herd-runner/src/mesh.rs`'s own header says *"It binds no
   socket"*, and `main.rs`'s judging site says *"this version holds the
   rule and no endpoint"*. So *"nothing listens on this machine"* was
   still **literally true** of the shipped binary at `5f73c18`; what
   D222 made false is the reasoning, because the comment cited invariant
   3 for an absolute the invariant no longer states. The new wording is
   written to be true today **and** still true once the endpoint binds,
   which is why it says `--mesh-listen` *permits* a socket rather than
   *opens* one. A reader who checks the claim against the binary today
   will find no listening socket, and the comment does not promise one.
2. **The same false claim is in two files I may not edit.**
   `herd-proto/src/lib.rs:1436` and `:1451` both justify a frame with
   *"invariant 3 — nothing listens on a runner"*, and
   `herd-hub/tests/events.rs:324` repeats it. Same shape as the three
   fixed here: literally true today, and an invariant cited for an
   absolute it no longer states. Worth one board row and folding into
   whoever next edits those files, not a branch.
3. **`deploy/release-notice.md` was checked and is *not* stale.** Its
   *"It listens on nothing"* section is a **measured** claim about the
   running deployment (`ss -tnap`, zero listening sockets), which is
   still correct, and it already carries the parenthetical *"A future
   milestone, M11, lets runners serve content to each other. When it
   arrives a runner may listen on a private mesh interface."* No edit
   needed. Reported because the first scan flags it.
4. **`docs/joining.md`'s "nothing on your machine listens" bullet is
   already tracked** as `board.md` ORCH-159, which requires it corrected
   in M11's pack change. Not touched.
5. **`herd-runner/src/main.rs:59`** reads *"The hub to dial. The runner
   opens this connection; nothing listens here."* Left alone as a
   judgement call: *"here"* scopes it to that connection's local end,
   which stays true. Named so the next reader does not have to decide
   again.
6. **The `deploy/` extension is a detector I proved fires**, not a
   silent zero. 120 `x` characters appended to
   `deploy/release-notes/0.4.1.md` produced
   `deploy/release-notes/0.4.1.md:130 (120 cols)` and
   `docs check failed`; the file was reverted with `git checkout --` and
   `git status --short` confirmed it. Without that, "all five files
   pass" would have been indistinguishable from a glob that matched
   nothing.
7. **What the `deploy/` extension costs: nothing today, and that is
   measured.** All five markdown files under `deploy/` pass the
   fence- and table-aware count unchanged. `deploy/README.md` (longest
   raw line 346 columns) and `deploy/release-notice.md` (447) have **no**
   finding, because every long line in them is inside a fence or a
   table — which is also the reason the raw maximum is not the figure to
   quote. `deploy/release-notes/0.4.0.md`, `0.4.1.md` and
   `release-notes/README.md` sit at 72 columns. Zero new findings, so no
   existing file was reflowed and no release notes text was touched.
8. **Why the globs are not `deploy/*/*.md`.** `deploy/` holds one other
   subdirectory, `reverse-proxy/`, whose two files are an nginx config
   and a Traefik YAML; `deploy/*/*.md` matches no markdown there today
   and would silently adopt whatever the next subdirectory holds. The
   non-prose files in `deploy/` — `docker-compose.yml`,
   `docker-compose.traefik.yml`, `env.example`, `herd-runner.service`,
   `nixos-herd-runner.nix`, `capabilities-wsl-pc.json`, `wsl2-check.sh`
   — are not `.md` and are matched by neither glob, so no YAML is
   width-checked.
9. **`check-orchestration.sh` risk, checked.** Its corpus builder copies
   `deploy/release-notes` into the fixture and nothing else under
   `deploy/`, so the two new globs find three files there and
   `[ -f "$f" ] || continue` covers the rest. Its three release-notes
   cases blank or rewrite those files, which produces no width finding.
   The figure is in the report.

## Drafted for `herd-merger`

Two board rows and one `docs/decisions.md` entry. **In full, here, for
the reason this branch exists**: the merger reads the tree and nothing
else.

### Drafted `docs/decisions.md` entry

```markdown
## 2026-10-30 — D-NEXT A drafted decision lives in the handoff, not in a report

**Context.** A `herd-step` drafts a `docs/decisions.md` entry and
`herd-merger` places it, because the merger is that file's only writer
(invariant 8). On 2026-10-29 `spool-offer-fix` put its drafted entry in
its hand-back report to the orchestrator and wrote in its committed
handoff only that the entry was *"deferred to the agent's report"*. The
merger never receives those reports: it reads the branch. So D221 was
**composed from that handoff's own *Done* section**, which
`history/2026-10-29-merge-batch-10.md` states plainly, flagging the
entry to re-read. The orchestrator re-read it and it was faithful.
Nothing in the method had made it so.

**Choice.** A drafted `docs/decisions.md` entry and a drafted
`board.md` row go in the **committed handoff, in full** — the whole
text the merger will paste, headed `D-NEXT` for a decision since the
merger allocates the number and gaps stay (D148). Not a pointer, not
*"see the agent's report"*, not a summary. The reason is stated with the
rule, in one sentence, because a rule whose reason is missing is one
somebody reinvents: **the orchestrator's conversation is not an artefact
the merger can read, so anything the merger must act on has to be in a
file on the branch.** Written in `templates/handoff.md`, as a prose
block and as a new `## Drafted for herd-merger` section, and in
`workflow.md` step 4, which is the only other place that describes what
a handoff owes.

**Not a new duty, a located one.** Nothing here asks an agent to draft
more than it drafted before. The entry was always owed; this says where
it is written down. The agent's report keeps the drafted text too — a
report is for the orchestrator and loses nothing by repeating it.

**Left to a deliberate change.** `.claude/agents/herd-step.md` says
only that an agent may write a `D-NEXT` entry when the brief asks, and
never where a draft goes; an agent reading the role file alone can still
put it in its report and break no written rule. Agent files are role
definitions and change on their own decision, not folded into a task
(the orchestrator's instruction, 2026-10-30). One sentence, when it is
decided.
```

### Drafted `board.md` rows

`ORCH-207` is **closed** by this branch. Its row's *State* becomes
`done` and its *Recommendation* column gains, appended to what is there:

```markdown
**Done 2026-10-30 on `stale-comments`.** Both comments rewritten
against the code, plus a **third** carrying the same false claim —
`main.rs:320-321`, the `--switch-file` doc comment's *"nothing on a
runner's machine ever listens"*. The no-listener claims now say a runner
listens on nothing with no mesh address configured and that `mesh.rs`
holds the address rule; the output-cap comment now says
`INLINE_OUTPUT_MAX` is a carriage threshold and that `Confined::prepare`
**clamps** against the disk budget. **Correction to this row's premise:**
D222 landed the rule and binds no socket, so *"nothing listens"* was
still literally true of the binary — what it made false was citing
invariant 3 for an absolute the invariant no longer states. The new
wording holds both today and after the endpoint binds. See
`history/2026-10-30-stale-comments.md`.
```

Two new rows, at the next free ids (`ORCH-209` is the highest on `main`
at `5f73c18`):

```markdown
| ORCH-210 | **The same stale invariant-3 absolute is in two files a `main.rs` task may not edit.** `herd-proto/src/lib.rs:1436` and `:1451` justify `SetSwitch` and `SwitchApplied` with *"invariant 3 — nothing listens on a runner"*, and `herd-hub/tests/events.rs:324` repeats it. Literally true of the binary today, because D222 binds no socket — and an invariant cited for an absolute it no longer states, which is how an invariant gets quietly misread. Same defect `stale-comments` fixed in `herd-runner/src/main.rs` | found by `stale-comments` while scanning the merged tree, 2026-10-30; `history/2026-10-30-stale-comments.md` *Risks and findings* 2 | herd-step | open | **Fold into whoever next edits `herd-proto/src/lib.rs`**, not a branch of its own: three comment lines, and the wording `stale-comments` settled on is there to copy. Checked and **not** stale, so not in scope: `deploy/release-notice.md`'s *"It listens on nothing"*, which is a measured `ss -tnap` claim that already carries its M11 parenthetical | if nobody edits those files before the fetch endpoint lands, the endpoint's own task corrects them, because that is when the claim stops being literally true | **D222**; `herd-proto/src/lib.rs:1436`, `:1451`; `herd-hub/tests/events.rs:324`; `herd-runner/src/mesh.rs` header |
| ORCH-211 | **`.claude/agents/herd-step.md` does not say where a drafted decision goes, and the template now does.** The role file says an agent may write a `D-NEXT` entry when the brief asks and that it reports *"decisions needed"* to its caller; neither sentence is wrong and neither says the drafted text belongs in the committed handoff. An agent reading the role file alone can still put the entry in its report, which is what cost D221 its author's wording | `stale-comments` was told to leave `.claude/agents/*` alone and report instead, 2026-10-30; `history/2026-10-30-stale-comments.md` *Not done* | Alexa / orchestrator (role files change deliberately) | open | One sentence in `herd-step.md` pointing at the handoff's *Drafted for `herd-merger`* section. **Agent files are role definitions**: decide it, then brief it, rather than folding it into a task — which is the instruction this row exists because of | the rule is already in `templates/handoff.md` and `workflow.md` step 4, which is what an agent is contractually handed, so the gap is a second copy missing and not an unwritten rule | `docs/orchestration/templates/handoff.md`; `docs/orchestration/workflow.md` step 4; `.claude/agents/herd-step.md` |
```

## Decisions needed

1. **Should `.claude/agents/herd-step.md` carry the rule too?** —
   *Recommendation:* yes, one sentence pointing at the handoff section,
   as its own deliberate change to a role file. *Fallback:* leave it;
   `templates/handoff.md` and `workflow.md` are what a brief hands an
   agent, so the rule is reachable either way. Drafted as `ORCH-211`.
2. Nothing else. The `deploy/` question was decided here, with figures,
   and is under *Risks and findings* 6 to 9.

## Resume point

Nothing on this branch. For `herd-merger`: take the `D-NEXT` entry and
the three board rows above verbatim from *Drafted for `herd-merger`*.
For the orchestrator: decide `ORCH-211`, and brief `ORCH-210` into the
next task that opens `herd-proto/src/lib.rs`.

## Processes

PIDs: one backgrounded shell that re-ran `./scripts/bench-preflight.sh`
every 30 s until it cleared, started and ended by this agent; the cargo
and shell gate processes, each run in the foreground and exited. No
`pkill`, no kill by name, nothing of the consumer's instance.

Own build tree deleted once the branch was pushed:
`/mnt/ssd_980/herd/build/stale-comments`. The worktree's own `target/`
was never written, because `CARGO_TARGET_DIR` was exported for every
cargo command.
