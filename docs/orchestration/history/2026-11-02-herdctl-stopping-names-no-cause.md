# herdctl-stopping-names-no-cause — handoff 2026-11-02

- **Item:** ORCH-NEXT (drafted below; `herd-merger` allocates the number)
- **Agent:** herd-step
- **Branch / commit:** `herdctl-stopping-names-no-cause` / the tip of this
  branch, based on `origin/main` `c824fc8`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**`herdctl` no longer names a cause for a runner's stop.**
`herdctl/src/lib.rs`, in `switch_lines`, appended the literal
`"; it is also stopping (SIGTERM)"` whenever `RunnerSwitch.stopping` was
true. It now appends `"; it is also stopping"` and nothing else.

The parenthetical was a literal, never a reading of the frame. The field
it was gated on carries no cause: `herd_proto::RunnerSwitch.stopping` is
a `bool`, the hub fills it from the runner's own `draining` flag
(`herd-hub/src/server.rs:2750`, `stopping: runner.draining`), and that
flag is set by `Session::start_draining` (`herd-runner/src/lib.rs:3430`),
which **every** stop path calls — including the one that is not a signal:
a `drain` position whose runner now holds nothing leaves by itself
(`herd-runner/src/lib.rs:4203`).

**The cause is not inferred from `runner.switch` either**, and the code
says why in a comment at the site: a runner that is draining *and* was
signalled is one machine with two causes, so a label taken from a
correlate is the same defect with a longer reach.
`Switch::leaves_when_empty()` would usually be right, and usually is not
a warrant.

**`herd_proto::RunnerSwitch.stopping`'s doc comment** said *"Whether it
is also stopping for good (`SIGTERM`, invariant 2)"* — narrow in exactly
the way the rendering was, and the place the next author would learn the
wrong thing. It now states that every stop path sets the flag, that a
signal and an emptied drain are indistinguishable in it, that no cause
may be read from it or from the switch beside it, and that the runner's
own reason exists but stops at the link. **Doc comment only: no field
changed, nothing on the wire moved, `PROTOCOL_VERSION` stays 3** — the
diff touches no `#[serde]` attribute, no field name and no type.

**A unit test pins the new contract**,
`a_stopping_runner_names_no_cause_for_its_stop` in `herdctl/src/lib.rs`'s
`mod tests`. It renders a stopping runner twice, once under
`Switch::Drain` and once under `Switch::On`, and asserts:

1. each line ends with `"; it is also stopping"`;
2. **the string `SIGTERM` is absent**, and so is any case of `signal` —
   the absence, not merely the presence of the new wording, because the
   regression being prevented is a cause creeping back in and an
   assertion on presence alone passes a line that says both;
3. the two lines are **identical past the position**, since `herdctl`
   cannot tell an emptied drain from a signalled stop and must not
   pretend it can;
4. a runner that is not stopping gets no such clause at all, so the three
   assertions above are about this branch and not about a line that never
   carried it.

**Every assertion on the old spelling was searched for and there was
exactly one site.** `grep -rn "also stopping" --include=*.rs .` returned
two lines before this change — the rendering and the proto doc comment —
and no test. The same search over `--include=*.md` returned nothing.

## Not done, and why

- **The brief's third place is not where the brief says.** It states that
  `herd-hub/tests/owner_control.rs` asserts `switch_lines` output around
  `:749`-`:764`. It does not: those lines assert
  `"wsl-pc: off (she is gaming), nothing running"` through
  `until_the_fleet_says`, a switched-off runner that is not stopping, and
  the string `also stopping` appears nowhere in any test in the tree. So
  **no existing assertion was updated, because none existed** — the
  rendering was untested, which is how the literal survived. Nothing was
  edited in `owner_control.rs`.
- **The stop's reason is not carried.** Priced below, changed nowhere, as
  the brief directs.
- **No board or `decisions.md` write.** The drafted row is below for
  `herd-merger`.

