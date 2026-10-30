# runner-silence — handoff 2026-10-30

- **Item:** the startup-sequence audit of 2026-10-30, finding **F6** (its
  table row **R3**), the sixth of its seven `NONE` rows and the only one
  asserted by nothing at all; board rows below
- **Agent:** herd-step
- **Branch / commit:** `runner-silence` / `ed970ea` for the test, plus this
  handoff as a second commit, based on `origin/foreign-and-quiet` `b16f732`,
  itself a descendant of `audit-tests` `81132eb` → `store-guards` →
  `pack-registry`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

One new file, `herd-hub/tests/silent_hub.rs`. **No production line changed**
— the whole diff is that file and this handoff. Nothing under
`herd-runner/src`, `herd-hub/src`, `herd-proto`, `herdctl`, `scripts`,
`.github`, `docs/decisions.md`, `docs/orchestration/board.md`,
`docs/status.md`, and none of the five tests written since 02:00.

### The gap, measured before and after

The audit's own search, run on the base `b16f732` over every test
directory in the workspace:

```
$ git grep -n "so this link is dead" b16f732 -- herd-proto/tests \
      herd-hub/tests herd-runner/tests herdctl/tests
exit=1
```

Zero, as F6 says. **And the detector is proved rather than trusted for its
silence** — the same needle over the whole tree at the same commit finds
the one production site and the three places that quote it, so the zero
above is a real zero and not a search that cannot match:

```
b16f732:docs/decisions.md:7994
b16f732:docs/orchestration/history/2026-10-30-foreign-and-quiet.md:479, :509
b16f732:docs/remediation/00-baseline.md:424
b16f732:herd-runner/src/lib.rs:3848
```

The same search on the tree as it now stands returns the assertion, not
only prose:

```
herd-hub/tests/silent_hub.rs:7   (the file's own explanation)
herd-hub/tests/silent_hub.rs:383 (the expected text of the assertion)
```

### What the file asserts

Three tests.

1. `a_runner_whose_hub_says_nothing_concludes_the_link_is_dead_and_dials_again`
   — the conclusion **and** the act. The runner's own sentence, *"the hub
   has said nothing for 3s, so this link is dead even though its socket is
   open"*, with the duration read back out of it (below); that it waited
   the whole of the limit before saying so; that its health reports were
   arriving at the hub the whole time nothing came back, which is what
   makes the fixture a *silent hub* and not a broken socket; that it says
   it is dialling the same hub again; and that **a second connection
   really reaches the listener**. The last one is the half a log line
   cannot stand in for: a conclusion printed and not acted on leaves the
   fleet exactly as quiet.
2. `a_hub_that_answers_keeps_the_same_runner_on_the_same_link` — the
   control, and the red run made permanent. One difference from the
   fixture: the health reports are answered. Then, after **twice** the
   limit, no such sentence anywhere in the runner's output, exactly one
   dial, and the reports really answered. Without it the first test could
   be about a handshake this harness gets wrong rather than about silence.
3. `the_runner_publishes_the_deployed_silence_limit` — the deployed 60 s,
   read out of the binary.

### How a hub that says nothing was built, and which side is real

**The runner is the real `herd-runner` binary, as a process**, started with
`--hub`, `--name`, `--data-root`, `--health-every` and
`--hub-silence-limit`, its two streams on disk so they can be read while it
runs, `kill_on_drop(true)`, and built by `real_herd::binaries()` inside the
test's own run so no stale binary can stand in for it. **The silent party
is a hand-written listener in the test process**: it binds
`127.0.0.1:0`, completes the WebSocket handshake with
`tokio_tungstenite::accept_async`, answers `Hello` with `Welcome`, counts
every frame that arrives — and answers none of them. It is the same shape
as `unknown_frames.rs`'s `HubFromTheFuture`, which is where the pattern
came from; the difference is that the runner there is in-process and here
it is a process.

