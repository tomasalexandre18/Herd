# pack-registry-gate — handoff 2026-10-30

- **Item:** M11's pack half, step **S6**, second pass — gate the
  `pack-registry` branch and put `LAYOUT_VERSION` on the release body's
  machine-readable lines, in the same branch that moves it
- **Agent:** herd-step
- **Branch / commit:** `pack-registry` / this file's commit, on top of
  `f96523e` *"Read LAYOUT_VERSION into the release body too"*, which is on
  top of `9b36871` *"Build the hub's pack registry and move record and
  layout to 2"*, based on `pack-proto` `adbc204` merged with origin/main
  `1a8c1ae`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

This file **continues** the branch's own handoff,
`docs/orchestration/history/2026-10-30-pack-registry.md`, and does not replace
it. That handoff holds the registry's shape, the three
refusals verbatim, the six acceptance cases and the five revert experiments;
nothing of it was re-derived and nothing in it was edited. What is here is the
one change added on top, the gate its author could not run, and what the gate
and a few read-only checks say about that handoff's claims.

## Done

### Part one — the number the release page would not have announced

`.github/workflows/release.yml` read `PROTOCOL_VERSION` and `RECORD_VERSION`
out of the source and put each on a line the consumer's own tooling can match.
It did not read `LAYOUT_VERSION`. Verified at `:151-205` before touching it,
exactly as its author described: two `sed` patterns, one D168 refusal loop over
two entries, two `GITHUB_ENV` lines, and a body carrying *"The two numbers this
build carries"*.

`LAYOUT_VERSION` moves 1 → 2 on this branch — the first time in the project's
history — and it is the one of the three with no migration path at all. So the
first release to move the most destructive number would have been the one
release not to put it on a matchable line.

**One commit, `f96523e`, `.github/workflows/release.yml` only, six hunks:**

| Where | What |
|---|---|
| `:151-170` | the step's comment, from *"The two numbers"* to *"The three numbers"* (eight lines longer), with `LAYOUT_VERSION`'s paragraph: refused before the port is bound, nothing converts a root in either direction, equality and not a floor, and **why it is on a line of its own** — a script matching two of three numbers concludes a move is safe when it is not |
| `:171` | the step's name, `the protocol, the record and the layout version, from the source` |
| `:176`, `:183-185` | `layout_src=herd-hub/src/store.rs` and a third `sed`, the same pattern as the other two, anchored on `^pub const LAYOUT_VERSION: u32 = \([0-9]\{1,\}\);.*$` |
| `:200` | the D168 refusal loop gains `"LAYOUT_VERSION:$layout_version:$layout_src"`. **The loop body is unchanged** — the third number gets the identical treatment, which is what the brief asked for |
| `:216`, `:219` | `layout_version` into `$GITHUB_ENV`, and the step's own log line names all three |
| `:452-481` | the published body: `LAYOUT_VERSION: 2` at `:457`, between the other two, and a paragraph saying what crossing it costs; *"Both lines are meant to be matched"* → *"All three lines"* at `:480` |
| `:428-434` | the dev lane's rollback sentence now names **both** state-root numbers, not only the record's |

**The three extractions, re-run by hand on this tree**, the whole step's body
including the refusal loop:

```
the source says PROTOCOL_VERSION 3, RECORD_VERSION 2, LAYOUT_VERSION 2
exit 0
```

`3`, `2`, `2` — the figures its author reported, from the moved lines.

**The new guard was proved to fire, on three known-bad fixtures**, because a
refusal nobody has watched refuse is not a refusal. Each is a copy of
`herd-hub/src/store.rs` in this agent's scratchpad; the worktree was not
touched:

| Fixture | Read | Verdict |
|---|---|---|
| the declaration deleted outright | empty | **REFUSED** |
| `pub const LAYOUT_VERSION: u32 = TWO;` | empty | **REFUSED** |
| declared twice, `= 2;` and `= 3;` | `2\n3` | **REFUSED** — the second match brings a newline, and a newline is not a digit |
| `= 2; // a trailing comment` | `2` | accepted, as the comment above the loop promises |

**The body renders.** The `body-head.md` block was extracted and run with this
tree's figures, and the three lines a script matches came out:

```
RECORD_VERSION: 2
LAYOUT_VERSION: 2
PROTOCOL_VERSION: 3
```

