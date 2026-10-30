# m12-p5-hub-keepers — handoff 2026-10-30

- **Item:** ORCH-253 (M12 Track P step P5, *the hub chooses keepers and
  says so*)
- **Agent:** herd-step
- **Branch / commit:** `m12-p5-hub-keepers` / `77f5740` for the code, plus
  this file's own commit, based on origin/main `83bc78a`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

The hub now has a keeper policy, fills `Welcome.keeper_for` from it, and
sends `FromHub::PinPacks` when the set a connected runner keeps moves.
**No `herd-proto` file is touched**: every frame was already on the wire
at `PROTOCOL_VERSION` 3, which is H-A of the planner's M12 plan holding.

1. **`herd_hub::Keepers`** (`herd-hub/src/server.rs:361` onward) — the
   policy as a value: an optional **default keeper**, and a map from key
   family to the runners named for it. `Keepers::parse` is what the
   command line produces, `Keepers::said` is the startup line,
   `Keepers::keys_for` is the one body that answers *which families does
   this runner keep*. `Keepers::nobody()` is the default everywhere.
2. **Two settings on `herd-hub`** (`herd-hub/src/main.rs`):
   `--keeper RUNNER` (`HERD_KEEPER`) names the default keeper, and
   `--keeper-for KEY=RUNNER` (`HERD_KEEPER_FOR`, repeatable,
   comma-separated in the environment) names a runner per key family.
   **Neither has a default**, so a hub given neither names no keeper,
   carries `keeper_for` on no welcome and sends no `PinPacks` at all —
   this build's behaviour before the settings existed. Both are parsed
   **before the state root is opened**, for the reason `--output-budget`
   is: opening the root sweeps, and a refusal must name what the operator
   typed.
3. **The welcome is filled** — `server.rs:3619` derives this connection's
   custody and `:3632` sends it, where `83bc78a` sent `Vec::new()` at
   `:3230`. A **requester** gets an empty list whatever it is called, so
   its welcome is byte for byte the one it got before.
4. **`PinPacks` on a change** — `tell_the_keepers` (`server.rs:3782`)
   recomputes every connected runner's set, sends one frame to each
   runner whose set **really moved**, and nothing at all to the others.
   An **empty** frame is how custody is retracted. It is reached from two
   places: `HubHandle::name_keepers` (`:1376`), and a pack registered
   under a key family this hub had never seen (the `RegisterPack` arm).
   **No sweep is triggered anywhere** — D262 part 1 stands unreversed.
5. **The hub remembers what it told each runner** —
   `ConnectedRunner::keeper_for`, set from the welcome at
   `register_runner` and replaced by every `PinPacks` sent since. That is
   what makes "the runners whose custody moved and nobody else" possible
   and what makes *and nothing else* assertable about a second runner.
6. **`herd_hub::KEEPER_KEEP_PER_KEY` is 5 and is asserted equal to
   `herd_runner::packstore::KEEP_PER_KEY`** in
   `herd-hub/tests/keeper_policy.rs`. The welcome carries keys and no
   figure, so a keeper applies its own constant; `PinPacks` carries one,
   and the two must not drift.
7. **Five assertions, in `herd-hub/tests/keeper_policy.rs`** — a real
   in-process hub, hand-written runner peers, everything read off a
   socket:
   - `the_count_the_hub_asks_a_keeper_for_is_the_count_the_runner_keeps`
   - `a_named_keeper_is_welcomed_with_its_keys_and_nobody_else_carries`
     `_the_field` — one name, split here only because this file is held
     to 79 columns
   - `a_hub_that_names_no_keeper_welcomes_every_runner_without_the_field`
   - `changing_the_setting_sends_pin_packs_and_nothing_else`
   - `a_default_keeper_keeps_the_families_this_hub_has_packs_under`

   The **absent** field is asserted on the frame's own text, because a
   decoded `FromHub` cannot tell an empty list from an absent one.