**Why that way round, said plainly because the audit's sharpest finding
was three tests believed to use real binaries on both sides when one
did.** The thing under test is the runner's code, so the runner has to be
real. The real hub cannot be the silent party: answering is exactly what
it does, and making it stop would mean a production code path for a hub
that goes mute — a change to the product in order to test the product.
Saying nothing is the one thing a hand-built peer does better than the
real binary, so that is the half it plays. A reader who needs the real hub
on the other side needs a different test; this one does not claim it.

**And why the file is in `herd-hub/tests/`**: it binds a listening socket,
and `no_module_of_the_runner_can_bind_a_socket`
(`herd-runner/tests/confinement.rs`) forbids the string `TcpListener`
anywhere under the `herd-runner` crate, its tests included — invariant 3,
and it is right. A stand-in for a hub belongs beside the hub's tests,
which already bind loopback ports. The runner under test is the real one
either way.

### The threshold, read back and not retyped

Two read-backs, because the runner publishes the figure on two different
surfaces and neither is a status document.

- **The figure it applied.** `next_message` prints `{silence_limit:?}` of
  whatever it really used, so the expected text is built in the test from
  `Duration::try_from_secs_f64(SILENCE_LIMIT_SECONDS)` — the same
  arithmetic `herd-runner/src/main.rs`'s `seconds()` does on the same
  command-line figure — and nowhere else. A runner that applied the
  deployed 60 s, or any figure but the one its command line gave it, does
  not print the line the test waits for. The test also asserts it waited
  **at least** the limit, measured from the instant the hand-written hub
  welcomed it; the reading is polled, so it can only be later than the
  truth, which is what makes a floor the right thing to assert.
- **The figure it ships.** `--hub-silence-limit` is declared with
  `default_value_t = HUB_SILENCE_LIMIT.as_secs_f64()`, so `herd-runner
  --help` is the one surface the deployed limit is published on. The test
  runs the real binary's `--help`, finds the `--hub-silence-limit`
  paragraph, and asserts it carries `[default: 60]` built from
  `herd_runner::HUB_SILENCE_LIMIT.as_secs_f64()`. The two cannot drift
  apart without that failing.

**What could not be read back, stated rather than glossed.** The runner
publishes **no status document**, so there is no equivalent of the hub's
`lost_after_ms` — the figure another agent read out of the hub's status and
asserted equalled what the command line asked. Its startup lines print the
memory it lends, its floor, its tmpfs, how many jobs it holds and the hub
it is joining; the timing pair is not among them. The applied figure is
therefore readable only in the sentence the runner prints **when the limit
fires**, which is what the first read-back uses.

### The shortened pair is the deployed pair ÷ 20

The audit names *cadence × limit* as F6's scale-sensitive quantity, so the
ratio is what has to survive a shortening. 60 / 15 is 4; the test's 3 /
0.75 is 4. The runner under test therefore lets **exactly four of its own
unanswered reports** pass before concluding, as it does in the field, on a
clock twenty times faster, and with the same margin ratio against
`herd_proto::silence_threshold_refusal` (3 s against the 2.25 s it needs,
as 60 s against 45 s). Both ratios are computed in
`the_runner_publishes_the_deployed_silence_limit` from
`HUB_SILENCE_LIMIT` and `HEALTH_EVERY` and compared, so "÷ 20" is an
assertion and not a sentence in a comment. That test also asserts the
binary really accepts the pair, because a fixture only a test could run
proves nothing.

### The decision about the 60 s cost

**A shortened threshold in the gate, no `#[ignore]`d twin, and the honest
figure here.** `announced_job_ceiling.rs`'s precedent — a 60 s twin
`#[ignore]`d at a measured 171 s — was read and not followed, for two
reasons. First, nothing about the code path differs between 3 s and 60 s:
`config.hub_silence_limit` is handed to `next_message` and the arithmetic
is the same branch, so the full-length twin would exercise no line the
shortened one misses, while the deployed figure is asserted separately out
of `--help`. Second, **this base's `gate.sh` does not report an `#[ignore]`
as a compile-time skip by name** (see *Risks and findings* 2), so a twin
would be a test nobody runs whose absence nothing announces — worse than
no twin. No `#[ignore]` was added, so the gate's refusal of a reasonless
one is not in play.