`^RECORD_VERSION: [0-9]+$`, `^LAYOUT_VERSION: [0-9]+$` and
`^PROTOCOL_VERSION: [0-9]+$` each match exactly one line of the rendered body.

**What the body does not claim.** `deploy/release-notice.md`'s section is
called *"The two numbers in the release body"* and covers `RECORD_VERSION` and
`PROTOCOL_VERSION` only. That file is not this task's, so the body now says
what to do about `LAYOUT_VERSION` **inline** — take the `herd-hub report
--state-dir` reading off the root with the old release's binary first, then
move the root aside, do not convert it, do not delete it — and says plainly
that the notice's section *"predates this line and covers the other two"*
rather than pointing at coverage that is not there. Finding 3.

### Part two — the gate, which is green

Its author could not run this code once. It has now been run, on a quiet host,
with nothing lifted. **All four Rust gates and `check-docs.sh` are green, and
the suite passed on the first attempt**: 1 006 assertions passed, 0 failed, 39
ignored, 12 skipped — this host's at-rest expected list exactly. Output under
*Gates*.

`admission` passed. That is the author's one non-timeout red, and it was the
host: at a slice room of 9 829 MiB the runner could lend the 1.5 GiB
`admission.rs:456` asks for, so the assertion that went red on a true statement
about a crowded host is green on a quiet one. The diff was never implicated,
and finding 1 explains why that red was likelier on this branch than the brief
expected.

**No tenth version-sensitive site exists in the code**, by the suite and by six
read-only searches that found the nine already fixed and nothing else; the
detectors and what each one cleared are under *Risks and findings*, finding 6.

## Not done, and why

- **Nothing in the registry, the record or the two constants was changed.** The
  gate found nothing to fix, so there was nothing to touch. `9b36871` stands
  unamended and unsquashed.
- **`deploy/release-notice.md` is untouched**, so its *"The two numbers in the
  release body"* section covers two of the three numbers the body now prints.
  Not this task's file. Finding 3.
- **`.github/workflows/release-dev.yml` is untouched**, and its `:17` comment
  is now stale by one number. Not this task's file; the brief named
  `release.yml` alone. Finding 2.
- **No prose outside the code was edited**, so the five stale sites its author
  found are still stale and one of them is worse than it looks. Finding 4.
- **The three findings the brief reserved were left**, and are named here so
  that leaving them is on the record, not an omission:
  - the record refusal's **three wordings** across `job.rs`, `report.rs` and
    `retention.rs` were not converged;
    `herd_hub::record_version_mismatch_reason` is still exported at the crate
    root and still takes the `Option<u32>` the other two readers produce,
    ready for the follow-up;
  - `packs/` still has **no retention, no sweep and no budget**; the board row
    is drafted in full below;
  - `PackRegistry::open` still re-hashes every manifest at every start and is
    still **unmeasured at scale**; nothing was timed here.
- **`docs/decisions.md`, `docs/orchestration/board.md`, `docs/plan.md` and
  `docs/operating.md` are untouched**, per the brief. The decision-entry
  correction and the board rows are drafted below.

## Gates

**Verdict: all four Rust gates and `check-docs.sh` GREEN**, on the tree of
`f96523e` — that is, on everything in this branch except this file.
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-registry`, a cold tree (its
author deleted the old one on purpose).

```
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 05s
exit 0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.02s
exit 0
(0 lines beginning `warning` or `error`)

$ nix develop --command cargo fmt --all --check
(no output)   exit 0

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit 0

$ nix develop --command ./scripts/test.sh
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
exit 0
```

Summed over every `test result:` line of that run: **1 006 passed, 0 failed,
39 ignored**. No `failures:` block and no `test result: FAILED` anywhere in the
log.

**Skips: 12, each one named, none counted as a pass (D27).** The twelve are
this host's at-rest expected list, byte for byte the names in
`scripts/expected-skips/local.txt`, and all twelve are the same family — cgroup
v2 delegation is unavailable, so this runner cannot create a cgroup under
`/sys/fs/cgroup/chatons.slice/chatons-herd-claude-256608.scope` (Permission
denied):

`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
`a_declared_ceiling_changes_what_the_inert_weight_line_says`,
`a_job_cannot_fork_past_its_own_pid_ceiling`,
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`,
`a_job_s_record_carries_the_pressure_it_ran_under`,
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`,
`an_io_weight_that_cannot_bite_is_said_out_loud`,
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`,
`no_declared_ceiling_leaves_io_max_empty`,
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`,
`the_runner_says_which_memory_ceiling_it_enforces`.