## Gates

**All four Rust gates plus both docs checks, run whole and bare through
the queue** on the code commit, never per step: `./scripts/gate.sh` runs
step 1 `check-docs.sh`, step 2 `check-orchestration.sh`, step 3
`nix flake check`, step 4 `cargo fmt --all --check`, step 5
`cargo clippy --workspace --all-targets -- -D warnings`, step 6 the
suite, step 7 the skips by list and both kinds, step 8 the verdict. Its
verdict and receipt are in this agent's report, verbatim, with their
figures.

It was submitted to our own queue hub with
`/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791 run
--label build --memory-mib 4700`, after `./scripts/bench-preflight.sh`
whose verdict was read **as text** and said `ok, safe to start`. Bare
`herdctl` was never used: on this host its compiled-in default is the
consumer's production hub.

**The last run, named and not quoted** — it is made after this file is
committed, so its output cannot be in the tree it gates:

```text
./scripts/check-docs.sh
```

from the worktree root; its output goes to the terminal and is reproduced
verbatim in this agent's report.

**Skips:** named in this agent's report as a list and never as a count,
out of the gate's own step 7. Nothing in this diff adds a
`HERD-SKIPPED` line or an `#[ignore]`. No `HERD-SKIPPED` line is copied
as a pass.

## Risks and findings

- **The brief's premise about the test site is refuted** (see *Not done*).
  The rendering had no test at all. This is the whole of why a literal
  parenthetical lived in a line an operator reads first.