**What it costs in the gate, measured.** The three tests run in parallel
and **the whole file is 6.18 s inside the suite** — read out of a green
`./scripts/gate.sh`'s own step 6 output for commit `ed970ea`'s tree,
`test result: ok. 3 passed; 0 failed; 0 ignored; … finished in 6.18s`.
That figure is the control's wait of twice the limit and essentially
nothing else, which is the shape intended: the conclusion costs 3 s and
the control, which has to outlast it to mean anything, costs 6 s.
Standalone — `cargo test -p herd-hub --test silent_hub` on its own — the
same file reads 15.96 s to 19.01 s, and that difference is **not** this
file's: `real_herd::binaries()` runs a nested
`cargo build --offline` whose feature resolution differs from the outer
test build's, so it recompiles `herd-runner`, `herdctl`, `ring` and
`rustls` every time it is invoked that way (9.15 s observed in one run's
own output). `real_herd`'s own doc records the same thing — 9.7 s after
`cargo build --workspace --all-targets`, 0.06 s after the command
`scripts/test.sh` runs — so inside the gate it is near-free. Shared with
`killed_hub.rs` and `priorities.rs`; pre-existing, and not this task's to
change.

### Six runs of the new test, and the red demonstration

**Six consecutive runs of the file as committed**, each one preceded by a
preflight joined with `&&`. One green run of a timing conclusion is not a
result, and a fixture on this project was green three runs running while
measuring the wrong state, so all three tests were run six times and every
run is here:

```
===== run 1 =====  test result: ok. 3 passed; 0 failed; 0 ignored; finished in 17.32s
===== run 2 =====  test result: ok. 3 passed; 0 failed; 0 ignored; finished in 18.54s
===== run 3 =====  test result: ok. 3 passed; 0 failed; 0 ignored; finished in 16.43s
===== run 4 =====  test result: ok. 3 passed; 0 failed; 0 ignored; finished in 16.80s
===== run 5 =====  test result: ok. 3 passed; 0 failed; 0 ignored; finished in 15.96s
===== run 6 =====  test result: ok. 3 passed; 0 failed; 0 ignored; finished in 19.01s
```

Six of six, every test named `ok` in every run, spread 15.96 s to 19.01 s
— and that spread is the nested `cargo build --offline` above, not the
conclusion: the silence itself is 3 s and the control's wait 6 s.

**Red on demand, twice.** The demonstration asked for is the hub made to
answer: `HandWrittenHub::bind(Answers::Nothing)` changed to
`Answers::Health` inside the first test, nothing else touched. It was run
that way at the earlier figures and again at the committed ones, and both
failed at the same assertion with the same sentence:

```
thread 'a_runner_whose_hub_says_nothing_concludes_the_link_is_dead_and_dials_again'
  panicked at herd-hub/tests/silent_hub.rs:348:5:
the runner says a silent link is dead, naming the limit it was given did not
happen within 30s; the runner said:
[…]
herd-runner: joining ws://127.0.0.1:35377
[…]
herd-runner: dri: present
herd-runner: kvm: present
herd-runner: vfio: present
```

What the output proves is the three things at once: the runner **did**
join (so the harness, the handshake and the data root are all sound), it
**stayed** for the whole of `PATIENCE` — thirty seconds against a three
second limit, ten times over — and it printed **nothing** of the kind, its
output ending on its ordinary capability report. So the assertion is about
silence specifically, and not about any of the other ways this test could
have been green. The failure mode is also the right one: the test ran out
of patience rather than hanging, because `wait_until` polls the child's
exit status and the deadline rather than a condition alone.

## Not done, and why

- **No production code, and none was needed.** The conclusion was testable
  as it stands: the limit is already a command-line figure and the sentence
  is already printed.
- **The handshake's own silence call site is untested still.**
  `config.hub_silence_limit` guards **two** reads: `serve_hub`'s
  steady-state loop, which this file covers, and the `next_message` that
  waits for `Welcome` after `Hello` is sent (`herd-runner/src/lib.rs:3550`).
  A hub that completes the WebSocket upgrade and never sends `Welcome`
  prints the same sentence from the other site, and the consequence
  differs: that connection was never welcomed, so `Backoff::settled` is
  false and the give-up budget for *a hub that cannot be reached* applies
  to it (D104). It is one more `Answers` variant in this file's harness —
  named in a board row below rather than half-built, because the brief
  asked for one test.