Neither flood bench skipped: the host was quiet, so
`scripts/expected-skips/local-under-load.txt`'s two names are absent and the
list is the 12 and not the 14.

**The last run, named and not quoted** — it is about the tree this file is
committed into, so its output cannot be in this file (`templates/handoff.md`):

```
$ ./scripts/gate.sh          # bare, NOT inside nix develop (SUN_LEN, confinement.rs)
```

Its receipt lands in `target/herd-gate/<sha>.receipt` in this worktree, and its
log in this agent's own scratchpad subdirectory:

```
<scratchpad>/pack-registry/gate2/gate.log
```

Verbatim output is in the agent's report. The branch is pushed only on
`gate: GREEN`, and a red one is reported red.

**Preflight readings, before each heavy phase, nothing lifted.**
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was **never set and never exported** —
not inline on a command either. Nothing under `/sys/fs/cgroup` was written and
no cache was dropped; `memory.current`, `memory.high` and `memory.stat` of
`/chatons.slice` were read, which the brief allows.

| Phase | load (1 m) / ceiling | slice room / floor | verdict |
|---|---|---|---|
| `cargo build --workspace` | 3.65 / 9.00 | 13 475 MiB / 8 192 | ok |
| *(before clippy)* | **17.21** / 9.00 | **2 411 MiB** / 8 192 | **REFUSED, waited** |
| `cargo clippy` | 2.63 / 9.00 | 12 406 MiB / 8 192 | ok |
| `./scripts/test.sh` | 2.67 / 9.00 | 9 829 MiB / 8 192 | ok |

`cargo fmt --all --check` compiles nothing and was not treated as a heavy
phase. Between the refusal and the clippy clear, a poller read the preflight
every 20 s and waited for **two consecutive clears**: 24 attempts, both clauses
refusing at times, cleared at attempt 24. A second waiter then held the suite
until no other agent's `gate.sh` or `test.sh` was running — one had started at
00:58 — so the suite ran with **one** suite on the host and not three. The
preflight was read again immediately before each heavy command; the figures
above are those readings.

## Risks and findings

Ordered by what is hardest to repair later.

**1. The brief's premise about `room-honest` does not hold for this branch, and
it is what reddened its author's suite.** `room-honest` is commit `7c75525`,
*"Stop holding reclaimable page cache against a runner's room"*, and it is on
`origin/main` at `5f73c18` — **batch 10**. This branch merged main at
`1a8c1ae`, batch 9, so:

```
$ git merge-base --is-ancestor origin/main HEAD
origin/main is NOT an ancestor
$ grep -c 'inactive_file\|reclaim' scripts/bench-preflight.sh          # this tree
0
$ git show origin/main:scripts/bench-preflight.sh | grep -c 'inactive_file\|reclaim'
8
```

So the `bench-preflight.sh` in this worktree is the **pre-`room-honest`** one:
it subtracts `memory.current` once (`:170`) and nets out no page cache.
Measured during the refusal, `/chatons.slice` read `memory.current` 17 808 MiB
against `memory.high` 18 432 MiB, of which `file` was 14 480 MiB and
`inactive_file` alone **13 240 MiB** — reclaimable cache from sibling build
trees. The refusal was therefore the old pessimistic reading of a host with
22 GiB available, not the honest one the brief describes.

**The product half matters more than the script half.** `room-honest` changed
`herd-runner/src/room.rs` too, so on `origin/main` a runner nets reclaimable
cache out of the room it announces. On *this* branch it does not — which is
exactly why `admission.rs:456` saw *"12 MiB free of its 1 GiB"* where it wanted
1.5 GiB: the runner announced 1 GiB because this branch's `room.rs` still holds
page cache against it. The author's conclusion was right (the runner was right,
the host was the cause) and the deeper reason is that the fix already existed
on `main` and this branch cannot see it.

**Nothing was merged to get it.** The brief set this branch's base and did not
ask for a second merge of `main`, and a merge would have changed what gets
gated and pushed and would have touched `herd-runner/*` and `herd-proto/*`,
both held by other agents tonight. The gate is green without it. It resolves
itself at integration, where the branch sits on top of batch 10 — and that is
the reason to prefer the batch's own `gate.sh` as the authority on
`admission` rather than this one.