- **The window in which the misleading line is visible is short, and
  that matters for the pricing below.** `start_draining` queues a health
  report at once, and the runner then flushes and disconnects inside
  `drain_grace` (invariant 2's five seconds). For the emptied-drain path
  the runner leaves within seconds of saying it is stopping, so the line
  the consumer read came from a runner their unit kept restarting onto a
  persisted `drain` position — each restart draining again, each drain
  showing the clause for a few seconds. **A reason must therefore ride
  the frame; nothing can be fetched afterwards, because by then there is
  no runner to ask.**
- **A third surface asks the same question and was not touched.**
  `herd-hub/src/server.rs:1427`'s `RunnerStatus.draining` feeds the hub's
  status output, and `:2359`, `:2395` and `:2541` use `runner.draining`
  in the sentences a waiting job gets. None of them names a cause today,
  so none of them is wrong; all of them would want the reason the moment
  it exists. That is scope for the decision below, not for this branch.
- **`switch_lines` is called from exactly one place**,
  `herdctl/src/main.rs:1231`, so the rendering has one reader and no
  other caller was affected.

## Drafted for `herd-merger`

### `docs/decisions.md` — one new entry

```markdown
## 2026-11-02 — D-NEXT A flag that every path sets names no cause

`herdctl` printed `; it is also stopping (SIGTERM)` for any runner the
hub reported as stopping. `herd_proto::RunnerSwitch.stopping` is a
`bool`; the hub fills it from the runner's `draining` flag
(`herd-hub/src/server.rs:2750`) and `Session::start_draining`
(`herd-runner/src/lib.rs:3430`) sets that flag on **every** stop path,
the signal and the `drain` position that has emptied and leaves by itself
(`herd-runner/src/lib.rs:4203`) alike. The parenthetical was a literal.

On 2026-11-02 at 01:11 CET the first consumer drained their server runner
to recreate their hub, their idle check printed
`server: … nothing running; it is also stopping (SIGTERM)`, their unit
restarted the runner, and they opened an investigation into a signal
nobody had sent.

**The line now says `; it is also stopping` and names no cause.** The
cause is **not** inferred from `RunnerSwitch.switch` beside it, although
`Switch::leaves_when_empty()` would usually be right: a runner that is
draining *and* was signalled is one machine with two causes, and a label
taken from a correlate is this same defect with a longer reach. An
inferred cause is not better than no cause.

The field's own doc comment said `SIGTERM` too, which is where the
rendering's reading was learnt; it now states that the flag carries none.
**Doc comment only**: no field, no wire change, `PROTOCOL_VERSION` stays
3. A unit test on `switch_lines` asserts the **absence** of `SIGTERM` and
that the line reads alike under `drain` and under `on`, because the
regression to prevent is a cause creeping back in.

Whether the stop's *reason* should travel to the operator is a separate
question, priced and not decided here (see the board row of the same
date): the runner holds the sentence and throws it away at the link, and
carrying it is two additive fields across two hops, which invariant 4
makes a decision and not a cleanup.
```

### `docs/orchestration/board.md` — one new row, in *Open*

| ORCH-NEXT | **Should the stop's reason travel to the operator? Priced, not decided; nothing was changed.** The runner holds a sentence for every stop and throws it away at the link: `herd-runner/src/lib.rs:4305` prints `herd-runner: draining (<reason>); N job(s) to end, no new job accepted` on its own stderr, and `start_draining` takes the reason only to cancel jobs with it. **Three reasons exist in production, and all three are the operator's own words**: `"the runner's owner asked it to stop (SIGTERM)"` (45 chars), the same with `(SIGINT)` (44), and `format!("the owner's switch is {} and this runner holds nothing, so it leaves", switch.sentence())` — 92 chars with `draining (a reboot is due)` (`herd-runner/src/lib.rs:4203`, `:4401`-`:4402`). **Hop 0, unnamed in the brief and real:** the runner does not keep the reason. `Inner.draining` is a `bool` (`herd-runner/src/lib.rs:1204`), so either a second field `drain_reason: Option<String>` is added beside it — 3 edits in one file, no reader changes, and the two can diverge — or `draining` becomes `Option<String>`, which cannot diverge and touches 6 sites in that file plus `Session::draining()`'s return type (`:1379`, `:2151`, `:2170`, `:2313`, `:3430`-`:3432`, `:3908`). **Hop 1, the health frame, is the expensive one: 18 Rust construction sites across 13 files, plus one exhaustive pattern.** `ToHub::Health` (`herd-proto/src/lib.rs:1694`) gains `#[serde(default)] drain_reason: Option<String>`; a struct-variant construction cannot elide a field, so all 18 must gain it — 1 in production (`herd-runner/src/lib.rs:3906`), 11 in `herd-hub/tests/` and 6 in `herd-proto/tests/round_trip.rs`. Four more sites destructure with `..` and are free; `herd-hub/src/server.rs:5828` deliberately binds every field by name and is not. Then the hub's own `Runner.draining: bool` (`:1764`), `record_health`'s signature and its 5 read sites (`:1335`, `:2395`, `:2541`, `:2750`, `:2904`). **Hop 2 is cheap: 3 construction sites today** — `RunnerSwitch` gains `#[serde(default)] stopping_reason: Option<String>` at `herd-hub/src/server.rs:2746` and `herd-proto/tests/round_trip.rs:1033`, `:1043`, plus the two in this branch's new unit test. **A cheaper variant of hop 2 exists and has a caveat**: `RunnerSwitch.facts` is already `#[serde(default)]` and `RunnerFacts` derives `Default` with all-`Option` fields, so a reason put there costs 1 production site (`:1823`) and reaches `herdctl runner list` as well (E-51, D166) — but `RunnerFacts` means "this hub did not say", which would then sit on the same field as "not stopping". **No `PROTOCOL_VERSION` move either way, on D124/D128's own ground and with two precedents** — `FromHub::AssignJob.need` and `ToHub::AttemptAbandoned.still_running_attempt` (ORCH-350, D294) both landed additive and defaulted, each spelled `#[serde(default, skip_serializing_if = "Option::is_none")]` so an absent field is also an absent key on the wire (`herd-proto/src/lib.rs:2544`, `:1889`). **Two further costs, named because they are not field counts.** (1) **The display is its own decision, not an append**: the longest reason is 92 chars and the current line is 72, so `; it is also stopping (<92 chars>)` is a 167-column line, measured, which is exactly what D49 and D166 already refused for a suspension's reason — the shape that fits is a second indented line beside `suspension_lines`. (2) **A third surface asks the same question**: `RunnerStatus.draining` (`herd-hub/src/server.rs:1427`) and the waiting-job sentences at `:2359`, `:2395`, `:2541` would all want the reason once it exists, so the field count above is the floor | the 2026-11-02 `herdctl` defect, found by the orchestrator in the tree the same night the first consumer hit it; priced by `herd-step` on `herdctl-stopping-names-no-cause` | herd-planner | open | Decide whether the reason travels. **The recommendation is yes, as one unit of three hops, and not as a cheap first half** — see *Evidence* for why either half alone buys nothing | none; the rendering fix is already landed and is complete without this | `herd-runner/src/lib.rs:4203`, `:4305`, `:4401`-`:4402`, `:1204`, `:3430`-`:3432`, `:3906`; `herd-proto/src/lib.rs:1694`, `:569`-`:575`; `herd-hub/src/server.rs:1764`, `:2746`, `:2750`, `:2904`, `:5828`; `herd-proto/tests/round_trip.rs:1033`, `:1043`; D124, D128, D294, D49, D166, invariant 4. **The pricing's own asymmetry is the argument**: hop 2 alone is 3 sites and buys nothing, because the hub has no reason to forward until hop 1 exists; hop 1 alone is 18 sites and buys the hub a fact it does not show. So the useful unit is both hops and hop 0 together — about 25 construction sites, one display decision and a test per hop — for a sentence the runner already writes and already prints to the one person who was already standing in front of it. **What the landed fix buys without any of it**: the operator is no longer told something false, which was the whole of the 01:11 cost; what stays unbought is being told something true |

## Decisions needed

**Should the stop's reason travel to the operator?** Priced in the board
row above and changed nowhere.

- **Recommendation: yes, eventually, and as one unit of three hops, not
  as a cheap first half.** The runner already composes the exact sentence
  an operator needs and already prints it to that operator's own stderr;
  the only thing missing is the link. But the useful unit is hop 0 plus
  hop 1 plus hop 2 — about 25 construction sites, 13 files, one display
  decision (a second indented line, because the longest reason makes a
  167-column one) and a test per hop. Either half alone buys nothing:
  hop 2 forwards a reason the hub does not have, hop 1 gives the hub a
  fact nobody shows.
- **Fallback: leave it.** The landed fix already removes the whole of the
  measured cost — an operator told something false. What stays unbought
  is an operator told something true, and today they can read it on the
  runner's own stderr, which is where the consumer's unit journal already
  has it.
- **Not a compatibility question** (`CLAUDE.md`, *No compatibility to
  preserve*): both fields default, so by D124 and D128 nothing moves
  `PROTOCOL_VERSION` and no upgrade order is forced — a hub that receives
  no reason shows none, which is today's behaviour exactly.

## Resume point

`herd-merger` picks this branch up in the batch's one landing: allocate
the `D-NEXT` above a number, paste the board row into *Open* with an
`ORCH-nnn`, and run `./scripts/gate.sh` on the gated sha.

Nothing else is owed on this branch.

## Processes

PIDs started and ended: one `herdctl` client process per queued gate run,
started by this agent inside its own background shell and ended when the
job's verdict arrived; no PID was killed, and no process was ever
signalled by name. The queue hub and its `queue-local` runner were
already up and were not touched.

`CARGO_TARGET_DIR` was
`/mnt/ssd_980/herd/build/herdctl-stopping-names-no-cause`, off the root
filesystem, and is deleted once this branch is pushed — the path is named
here so that whoever reads this can check it is gone. The worktree is
this agent's own scratchpad, under
`…/scratchpad/wt/`, in a directory named for this task.