- **F5 and F7 of the audit are untouched** — they were not in this brief.
  The board row below ranks what is left.

## Gates

All four Rust gates plus `check-docs.sh`, run on the tree **before** this
handoff was committed. Every heavy phase was joined to
`./scripts/bench-preflight.sh` with `&&`, in one command
(`gotchas/a-gate-joined-by-a-semicolon-is-advice.md`), and one of those
preflights **refused** on a one-minute load of 11.82 against the 9.00
ceiling: nothing behind the `&&` ran, and the phase was re-run after the
host came back to a plateau.

```
$ nix develop --command cargo fmt --all --check
FMT=clean

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.08s
(no warning, exit 0)

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.35s

$ nix develop --command ./scripts/test.sh
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED

$ ./scripts/check-docs.sh
docs ok
```

The figure the brief asked for first, `herd-runner`'s own library tests on
the base, run before the gate and reported for what it is — a reading of
the lib, which this diff cannot touch:

```
$ nix develop --command cargo test -p herd-runner --lib
test result: ok. 217 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.67s
```

**Skips: 12, all of them this host's standing cgroup-delegation list** —
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
`the_runner_says_which_memory_ceiling_it_enforces`. **No skip of this
file's own**, and the two flood benches did **not** decline: neither
appears in the list. A `HERD-SKIPPED` line is a skip and never a pass
(D27).

**The last run, named and not quoted:** bare `./scripts/gate.sh` — never
inside `nix develop` — on this commit, with its log and receipt under this
worktree's `$HERD_GATE_RECEIPTS` (default `target/herd-gate/`). Its
verbatim output is in the agent's report, because a gate run made before
this file was committed tested a different tree and a second commit to
carry the output would need a second gate run.

**And it was run twice, which is the one place that rule bit.** The first
run was on `954032a`, this handoff's first version; it is where the 6.18 s
and the unnamed `6 ignored` above were read. Then this file was amended to
carry both figures — a handoff that promised a figure it did not give is
worse than one that costs a second gate — so the committed sha moved and
the gate was run again on it. Both verdicts are in the agent's report.
Nothing but this file differs between the two trees; `herd-hub/tests/`
is byte for byte `ed970ea`'s in both.

## Risks and findings

1. **The audit's F6 is right in every particular I could check, and I
   looked for a way it was not.** The zero is a real zero (the needle
   matches the production site at the same commit), the conclusion really
   is drawn from `HUB_SILENCE_LIMIT` alone, and the only thing tested
   before today was the *configuration refusal*, exactly as the row says.
   One drift, which changes nothing: the row cites that refusal at
   `killed_hub.rs:1618`, and on this base the function
   `a_lost_after_shorter_than_the_health_cadence_is_refused_at_startup` is
   at `:1638` — the four tests written since 02:00 moved it.
2. **A premise of this task's brief does not hold on this base: `gate.sh`
   does **not** report an `#[ignore]`d test as a compile-time skip by
   name.** The brief says it now does, "as of last night". Measured:
   `grep -rn "ignored" scripts/` finds nothing of the sort,
   `grep -rn "ignore" scripts/gate.sh` finds one line about a *gitignored*
   directory, and `scripts/report-skips.sh` and the gate's step 7 are
   wholly about runtime `HERD-SKIPPED` lines. The green gate's own step 6
   output settles it: one test binary reports `6 ignored` and **not one of
   the six is named anywhere in the log or the receipt**, while step 7's
   list is the twelve runtime skips and nothing else. The gate's refusal of an
   `#[ignore]` **without** a reason was not reachable to test, since no
   `#[ignore]` was added. There is a worktree on this host for a branch
   named `orch202-ignored-skips`, which is presumably where that change
   lives; it is not an ancestor of `b16f732`. **This is why no `#[ignore]`d
   twin was written**, and the merger should know that the argument for the
   shortened threshold rests partly on a fact that the batch may change.