**2. `.github/workflows/release-dev.yml:17` is now stale by one number, and
this change is what made it stale.** Its comment argues the dev lane is nine
lines and not a second workflow, listing what `release.yml` does for it:

> the same `RECORD_VERSION` and `PROTOCOL_VERSION` read out of the source with
> `sed` and asserted numeric (D168)

Three are read now. The lane itself is correct — it calls `release.yml`, so it
gets the third number for free and needs no code change — only the sentence is
short. The brief named `release.yml` and not this file, so it was left. It is a
two-word fix and belongs with whoever next opens `.github/`.

**3. `deploy/release-notice.md`'s section is named for a count that moved.**
*"The two numbers in the release body"* (`:48`) now describes two of three. The
body is honest about it rather than silent, and says the section covers the
other two — but the notice is quoted in full underneath, on two published
release pages, so a reader meets a heading that disagrees with the block above
it. The remedy is a third paragraph in that section and a heading that says
three; the file is not this task's.

**4. The stale prose is real, and one site is worse than its author said.**
Every figure below was read, not assumed. None of these files is this task's.

| Site | What it says | Why it is false now |
|---|---|---|
| `docs/operating.md:555-563` | *"the release body prints the two that matter"*, then a table: `PROTOCOL_VERSION` **2**, `RECORD_VERSION` **1**, `LAYOUT_VERSION` **1** | three are printed now, and **all three figures are wrong** — the protocol's was already wrong before this branch (`pack-proto` moved it to 3) |
| `docs/architecture.md:345-346` and the line after | `RECORD_VERSION` **1**, `LAYOUT_VERSION` … **1** each; and *"`RECORD_VERSION` is frozen at 1 since M16"* | both figures are 2, and the freeze sentence is now the opposite of true |
| `docs/handover.md:65` | `PROTOCOL_VERSION` **2** | already wrong before this branch; not caused here |
| `docs/disk-policy.md:769` | *"`objects/` **must not move `LAYOUT_VERSION`**, which is 1 `[C, store.rs:59]`"* | the figure is 2 and the citation is stale — the constant is at `store.rs:99`. The **argument** still stands; only its premise and its pointer rotted |
| `docs/disk-policy.md:1462` | ORCH-171's acceptance criterion: *"`LAYOUT_VERSION` unchanged at 1"* | **unsatisfiable as written.** It is a planned row whose criterion pins a literal this branch moved. Whoever takes ORCH-171 must read it as *unchanged at 2* — the intent (deduplication by hard link moves no layout number) is untouched, the number is not |

`docs/disk-policy.md:1462` is the one to carry forward: a stale figure in prose
is read by a person, but a stale figure in an acceptance criterion is read as
an instruction.

**5. One of its author's stale-prose claims is refuted, in the branch's
favour.** The handoff lists `docs/operating.md:560-563` as now false. The table
there is false, as above — but the **quoted refusal** two paragraphs below, at
`:576-578`, is

```
the state root /var/lib/herd has layout version 1, and this hub writes
version 2
```

which was written as an illustration when both numbers were 1, and is now
**literally what a v0.4.1 root gets from this build**. It is also still a
correct prefix of the longer sentence `layout_version_mismatch_reason` now
formats, verified against `herd-hub/src/store.rs:3345-3360`. That block needs
no edit and became more accurate, not less; the table above it needs one.

**6. No tenth version-sensitive site, and these are the detectors that say
so.** The suite is the first of them and it is green. Five static searches ran
over the workspace, each cleared with the site it found:

- every `record_version`/`layout_version` key compared against a literal →
  three hits, all correct: `crash_points.rs:346` `{"record_version":99}` and
  `store.rs:4388` `{"layout_version":99}` are the deliberate out-of-range
  fixtures the brief said to leave, and `store.rs:4031`'s
  `{"record_version":1,"at":"2026` is a **torn, unterminated** line that is
  never parsed — the test asserts only the whole line is an event, so it means
  the same at any version;
- `assert*!(…version…, 1|2)` → three hits, all intended pins:
  `pack_registry.rs:69` pins `LAYOUT_VERSION` at 2, and
  `frozen_formats.rs:124` / `frozen_verdict.rs:287` pin
  `OUTPUT_FORMAT_VERSION` and `VERDICT_FORMAT_VERSION` at 1, neither of which
  this branch moves;
- `contains("…version…")` with a literal → nine hits, every one relative (a
  build-version string, or a peer number passed in as an argument);