8. **One more in `herd-hub/src/main.rs`'s test module** —
   `the_keeper_policy_is_read_off_the_command_line_or_refused_by_name`,
   with the three `KEY=RUNNER` refusals and the no-flags case; and
   `the_long_help_states_every_figure_its_constants_decide` now also
   gates `--keeper`'s *newest five packs* against
   `KEEPER_KEEP_PER_KEY`.
9. **Nothing outside the brief's ownership.** Four files:
   `herd-hub/src/server.rs`, `herd-hub/src/main.rs`,
   `herd-hub/src/lib.rs` (two `pub use` lines) and the new
   `herd-hub/tests/keeper_policy.rs`. No `herd-proto`, no runner, no
   `docs/decisions.md`, no `docs/orchestration/board.md`.

**What this does NOT make true, said plainly.** Custody is visible **for
the key families a policy names, on a hub configured to name them**. On
the consumer's hub today, which passes neither setting, nothing is
pinned and nothing has changed. A pack outliving the machine that built
it is M12 P7 and needs M11's push path; no report may say custody works
end to end on the strength of this step.

## Not done, and why

1. **No live reload of the setting.** The only operator-facing way to
   move the policy is to restart the hub. The mechanism lives on
   `HubHandle::name_keepers`, which the registration trigger and the
   tests both go through; a signal, a file re-read or a `herdctl` command
   is not built, and the `herdctl` one could not be — it would need a
   frame on a protocol frozen at 3. Drafted as a row below.
2. **Custody appears in no report.** `herd-hub status`, `status.json` and
   `herd-hub report` say nothing about who keeps what: measured, zero
   occurrences of `keeper` in `herd-hub/src/report.rs`,
   `herd-hub/src/status.rs` and `herd-hub/src/registry.rs`, with the
   detector proven on the same files (`budget` 4 in `report.rs`, `pack`
   109). `status.rs` and `report.rs` are outside this task's ownership.
   Drafted as a row below.
3. **A default keeper on an empty registry keeps nothing.** *Every
   family nobody else keeps* has no finite spelling until the hub knows
   which families exist, so the default keeper's share is derived from
   the key families this hub has a pack registered under. The
   registration trigger is what stops that being permanent; it is the
   only live trigger there is. Asserted as a fact in the test, not
   hidden.
4. **No end-to-end with the real runner binary.** This step's test
   drives a real hub and hand-written runner peers, which is the
   opposite division from `pack_custody.rs` and is deliberate: what P5
   owes is the choice and the frame. The two files together still do not
   prove custody end to end — that is P7.
5. **No board or `decisions.md` edit, no merge, no tag, no pull
   request.** As briefed. Both drafts are below, in full.

## Gates

**Eight separate Herd jobs, one per gate, never one `gate.sh`** — that
run is `herd-merger`'s and is the only thing that writes a receipt. Every
submission went through `/mnt/ssd_980/herd/queue/bin/herdctl` and never
bare `herdctl`, with `--label build --memory-mib 6000 --verdict-to
<file>`, after a `./scripts/bench-preflight.sh` that answered **ok** each
time. `CARGO_TARGET_DIR` was
`/mnt/ssd_980/herd/build/m12-p5-hub-keepers` in all eight, so the cold
build's bytes were paid once and the rest ran warm. Each line is
`nix develop --command <command>`, one level, never nested.

The jobs build the **working tree**, which is this branch's one commit
plus this handoff while the last of them ran; nothing under `docs/`
enters a Rust compilation, so the Rust evidence is the code commit's.

| job | command | id | exit | wall | bytes_written |
|---|---|---|---|---|---|
| 1 | `cargo build --workspace --all-targets` | `j0000000080` | **101 — RED** | 55 s | 90 137 663 |
| 1b | the same, after two fixes | `j0000000082` | **101 — RED** | 39 s | 89 576 156 |
| 1c | the same, after the third fix | `j0000000083` | **0** | 89 s | 89 867 212 |
| 2 | `cargo fmt --all --check` | `j0000000084` | **1 — RED** | 2 s | 3 447 888 |
| 2b | `cargo fmt --all` (a write, not a check) | `j0000000085` | 0 | 2 s | 3 447 888 |
| 2c | `cargo fmt --all --check` again | `j0000000086` | **0** | 2 s | 3 447 888 |
| 3 | `cargo clippy --workspace --all-targets -- -D warnings` | `j0000000087` | **0** | 22 s | 89 440 988 |
| 4 | `./scripts/test.sh` | `j0000000088` | **0** | 299 s | 22 639 198 354 |

