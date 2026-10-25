# c5-announced-job-ceiling — handoff 2026-10-25

- **Item:** ORCH-060 (C5, second half)
- **Agent:** herd-step
- **Branch / commit:** `runner-holds-its-own-limits` / `aeb2f3e`, based on
  `origin/main` `199e4df`. **No rebase was needed**: `origin/main` was still
  `199e4df` and is already an ancestor of the branch
  (`git merge-base --is-ancestor origin/main HEAD` exits 0).
- **PR:** opened from the template on this branch. **Not merged** —
  `herd-merger` merges.

This is the second commit of C5's work on this branch, on top of `aa4699a`
(the runner's own ceiling) and `6e12a8f` (its handoff). One PR for the whole
of C5, as briefed.

## Done

**Alexa's D-C of 2026-10-25**, quoted in the decision entry:

> D-C : oui. `#[serde(default)] jobs_at_once: Option<u32>` sur Hello.
> Absent = « n'a pas dit ». Aucune version ne bouge. C5 peut livrer.

1. **`jobs_at_once: Option<u32>` on `ToHub::Hello`**
   (`herd-proto/src/lib.rs`, after `host`), `#[serde(default,
   skip_serializing_if = "Option::is_none")]`, documented in
   `health_every_ms`'s register: why the figure travels, and that `None`
   means *the peer did not say* — never unlimited, never zero, never a
   figure derived from `host`.
2. **The runner fills it** from the ceiling it already computes.
   `serve_once` now derives `confine::jobs_at_once(&config.host,
   confined.caps().memory_max)` **once, before the handshake**, sends that
   value in the hello and hands the same `Arc` to `serve_hub`, which is
   where every assignment is held against it. Previously `serve_hub` derived
   it itself; there is now one derivation and no way for announced and
   enforced to drift.
3. **The hub honours it**, in `herd-hub/src/matcher.rs`, at the top of
   `admission`, against `seat.reserved.jobs`. `Announced`,
   `ConnectedRunner` and `Seat` each carry the field; `seats()` passes it
   through. A machine at its ceiling is `Admission::ShortNow` →
   `Verdict::Later` → `Placement::Wait`; a machine announcing **zero** is
   `NotHere`, so nothing waits for it.
4. **The runner still enforces locally whatever the hub does.** Nothing in
   this commit makes the local ceiling conditional on the hub; the
   enforcement site in `take_job` is untouched.

**Neither version moved.** `PROTOCOL_VERSION` is 2
(`herd-proto/src/lib.rs:113`), `RECORD_VERSION` is 1
(`herd-hub/src/job.rs:52`). Neither line is in the diff — `git diff
origin/main -- herd-proto/src/lib.rs herd-hub/src/job.rs | grep -E
'PROTOCOL_VERSION|RECORD_VERSION'` returns nothing — and
`announced_job_ceiling::neither_version_number_moves` asserts both.

**No `JobEventKind` variant was added.** The stop condition in the brief did
not fire: `JobEventKind::Waiting { reason }` already existed and is exactly
the right reason, so the enum is untouched. `announced_job_ceiling` also
asserts that every `events.jsonl` line written in a capped run is one of the
eight kinds that already existed.

### Tests added

| Test | File | What it settles |
|---|---|---|
| `a_hello_carries_its_job_ceiling_in_both_directions_without_moving_the_version` | `herd-proto/tests/round_trip.rs` | Old peer → today: a literal `main`-shaped hello decodes with `jobs_at_once: None`. Today → old peer: a hello carrying the field is read by a struct of exactly the fields the older `Hello` had. Plus: `None` puts the key on the wire **not at all**. |
| `no_attempt_is_spent_on_a_ceiling_the_hub_was_told_about` | `herd-hub/tests/announced_job_ceiling.rs` | **The oracle.** Real hub, real runner announcing 2, eight memory-silent jobs. |
| `a_queue_well_inside_the_announced_ceiling_is_paced_by_nothing` | same | A runner nowhere near its ceiling is paced by nothing; the hub invents none. |
| `the_figure_announced_is_the_one_the_runner_derives` | same | The announcement equals `jobs_at_once`'s own arithmetic, and a silent host announces 1, never 0. |
| `the_hub_never_derives_a_ceiling_of_its_own` | same | Reads the hub's shipped source; fails if any line fills a ceiling in. |
| `neither_version_number_moves` | same | Both constants, asserted. |
| six tests in `matcher::tests` | `herd-hub/src/matcher.rs` | Silent runner unchanged (40 jobs on one machine); fills to the announced figure and refuses in the same pass; the two bounds and either biting; a ceiling of zero; readings counted; held-then-moved past the stickiness limit. |
| `welcome_a_runner` now asserts the announcement | `herd-hub/tests/fake_hub_flood.rs` | The hello carries exactly `EXPECTED_CEILING`, and this hub then **ignores it on purpose** — the rollback case. |

### Measured, on the real hub

`herd-hub/tests/announced_job_ceiling.rs`: hub + real runner announcing **2**
(4 GiB lent, 2 GiB floor, 1 GiB a job), **8 memory-silent jobs** with eight
*distinct* commands.

| | before the hub half | after |
|---|---|---|
| peak attempts out on the runner at once | 7 | **2** |
| attempts spent **because of the ceiling** | 12 | **0** |
| attempts spent for anything else | 0 | **0** |
| attempts recorded in all, for 8 jobs | 14 | **8** |
| jobs that reached their own verdict | 2 of 8 | **8 of 8** |
| jobs that ended `lost`, no verdict | **6 of 8** | **0** |

Both columns measured today on this branch, by running the same test with
and without the gate in `admission`. The "before" is therefore a
measurement, not a recollection.

**`fake_hub_flood.rs` still shows 8 held and 242 handed back**, re-run today
and unchanged, as the orchestrator's correction required.

## Not done, and why

- **Nothing on the consumer's side**, no deployment, nothing outside the
  repository (D161).
- **No new flag.** The ceiling is still derived from `--host-memory`,
  `--host-memory-floor` and `--job-memory-max`; the hub gets the derived
  figure and nothing else.
- **The hub does not sanity-check the announced figure against `host`.**
  Deliberate, and argued in §7.1 of the remediation page: the runner's
  arithmetic also uses the per-job cap, which no frame carries, so any check
  the hub made would be against an incomplete picture. Listed as an **H** in
  §7.7.
- **`docs/status.md` not touched** — it is the merger's, per the workflow.

## Gates

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/c5-runner-cap`, removed after the
run. `./scripts/bench-preflight.sh` run before each heavy phase and green
each time (`340 GiB free`, `load 0.87–0.97 on 12 cores`, `14 GiB available`).

```
$ nix develop --command cargo build --workspace
   (clean)

$ nix develop --command cargo fmt --all --check
   (clean)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
   (clean)

$ nix develop --command ./scripts/test.sh
test suite: PASSED
   795 assertions passed, 0 failed
   12 skipped
```

```
$ ./scripts/check-docs.sh
docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates the number at integration:
8595:## 2026-10-25 — D-NEXT A runner holds its own ceiling
8690:## 2026-10-25 — D-NEXT A runner announces its ceiling and the hub honours it
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
note: D174 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs check failed
```

**That is the expected failure**, and it is the *only* complaint:
`gotchas/a-decision-branch-cannot-be-all-green.md`. The two notes about D170
and D174 are gaps held by open PRs and are notes, not failures. Nothing was
done to make it green — the only way is to allocate a number, which an agent
may not do.

```
$ ./scripts/check-orchestration.sh
check-orchestration: 86 passed, 1 failed
FAIL  unmodified copy passes
    docs check failed
```

**Same failure, cascaded.** That check runs `check-docs.sh` on an unmodified
copy of this tree and expects it to pass; it fails for the one reason above
and for no other. It will pass once the merger allocates the two numbers.

### Skips — 12, the standing reference, unchanged

10 for cgroup-v2 delegation and 2 for the ZFS IO ceiling, exactly the set
`09-runner-self-limits.md` §4 names. **The flood bench's load guard did not
fire**, so its conditional `HERD-SKIPPED` does not appear, as expected on a
quiet host. No new skip was introduced by this work.

## Risks and findings

### Four things the brief was wrong or silent about

1. **The brief's oracle was the wrong bench, and the correction was right.**
   The brief asked `fake_hub_flood.rs` to go from 242 handed back to 0. Its
   "hub" is a hand-written WebSocket server that floods a real runner on
   purpose and will never honour anything announced; making it show 0 would
   have meant teaching the fake hub the cap, which proves nothing about the
   real one. I had reached this conclusion from the file before the
   orchestrator's correction arrived, and the correction's oracle — *attempts
   spent **because of** the ceiling = 0, other attempts unchanged* — is what
   the new test asserts. The bench keeps its 242 and now also asserts that
   the announcement arrived and was ignored, which is the rollback case
   stated rather than assumed.

2. **Eight identical submissions are one job, not eight** (E-05). My first
   version of the real-hub test submitted `sleep 1` eight times; `herdctl`
   joined them into `j0000000001`, so `place` ran **once** in the whole run,
   `state_of` reported the same job eight times, and the test read "8
   attempts out at once" on a hub that had placed one. It cost about forty
   minutes of chasing a cap that was working perfectly. The test now submits
   eight distinct commands and asserts `submitted` holds eight distinct ids,
   with the reason beside it.

3. **The damage is much worse than "an attempt is spent", and nobody had
   measured it.** The brief said each bite costs one of the job's attempts.
   With `attempts_allowed = 2` it costs *the job*: six of eight ran out of
   goes on refusals and ended `lost`, with no verdict, each carrying *"this
   was attempt 2 of 2 at a runner that went away"*. The remediation page's §5
   predicted this in words; nothing had put a number on it. That is the real
   before/after of this change, and it is now row 6 of the table in §7.5.

4. **`D-NEXT`, not `D-C`, belongs in code comments.** The brief told me to
   record Alexa's label D-C. I first wrote `D-C` in 29 Rust comments —
   which the merger's `D-NEXT` grep would not have found (see the gotcha
   `a-decision-branch-cannot-be-all-green.md`, "Also allocate the number
   in the code"), leaving 29
   dangling references after integration. All 29 now read `D-NEXT`. In the
   prose, `D-C` is kept only where it names Alexa's own word.

   Minor, in the same vein: the brief gave `jobs_at_once`'s signature as
   taking `(host, memory_max)`, which is right; and it placed `Hello` at
   `:1157` and `health_every_ms` "around `:1240`", both right. The one
   stale detail is in the repository rather than the brief —
   `peer_version`'s own doc comment says *"That is what moved
   `PROTOCOL_VERSION` to 8"* and `switch`'s says 5, while the constant is 2.
   Pre-existing, not touched, and worth a cleanup task.

### Risks in the change itself

- **H — the ceiling is a memory ceiling wearing a general name.** A machine
  whose real constraint is cores, disk bandwidth or file handles announces a
  figure that says nothing about it. Deliberate; §7.7 says what would settle
  whether a second figure is wanted.
- **H — the hub trusts the figure.** A runner announcing a very large
  ceiling is placed on as though it had none (which is today's behaviour,
  so not a regression); one announcing a very small figure holds its own
  fleet back, visibly, through the `waiting` sentence.
- **F — the upgrade order matters.** Runners newer than their hub pay
  exactly what they paid before this change. **Hub first.** Written down in
  §7.6 and in the decision entry.
- **The stickiness interaction is a behaviour change worth a reviewer's
  eye.** A job held by a ceiling now *sticks* to its preferred machine for
  up to `--stickiness-limit`, where before it would have been sent, refused
  and requeued free to go elsewhere. That is correct — a job ending there is
  what frees the place, which is `ShortNow`'s own contract — and it is
  asserted both ways in
  `a_job_held_by_a_ceiling_waits_for_that_machine_and_then_moves`, but it is
  the one place where a capped fleet behaves differently rather than simply
  better.

### The brief's four questions, answered plainly

**Where the hub counts a runner's live attempts, and why that place.**
`matcher::admission`, against `seat.reserved.jobs`. That counter is produced
by `matcher::reserved()` through `out_there()` — the single filter that also
produces the label holdings and the memory and tmpfs reservations — and it
is incremented again by `claim()` inside the pass. It is the right place for
three reasons: it is **already the count**, so no second counter can drift
from the first; it is **derived from the records, never accumulated** (D79),
so there is nothing to release when a job ends any of the eight ways and
nothing to rebuild after a restart; and it is **taken inside the pass**, so
two jobs offered together contend for the last place instead of both finding
it free. Putting it before the memory arithmetic is also deliberate: the
sentence a waiting job reads should name what is actually holding it up.

**What a queued job is told when every eligible runner is at its cap**, from
the real output (`announced_job_ceiling` prints it):

```
queued 1st of 1 at priority step, rising to merger in 30m; it is waiting
because it is waiting for capped-runner: on capped-runner, 2 job(s) already
out there and it said it holds at most 2 at once, so it takes nothing more
until one of them ends: 2 jobs running there hold 0 MiB of memory and 0 MiB
of tmpfs; no other eligible runner is free either. It moves to another
eligible runner only when capped-runner stops being available, or after 3h
in the queue (--stickiness-limit); it has waited under a minute; nothing is
ahead of it
```

It is a `waiting` line — an existing `JobEventKind`. The `0 MiB of memory`
is the clause that makes the defect legible: those two jobs really did
declare nothing, which is precisely why the memory admission could never
have stopped the third.

**Whether a job can sit for ever behind a cap, and what stops it.** No, and
three separate things stop it. (1) Every job runs under a time cap
(invariant 2), so a capped machine empties whether or not its jobs behave,
and a job ending triggers a dispatch pass. (2) `ShortNow` is bounded by
`--stickiness-limit`: past it the job takes any other eligible free runner
— asserted. (3) A ceiling of **zero** would be a wait nothing on that
machine could end, so it is answered as `NotHere` instead of `ShortNow`:
nothing is placed there and nothing waits for it — asserted. Our own runner
never announces zero (`jobs_at_once` is floored at one). And a job's own
deadline still cancels it while it waits (E-09).

**What I had to find out for myself** — the four items above: the wrong
bench, the E-05 join, the six lost jobs, and `D-NEXT` versus `D-C` in code.

## Decisions needed

**None blocking.** Alexa's D-C is the decision this work implements, and it
is implemented as she worded it.

One thing for `herd-merger`, not a decision: this branch carries **two**
`D-NEXT` entries and they want **two** numbers, in this order — the runner's
own ceiling (`docs/decisions.md:8595`) first, the announced cap (`:8690`)
second. The second entry's text refers to the first as "the entry above".
There are 29 `D-NEXT` references in Rust comments to allocate as well; all
of them belong to the second entry.

## Resume point

`herd-merger` takes `runner-holds-its-own-limits`: allocate the two D
numbers and the 29 in-code references, rerun `check-docs.sh` (it should then
be fully green but for the D170/D174 gap notes) and
`check-orchestration.sh`, update `docs/status.md` and the ORCH-060 board
row, wait for CI, merge under ORCH-001.

## Processes

Started and ended by me, all under `nix develop --command` in this worktree:
`cargo build`, `cargo fmt`, `cargo clippy`, `cargo test` and
`./scripts/test.sh`. Every one ran in the foreground and exited on its own;
nothing was killed, by name or otherwise, and nothing was left running.
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/c5-runner-cap` was removed at the
end. No PID of another agent, and no container, service or consumer process,
was touched.