- prose `version 1` / `version 2` inside `.rs` → comments and
  `herd-proto`'s peer-refusal arguments, where `version 2` is the *peer's*
  number and `PROTOCOL_VERSION` is pinned at 3 beside it;
- the non-Rust corpus for all four spellings → `release.yml`,
  `release-dev.yml` and three files under `history/`; no JSON or text golden
  carries a version at all.

A warning about that list, from its author's own measurement and not from
caution: a **half-moved** build left every behavioural test green, because each
is written relative to the constant it is about. So a green suite is not
evidence that no fixture means the wrong thing — it is evidence that nothing
*this* tree asserts is false. The literal pins are what catch an accidental
move, and they stayed literals.

**7. Two suite runs were deliberately not made concurrent.** Another agent's
`gate.sh` started at 00:58 and this agent's suite waited for it rather than
make two suites compete, which is what cost this branch its gate an hour
earlier. Clippy ran alongside it on purpose: it compiles but starts no hub, no
runner and no load-sensitive assertion.

## Decisions needed

1. **The drafted `D-NEXT` entry's last paragraph is now false and is replaced
   below.** Its author drafted the entry in full in the previous handoff, and
   its *"Cost, and what is not decided"* paragraph ends:

   > The release body's machine-readable lines still carry only
   > `PROTOCOL_VERSION` and `RECORD_VERSION`, and `LAYOUT_VERSION` — the number
   > with no migration path at all — is not among them.

   That was its *Decision 2*, and this branch answered it yes. **Replace that
   whole paragraph with this one, verbatim; the rest of the drafted entry
   stands unchanged:**

   ```markdown
   **Cost, and what is not decided.** `packs/` has no retention, no sweep and
   no budget: the registry only grows, one directory per `(commit, key)`. The
   removal delay and the archive path stay parked for Alexa. The release
   body's machine-readable lines now carry **all three** numbers —
   `.github/workflows/release.yml` reads `LAYOUT_VERSION` out of
   `herd-hub/src/store.rs` the same way it reads the other two, under D168's
   refusal, because a consumer's script matching two of three would conclude
   a move was safe when it was not. What is not done is the prose around it:
   `deploy/release-notice.md`'s section is still called *"The two numbers in
   the release body"* and covers two of the three, and the body says so
   rather than claim coverage it does not have.
   ```

   *Recommendation:* the merger allocates the number and takes the entry with
   that paragraph substituted. *Fallback:* take the entry as drafted and append
   a one-line correction — worse, because the false sentence would be in the
   permanent record of the very branch that falsified it.

2. **Who owns `packs/`'s growth** — its author's decision 3, unchanged and not
   this task's to settle. Both board rows are drafted in full below.
   *Recommendation:* file the two rows tonight and put the removal policy in
   front of Alexa with the removal delay and the archive path already parked.
   *Fallback:* leave it, and accept that a directory created by this layout
   move ships with no retention story.

3. **Does the third number need a test, or is the release workflow still
   untested on purpose?** Nothing in the tree asserts that the release body
   carries three matchable lines; the three extractions and the D168 refusal
   were proved by hand here and on four fixtures, and that evidence lives in
   this handoff and nowhere a gate reads. The same was true of the first two
   numbers, so this changes nothing — but it is now three numbers and a
   consumer's script depends on all three.
   *Recommendation:* leave it. `gate.sh` already says the release path is what
   it cannot cover, and a shell test over a workflow file would pin the `sed`
   rather than the behaviour. *Fallback:* a `check-docs.sh` case asserting the
   workflow reads one `sed` per declared public version — cheap, and it would
   have caught this very omission.

4. **Whether `admission`'s green here is enough, given finding 1.** This
   branch's `room.rs` predates `room-honest`, so its runner still holds
   reclaimable page cache against the room it announces; `admission.rs:456`
   passed only because the slice was quiet.
   *Recommendation:* treat the batch's own `gate.sh`, run with this branch on
   top of batch 10, as the authority on `admission` — that tree is the one the
   consumer gets, and it has the fix. *Fallback:* nothing; there is no second
   measurement to take on this base.

### Drafted board rows, in full