**Three gates were red and all three are reported red.** Not one was a
failing assertion, and nothing was worked around — no test was loosened
and no expectation was edited:

- **job 1, exit 101** — one `E0599` in the new test:
  `JobDirStore::packs()` answers `&PackRegistry` and the trait's
  `JobStore::packs()` answers the `Option`, and the test called
  `.expect()` on the inherent one. Plus one `unused_imports` warning,
  `KEEPER_KEEP_PER_KEY` imported in `main.rs` for a `cfg(test)` reader
  only — which `-D warnings` would have refused at clippy. Fixed by
  dropping the `.expect()` and by qualifying the constant at its one use.
- **job 1b, exit 101** — eight `E0063`, `missing field keepers`, every
  one of them an in-file test's `State` initializer: adding a field to a
  struct a test module builds by hand is eight edits and the compiler
  named all eight. Fixed by adding
  `keepers: Mutex::new(Keepers::nobody())` to each.
- **job 2, `cargo fmt --all --check`, exit 1** — rustfmt wanted a
  different shape for one `use` group and one `assert_eq!`. Fixed by
  `cargo fmt --all` (job 2b) and re-checked green (job 2c).

The suite, verbatim where it matters:

```
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
```

**1 218 passed, 0 failed**, summed over every test target, and the six
assertions this task adds all ran. From
`herd-hub/tests/keeper_policy.rs`:

```
running 5 tests
test the_count_the_hub_asks_a_keeper_for_is_the_count_the_runner_keeps ... ok
test a_named_keeper_is_welcomed_with_its_keys_and_nobody_else_carries_the_field ... ok
test a_hub_that_names_no_keeper_welcomes_every_runner_without_the_field ... ok
test changing_the_setting_sends_pin_packs_and_nothing_else ... ok
test a_default_keeper_keeps_the_families_this_hub_has_packs_under ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

and from `herd-hub`'s binary target:

```
test tests::the_keeper_policy_is_read_off_the_command_line_or_refused_by_name ... ok
```

**Skips, as a list and never a count (D187).** The run printed **12**
`HERD-SKIPPED` lines; their assertion names were extracted and `diff`ed
against `scripts/expected-skips/local.txt`'s twelve: **identical, no
line either way**. They are the standing cgroup-delegation twelve, every
one saying *"this runner cannot create a cgroup under
/sys/fs/cgroup/chatons.slice/…​ (Permission denied)"*, and under a Herd
job the reason is the queue runner's scope and not this shell's. The
`#[ignore]` census printed `COMPLETE (38 ignored, 0 without a reason)`,
and an **anchored** `grep -c '^[[:space:]]*#\[ignore'` over the four
crates answers **38** on this branch and **38** on `83bc78a` — the same
figure counted twice, from the run and from the source, which is the
comparison D187 asks for. An unanchored grep answers 51 and is the wrong
instrument.

**No skip belongs to this task.** Nothing added here is `#[ignore]`d and
nothing added here prints `HERD-SKIPPED`. A skip is copied as a skip and
is never counted as a pass (D27, invariant 5).

**On job 4's `bytes_written` of 22 639 198 354.** That figure is a walk
of the job's workspace summing **apparent** size, and the suite's own
sparse fixtures dominate it; it is not 22 GB of this task's writing. The
job was not capped — the disk budget is 64 GiB — and it carries a
verdict and a receipt, which a capped job does not.

**`./scripts/check-docs.sh` was run on this tree before this file was
written and printed `docs ok`.** The **last run** is
`./scripts/check-docs.sh`, from the worktree root, on the tree with this
file committed; its output goes to the agent's report and not into the
tree it has just gated.