3. **A WebSocket keepalive would make `HUB_SILENCE_LIMIT` unreachable, and
   nothing says so.** `next_message` restarts its `tokio::time::timeout`
   on every frame it loops past, and `Message::Ping(_) | Message::Pong(_)
   | Message::Frame(_) => continue` is one of those loops. So a ping every
   30 s — from either side, or from a reverse proxy that generates its own
   — resets the clock for ever, and a hub that answers no protocol frame
   is believed indefinitely: the exact failure F6 describes, arrived at by
   adding a keepalive rather than by removing one. It holds **today** only
   because neither peer sends pings (`grep -n "Ping\|Pong"` over
   `herd-runner/src` and `herd-hub/src` finds only those three skip arms)
   and `tokio-tungstenite` answers pings rather than originating them.
   This is a latent product hazard, not a defect, and a board row below
   carries it. `silent_hub.rs` is what would notice: its conclusion would
   simply never come.
4. **This test asserts the steady-state site and not the handshake's.** See
   *Not done* — same sentence, different arm, different backoff
   consequence.
5. **The control's own limits, stated so nobody reads more into it than it
   proves.** `a_hub_that_answers_keeps_the_same_runner_on_the_same_link`
   waits twice the threshold and no longer. It shows the detector is silent
   when there is nothing to detect; it is not evidence about a link that
   stays up for an hour.

## Drafted for `herd-merger`

No `docs/decisions.md` entry: this branch decides nothing that outlives it
— the one choice it made (a shortened threshold rather than an
`#[ignore]`d twin) is a property of one test file and is argued inside it.
Two `board.md` rows, in full.

### Board row one — F3, F4 and F6 are closed; F5 and F7 are what is left

This **replaces** `history/2026-10-30-foreign-and-quiet.md`'s *Board row
one* (drafted there as `ORCH-NEXT`), which itself replaced
`history/2026-10-30-audit-tests.md`'s *Board row three*. If either has
already been allocated a number, this text is that row's new *Item* and
*Next action*; if neither has, this row supersedes both.