```markdown
| ORCH-nnn | **`packs/` has no retention, no sweep and no budget.** The hub's pack registry creates one directory per `(commit, key)` under `packs/manifests/<id>/` and nothing ever removes one: `docs/retention` and the output budget govern `files/` and not this. Kilobytes each, unbounded in count, in a directory this layout move introduces. Name the owner and the policy before v1.0.0 | Alexa | waiting | the removal delay and the archive path already parked (plan §8) | `docs/disk-policy.md`, retention |
| ORCH-nnn | **One record-version refusal, one sentence.** Three wordings exist after this batch: `measured-fold`'s in `report.rs` and in `retention.rs`, and `pack-registry`'s longer one in `herd-hub/src/job.rs`. Point the first two at `herd_hub::record_version_mismatch_reason` — exported at the crate root, and it takes the `Option<u32>` both already produce — and keep `report.rs`'s *"NOT ONE RECORD COULD BE PARSED BY THIS BUILD"* banner, which says something the shared sentence does not (invariant 4) | herd-step | owed | both branches landed | `report.rs`, `retention.rs`, `job.rs` |
| ORCH-nnn | **The three version figures in prose are wrong in five places**, and one is an acceptance criterion: `docs/operating.md:555-563` (table, and *"the two that matter"* — three are printed now), `docs/architecture.md:345-346` plus *"frozen at 1 since M16"*, `docs/handover.md:65`, `docs/disk-policy.md:769` (figure **and** a stale `store.rs:59` citation, now `:99`), and `docs/disk-policy.md:1462`, where ORCH-171's criterion *"`LAYOUT_VERSION` unchanged at 1"* is unsatisfiable as written and must read **at 2**. `operating.md:576-578`'s quoted refusal needs nothing: it became literally true | herd-step | owed | batch 11 landed | the five docs |
| ORCH-nnn | **Two stale sentences in `.github/` and `deploy/`**, caused by the third number: `release-dev.yml:17` still says *"the same `RECORD_VERSION` and `PROTOCOL_VERSION`"*, and `deploy/release-notice.md:48`'s section *"The two numbers in the release body"* covers two of three — the body says so inline rather than claim coverage it lacks, which is the stopgap and not the fix | herd-merger | owed | batch 11 landed | `.github/`, `deploy/` |
```

## Resume point

For whoever takes **S7** (`server.rs`): the previous handoff's *Resume point*
stands unchanged and is the one to read — the registry's whole surface,
`register`/`resolve`/`announced`/`runner_gone`/`forget_all_holdings`, which
frame each belongs to, that `Registered::Refused(why)` is
`FromHub::PackRefused` and never `FromHub::Refused` (D13), and D150's
arrival-window lesson.

For whoever takes this branch next, there is nothing to run: it is gated and
pushed. To confirm it where it was left:

```sh
cd <worktree> && git log --oneline -3 && ./scripts/gate.sh --verify-receipt "$(git rev-parse HEAD)"
```

For the merger: this branch is `pack-registry`, two commits of its own plus
this file, and the one thing it needs from integration is a `gate.sh` on top of
batch 10 — see finding 1 and decision 4.

## Processes

- **Three background shells of this agent's own**, each a poll loop that only
  **reads**: a preflight waiter (24 attempts at 20 s, waiting for two
  consecutive clears), a host-quiet waiter (14 attempts, waiting for both the
  preflight and the absence of any other agent's suite), and the suite run
  itself. Each exited on its own. **No PID was signalled** — no `pkill -f`, no
  `killall`, no kill by name — and no process of another agent was touched;
  another agent's `gate.sh` was waited out, not disturbed.
- **At most one compile at a time**, and never two suites: the suite waited for
  the other agent's `gate.sh` to finish. Clippy ran beside it deliberately
  (finding 7).
- `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-registry`, off the root
  filesystem (gotcha `build-weight-off-root`), built cold. **`rm -rf`'d once
  the branch was pushed**, per `workflow.md`; the agent's report carries the
  command and the reclaimed figure.
- Nothing under `/sys/fs/cgroup` was written, no cache was dropped,
  `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set — not exported and not
  inline. Nothing on the consumer's instance, `/etc/nixos`, containers or
  services was touched (D161).
- Logs and the four `LAYOUT_VERSION` fixtures, all in this agent's own
  scratchpad subdirectory, `<scratchpad>/pack-registry/gate2/`, whose absolute
  path is in the agent's report: `build.log`, `clippy.log`, `fmt.log`,
  `suite.log`, `gate.log`, `preflight-wait.log`, `quiet-wait.log`,
  `body-head.md`, `fixtures/`.