## Risks and findings

1. **A premise of the brief was wrong in the opposite direction from the
   warning, and it is a measurement.** The brief said *the line number in
   P4's resume point is stale — it says `server.rs:3230`, which was true
   at `73c2dc8`*. Measured at `83bc78a`:
   `grep -n "keeper_for: Vec::new()" herd-hub/src/server.rs` answers
   **3230**, and nothing at 3161. So **P4's figure was the correct one**,
   and the three citations that are wrong are `board.md`'s ORCH-253 and
   ORCH-254 cells — which name `server.rs:3161` **twice each**, four
   citations against one of `:3230` on the whole board — and the M12
   plan's own Track P row. The agent found the line by what it is, as
   briefed, so the warning cost a verification and not a defect. Drafted
   as a row below.
2. **D10's *the server's runner is the default keeper* cannot be
   automatic, and this is the finding that shaped the whole step.** A hub
   knows a runner by the `name` it announced; `ToHub::Hello`'s `host` is
   a `HostSize` — memory, threads, memory floor, tmpfs — and carries no
   hostname, no address and no identity of any kind. There is therefore
   nothing in the handshake a hub could compare with its own machine, and
   `PROTOCOL_VERSION` is frozen at 3, so nothing can be added to carry
   one. The sentence becomes the **deployment's** line, `--keeper <that
   runner's name>`, and it is documented that way on the flag. **This is
   not a `herd-proto` edit and was not treated as one**: the policy needs
   nothing the frozen surface does not carry — it needs an operator to
   say which runner it is.
3. **Custody is configured and invisible.** The hub says who keeps what
   twice on `stdout` — once at startup, once per runner at its welcome —
   and nowhere a reader can ask. An operator cannot answer *which machine
   is keeping yesterday's pack* from any surface this build publishes.
   Measured with a proven detector (finding 2 of *Not done*). Drafted as
   a row.
4. **`KEEPER_KEEP_PER_KEY` is one figure in two crates.** Its honest home
   is `herd-proto`, beside the frame that carries it, and `herd-proto` is
   out of this task's ownership and is the file the brief said to stop
   at. The drift is closed by an assertion rather than by sharing:
   `herd-hub` has `herd-runner` as a **dev-dependency**, so
   `herd-hub/tests/keeper_policy.rs` can compare the two constants, and
   it does, with a message that names both. If `herd-proto` is ever
   opened for another reason, moving the constant there is a one-line
   improvement and needs no version move — a `const` is not wire.
5. **The default keeper's share is derived from the registry, which
   makes a welcome depend on registry state.** Two consequences, both
   named in the code: a keeper that dials before any pack exists is
   welcomed with nothing (and the registration trigger is what fixes
   it), and `State::known_key_families` clones every `Entry` the registry
   holds, once per dial of a runner, **only when a default keeper is
   named**. A hub with no `--keeper` never asks the registry anything on
   the welcome path. On a large registry that clone is the one cost this
   step adds and it is paid per connection, not per frame; if it ever
   matters, the registry wants a `keys()` accessor, which is
   `registry.rs` and outside this ownership.
6. **"And nothing else" is bounded by a round trip, not by a clock.**
   Each peer sends a `Health` after the policy change and reads to the
   hub's answer; the frames before that answer are what the change sent.
   A frame the hub sent *after* its own health answer would be missed.
   That is the same technique `pack_custody.rs` uses and the strongest
   assertion available without a hub-side frame log.
7. **Three red gates, all three mine, all three reported before being
   fixed.** Two cold-build submissions failed on my own compile errors
   (`j0000000080`, `j0000000082`) and one `fmt --check` failed
   (`j0000000084`). Nothing was worked around: no test was loosened and
   no expectation was edited. The figures are in *Gates*.

## Drafted for `herd-merger`

One `docs/decisions.md` entry and three `board.md` rows, verbatim and
fenced. Nothing here was written into the tree: `decisions.md` and
`board.md` are untouched on this branch, and a `D-NEXT` left in
`decisions.md` would redden `check-docs.sh`.