```markdown
| ORCH-NEXT | **The startup audit's F3, F4 and F6 are tested; F5 and F7 are not.** `runner-silence` closed **F6/R3**, the runner's `HUB_SILENCE_LIMIT`, which was asserted by nothing anywhere — `rg "so this link is dead"` returned zero across every test directory on `b16f732`, with the needle proved against the production site at the same commit. It is now `herd-hub/tests/silent_hub.rs`: the **real `herd-runner` binary as a process** against a hand-written listener that completes the handshake, welcomes it, counts its health reports and answers none, asserting the runner's own sentence with the duration **read back out of it**, that it waited the whole limit, and that **a second connection really arrives** — plus a control with the same hub answering, which must produce no such sentence and one dial. The threshold is the deployed pair ÷ 20 (3 s / 0.75 s, ratio 4 as 60 / 15), so the runner lets the same four unanswered reports pass as in the field, and `herd-runner --help`'s published `[default: 60]` is asserted against `HUB_SILENCE_LIMIT` so the deployed figure is read out of the binary. What is left has no test in any harness: **F5/H2a-H2b**, the watchdog's two checks, best covered today by a test that cuts a real TCP relay against an *in-process* hub and whose deployed-threshold twin is `#[ignore]`d; **F7/R4**, `MOST_ENTRIES_SWEPT` = 50 000 against a consumer scale of ~69 600 entries, where `names.sort()` makes the starved tail deterministic. **And one residual of F6 itself:** `hub_silence_limit` guards two reads, and only the steady-state one is covered — a hub that completes the WebSocket upgrade and never sends `Welcome` prints the same sentence from `lib.rs:3550`, where the connection was never welcomed, so `Backoff::settled` is false and the give-up budget applies (D104). That is one more `Answers` variant in `silent_hub.rs`'s own harness | the startup audit of 2026-10-30, F3-F7; closed in part by `foreign-and-quiet`, then by `runner-silence` | herd-step | owed | **F5 next**, since F6's residual is an hour's work in a harness that now exists and F7 is a *figure* rather than a test: measure the entries per spool a consumer pack produces before the pack half multiplies them, then decide whether the bound moves. Take F6's residual with F5 or with whatever next touches `silent_hub.rs` | — | `…/scratchpad/2026-10-30-startup-sequence-audit.md` F3-F7, rows R2b, R3, R4, H1b, H2a, H2b; `herd-hub/tests/silent_hub.rs`; `herd-hub/tests/killed_hub.rs`; `herd-hub/tests/quiet_runner_displaced.rs`; `herd-runner/src/lib.rs:3550`, `:3848`; **D104**; `history/2026-10-30-runner-silence.md`, `history/2026-10-30-foreign-and-quiet.md` |
```

### Board row two — a keepalive would silently disarm `HUB_SILENCE_LIMIT`

```markdown
| ORCH-NEXT+1 | **A WebSocket ping would make the runner's 60 s silence limit unreachable, and nothing in the code or the docs says so.** `next_message` (`herd-runner/src/lib.rs:3842-3881`) restarts its `tokio::time::timeout` on every frame it loops past, and `Message::Ping(_) \| Message::Pong(_) \| Message::Frame(_) => continue` is one of those loops — so one keepalive every 30 s from either peer, or from a reverse proxy that generates its own, resets the clock for ever and a hub answering no protocol frame is believed indefinitely. That is F6's own failure, reached by *adding* a keepalive rather than by removing a check. It holds today only because neither peer sends pings (`grep -n "Ping\|Pong"` over `herd-runner/src` and `herd-hub/src` finds only those three skip arms) and `tokio-tungstenite` answers pings rather than originating them. The hub's `watch_for_a_lost_runner` has the mirror-image exposure on its own side. **Latent, not a defect**, and cheap to close in prose; the behaviour change is not cheap and is not an agent's to pick | `runner-silence` *Risks and findings* 3, 2026-10-30 | herd-step | open | Smallest honest step is a **comment at those three arms and at the constant** saying the silence is about protocol frames and that a transport keepalive disarms it — no behaviour change, no decision needed. The alternative is to make the limit count **protocol** frames only, so a ping no longer resets it, which changes what a runner does on a live-but-mute link and should be decided rather than slipped in. `herd-hub/tests/silent_hub.rs` is what would catch a regression either way: its conclusion would simply never come | — | `herd-runner/src/lib.rs:106`, `:3842-3881`; `herd-hub/src/server.rs:8203`, `:8283`; `herd-hub/tests/silent_hub.rs`; the startup audit of 2026-10-30, F6/R3 |
```

## Decisions needed

None. The two questions this task met were settled inside it with the
evidence written down: the shortened threshold rather than an `#[ignore]`d
twin (*Done*, and *Risks* 2 for the fact it leans on), and the hand-built
silent side rather than a production path for a hub that goes mute
(*Done*).

## Resume point

```sh
# F5/H2a-H2b: the hub's watchdog, the last NONE row that is a test rather
# than a figure. silent_hub.rs is the harness to read first — a real
# runner against a peer that can be made to misbehave — and the brief
# should say whether the relay stays in-process or the hub becomes real.
git fetch origin && git worktree add -b watchdog-silence <path> origin/runner-silence
```

## Processes

- The six runs of `silent_hub.rs`, the red demonstration and the suite each
  started a real `herd-runner` (and, in the suite, real hubs and
  `herdctl`s) as children of the test process. Every one is
  `kill_on_drop(true)` and each test kills its own child by pid before
  returning; nothing was signalled by name, nothing of the first consumer's
  (hub 3006543, runner 3021002) was touched, and no process of this task
  is left running.
- Two background waits of my own, both exited: one `until` loop on
  `bench-preflight.sh` after its refusal on load 11.82, and one that waited
  for two consecutive one-minute readings under 5.0 before the re-run.
- `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/runner-silence`, never on the
  root filesystem, and `rm -rf`'d once the branch was pushed.