### The `docs/decisions.md` entry

Appended as written, with the number allocated by the merger in place
of `D-NEXT` and gaps kept (D148, invariant 8). Every unfenced line is
at or under 79 columns, the heading included.

```markdown
## 2026-10-30 — D-NEXT The hub names its keepers, and derives the default

M12 P5 filled the `keeper_for` that `herd-hub/src/server.rs:3230` sent
empty on every welcome, and had to answer three questions `docs/plan.md`
M12 and D10 do not: **how a hub knows which runner is the server's**,
**what a named key family does to the default keeper**, and **what
"changing the setting" is on a hub that is already serving**. All three
are decided here, all three are reversible, and the first is a
measurement and not a preference.

**1. The hub cannot find the server's runner, so the deployment names
it.** D10 and `plan.md` M12 both say *the server's runner is the default
keeper*. Measured against the frozen handshake: a hub knows a runner by
the `name` it announced, and `ToHub::Hello`'s `host` is a `HostSize` —
memory, threads, memory floor, tmpfs and nothing else. No hostname, no
address, no identity of any kind crosses, so there is nothing a hub
could compare with its own machine, and `PROTOCOL_VERSION` is frozen at
3. D10's sentence therefore becomes one line of the deployment's,
`--keeper <that runner's name>`, and the policy's job is to make it one
line. **There is no default**: a hub given neither setting names no
keeper, carries `keeper_for` on no welcome and sends no `PinPacks` at
all, which is this build's behaviour before the settings existed, byte
for byte. That is D194's rule — a name invented here would be a name
applied to somebody else's fleet.

**2. A named key family is MOVED off the default keeper, not added to
it.** `--keeper-for KEY=RUNNER` takes that family out of `--keeper`'s
share. The other reading — a named keeper beside the default one — was
rejected because it makes custody only ever grow: an operator could add
a holder and never move one, and the family whose packs are filling the
server's disk is exactly the one she would want to move. Naming the
default keeper again in a `--keeper-for` is how both hold it, which is
explicit and reversible. Two runners named for one family is D10's *one
or more keepers* and is not a mistake.

**3. The default keeper's share is derived from the registry, and a
registration is the one live trigger.** *Every family nobody else
keeps* has no finite spelling until the hub knows which families exist,
so it is filled from the key families this hub has a pack registered
under; a family named explicitly is pinned whether or not anything is
filed under it yet, because that is what the operator asked for and a
keeper that waited for the first pack would be a keeper that missed it.
The consequence is that a default keeper on a hub with an empty
registry keeps **nothing** until a pack arrives — and on the one
deployment this was written for the hub starts before any pack exists.
So a pack registered under a family this hub had never seen recomputes
custody and tells the runners whose set moved. Without that, the
default keeper would be inert in production.

**4. Changing the setting sends `PinPacks` to the runners whose set
moved and nothing to anybody else.** The frame replaces rather than
adds, so the whole new set always goes out and an **empty** frame is
how custody is retracted — a hub that merely stopped sending could
never take a pin back. A runner whose set is unchanged is sent nothing:
the runner prints a line per frame it reads, and a hub that re-sent to
everybody on every change would be writing on an owner's stderr about
an event that did not concern her machine. **No sweep is triggered**,
which is D262 part 1 unreversed: the runner's pass runs once per
connection, before its announcement, so a frame arriving mid-link
changes what the next pass may take and moves no pack today.

**5. The operator-facing way to change the setting is a restart.** The
mechanism lives on `HubHandle::name_keepers`, which is what the
registration trigger and the test both go through; a reload on a
running hub — a signal, a file re-read, a `herdctl` command — is not
built, and the `herdctl` one could not be, because it would need a
frame on a frozen protocol. Named as a board row, not as a defect of
this entry.

**6. `herd_hub::KEEPER_KEEP_PER_KEY` is 5 and is asserted equal to
`herd_runner::packstore::KEEP_PER_KEY`.** The welcome carries keys and
no figure, so a keeper applies the constant it shipped with; `PinPacks`
carries one. A hub sending a different number would move a keeper's
retention the moment an unrelated `--keeper-for` was added. The two
constants are in two crates because `herd-proto` holds the frame and
not the policy, and `herd-hub` sees `herd-runner` only in its tests —
so the equality is asserted in `herd-hub/tests/keeper_policy.rs`, which
is the one place both are visible. `N = 5` is `plan.md` M12's figure,
has no derivation anywhere, and is never an acceptance criterion.

**What this does not make true.** Custody is visible for the key
families a policy names, on a hub configured to name them. A pack
outliving the machine that built it is M12 P7 and needs M11's push
path; no report may say custody works end to end on the strength of
this.
```

### The `board.md` rows

Three rows for the *Owed* table, whose header is `ID | Item | Origin |
Owner | State | Next action | Depends on | Evidence`: eight content
cells, ten `awk -F'|'` fields, no literal pipe in any cell, State
exactly one of the seven words. Ids are `ORCH-NEXT-*` because several
branches are unlanded ahead of this one and the merger allocates.

```
| ORCH-NEXT-A | **A hub's keeper policy can only be changed by restarting it, and a default keeper on an empty registry keeps nothing.** M12 P5 landed the hub's half of custody — `--keeper RUNNER` and `--keeper-for KEY=RUNNER` on `herd-hub`, `Welcome.keeper_for` filled from them, and `PinPacks` sent to the runners whose set moves — and two limits of its own come with it. First, the only operator-facing way to move the policy is a restart: the live seam is `HubHandle::name_keepers`, and a reload on a running hub is unbuilt, where a `herdctl` one could not be built at all because it would need a frame on a protocol frozen at 3. Second, a **default** keeper's share is derived from the key families this hub has a pack registered under, so on a fresh root it is empty until a pack arrives; the registration trigger P5 added is what stops that being permanent, and it is the only live trigger there is | `history/2026-10-30-m12-p5-hub-keepers.md` *Not done* and *Risks*, 2026-10-30; measured against the frozen handshake at `83bc78a` | herd-step | owed | Decide whether the hub reloads this setting while serving, and if so by which mechanism — a `SIGHUP` re-read of the command line is the cheapest and is the only one that needs no frame. Nothing is owed before M12 closes: a restart re-derives every welcome from scratch, and the consumer restarts the hub for every deployment anyway. Worth a row because the *next* reader of `name_keepers` will assume an operator can reach it | M12 P5, landed | `herd-hub/src/server.rs` `Keepers`, `tell_the_keepers`, `HubHandle::name_keepers`, `KEEPER_KEEP_PER_KEY`; `herd-hub/src/main.rs` `--keeper` and `--keeper-for`; `herd-hub/tests/keeper_policy.rs`; **D262** part 1; ORCH-253 |
| ORCH-NEXT-B | **Nothing Herd prints says who keeps what, so custody is configured and invisible.** Measured at `83bc78a` with the detector proven on the same files — `budget` matches 4 times in `herd-hub/src/report.rs` and `pack` 109 times — zero occurrences of `keeper` in `herd-hub/src/report.rs`, `herd-hub/src/status.rs` or `herd-hub/src/registry.rs`. After M12 P5 a hub knows which runner keeps which key family, says it once on `stdout` at startup and once per runner at its welcome, and then says it nowhere a reader can ask: `herd-hub status` and `status.json` carry no custody field, and `herd-hub report` names no keeper. An operator cannot answer *which machine is keeping yesterday's pack* from any surface this build publishes, which is `disk-policy.md` §0.7's requirement one field over | `history/2026-10-30-m12-p5-hub-keepers.md` *Risks*, 2026-10-30, measured with a proven detector | herd-step | owed | One field on the status document's per-runner view, beside the three pack figures ORCH-251 brings there: the key families this hub told that runner it keeps. It rides with ORCH-251's task rather than taking one of its own, because both move `herd-hub/src/status.rs` and `STATUS_VERSION` should move once | ORCH-251 | `herd-hub/src/status.rs` `RunnerNow`; `herd-hub/src/report.rs`; `herd-hub/src/server.rs` `ConnectedRunner::keeper_for`; `docs/disk-policy.md` §0.7; ORCH-251, ORCH-252 |
| ORCH-NEXT-C | **Two live board cells and the M12 plan cite `server.rs:3161` for a line that was at `:3230`, and a brief built on them corrected the right figure into the wrong one.** Measured at `83bc78a`: `grep -n "keeper_for: Vec::new()" herd-hub/src/server.rs` answers **3230** and nothing at 3161. ORCH-253 and ORCH-254 name `herd-hub/src/server.rs:3161` twice each, four citations of it against one of `:3230` on the whole board, as does `herd-planner`'s M12 plan §3 Track P; P4's handoff, batch 23's merge handoff and `board.md`'s own M12 paragraph all say `:3230` and are right. P5's brief then warned that P4's `:3230` was stale and had moved, which reversed the one correct citation of the three — the agent found the line by what it is, as briefed, and the warning cost a verification rather than a defect | `history/2026-10-30-m12-p5-hub-keepers.md` *Risks*, 2026-10-30; measured with `grep -n` at `83bc78a` | herd-merger | owed | Restate the four citations to `server.rs:3230` when the board is next restated, or drop the line number: the construct is gone as of M12 P5, so the citation's remaining value is historical and a sha would carry it better than a line. Same defect and same direction as ORCH-241 and ORCH-242, which is why it is one row and not three | — | `docs/orchestration/board.md` ORCH-253 and ORCH-254 Evidence cells; `herd-hub/src/server.rs` at `83bc78a` line 3230; `history/2026-10-30-m12-p4-runner-custody.md`; `history/2026-10-30-merge-batch-23.md`; ORCH-241, ORCH-242 |
```

## Decisions needed

**One, and it is not Alexa's.** Does the hub reload its keeper policy
while it is serving, and if so by which mechanism? *Recommendation:* no,
not before M12 closes — a restart re-derives every welcome from scratch,
the consumer restarts the hub for every deployment, and the registration
trigger already covers the one case that cannot wait for a restart. If it
is wanted, a `SIGHUP` re-read of the command line is the cheapest and the
only one that needs no frame; a `herdctl` command is impossible on a
protocol frozen at 3. *Fallback:* leave it, and the drafted row keeps the
question from being lost. Everything else here is *how* and was decided
in the branch.

## Resume point

**M12 Track P step P6 — the no-holder state** (`plan.md` M12 clause 2):
a pack all of whose holders are gone becomes a visible, named state in
`herd-hub/src/registry.rs`, in `status.json` and in the **wait sentence**
of a job that needs it, never a `Refused` (D13). It waits on nothing and
can start the moment this branch is pushed; `PackRegistry::holders_of`,
`resolve`, `announced` and `runner_gone` all ship.

Then **P7**, the keeper end-to-end, which waits on **ORCH-220** (M11's
push path) and M11's `{in}`/`{out}` layout step and is the clause that
discharges invariant 5 for M12 (ORCH-257).

Exact next command for whoever continues this file:

```sh
git worktree add -b m12-p6-no-holder <path> origin/main
```

and read `herd-hub/src/registry.rs` `holders_of` and `Entry` first.

## Processes

- No PID was started by this task and none was signalled. Every heavy
  phase ran as a Herd job on `queue-local` through
  `/mnt/ssd_980/herd/queue/bin/herdctl`; the runner that executed them is
  PID 662711, which belongs to the queue and was not touched.
- `./scripts/bench-preflight.sh` ran before every submission and was
  **ok** every time (lowest margin: load 7.86 of a 9.00 ceiling, 335 GiB
  free; it refused nothing). `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was
  never set or exported.
- Build tree: `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/m12-p5-hub-keepers`,
  **debug with test targets**, measured with `du -sh` — the figure and
  its deletion are in the agent's report, because the tree is removed
  **after** the branch is pushed and this file is committed before that.
