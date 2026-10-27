# orch-081-hub-stop-path — handoff 2026-10-27

- **Item:** ORCH-081
- **Agent:** herd-step
- **Branch / commit:** `orch-081-hub-stop-path` / `4b879ae`, based on
  origin/main `cd59ef5`
- **PR:** #96

## Done

The hub has a stop path. Before this, `SIGTERM` reached it with its default
disposition and killed it exactly as `SIGKILL` does, so every ordinary
restart — `docker compose stop`, a redeploy, a host reboot — was a crash to
the store.

**The shape, in four pieces.**

1. `StopSignals` (`herd-hub/src/server.rs`) holds the `SIGTERM` and `SIGINT`
   handlers. It is a **value**, installed by `main` as its very first
   statement, before the options are parsed and before `JobDirStore::open`
   sweeps the state root. Until a handler exists `SIGTERM` kills the
   process, and opening a large state root is the longest uninterruptible
   moment the binary has; installing first turns a signal landing in that
   window into a buffered notification the serving loop acts on the instant
   it begins. A handler that cannot be installed is said in one line and the
   hub starts anyway, keeping today's behaviour for that one signal.
2. `State::stopping`, an `AtomicBool` set once and never cleared. Two
   readers, both about *not starting* work the hub will not be there to
   finish: `accept_submissions` refuses a `SubmitJob` **before anything is
   minted**, which is what makes it sound for `herdctl` to read the refusal
   as "no job exists" (D142); and `dispatch_queued_jobs` returns without
   starting a pass, so a queued job stays queued for the next hub instead of
   being numbered onto a runner this one is about to disconnect from.
3. `let_the_pass_finish(state, within)` polls `State::dispatching` — the
   claim lock, held for the whole of a pass and by nothing else that writes
   — every 5 ms until it is free or `within` has gone. `Some(took)` is a hub
   that may leave, `None` is the bound expiring with a pass still running. A
   *poisoned* lock counts as free, as it does at every other site that takes
   it: a poisoned claim lock is a pass that panicked, which is the question
   being asked.
4. `Hub::serve_until_stopped(signals) -> Result<Stop>`, with
   `Stop::{Clean, Overran, ListenerFailed}`. `Hub::serve()` is unchanged in
   behaviour and listens for no signal: an in-process hub shares its process
   with its test, and a library that installed a handler on its caller's
   behalf would be deciding what that process does when it is stopped. Both
   go through one `serve_until`, so the hub the tests drive and the hub the
   consumer runs cannot drift.

**What the operator sees.** A clean stop exits **0** and prints how long the
pass in flight took. An overrun exits **non-zero** with a sentence naming the
bound. Measured by hand on this workstation:

```text
herd-hub: SIGTERM and SIGINT stop this hub cleanly — it stops accepting
connections and submissions, lets the dispatch pass in flight finish, and
exits, giving that pass at most 5s. Give the container at least that much
grace. Jobs already running are not cancelled and their runners keep their
verdicts until a hub records them.
herd-hub: this hub was asked to stop (SIGTERM). No new connection and no new
submission is accepted, and no further dispatch pass starts. Waiting up to
5s for the pass in flight. Jobs already running are not cancelled: they
belong to their runners, which hold their verdicts until a hub records them
(E-44).
herd-hub: this hub was asked to stop (SIGTERM), and it stopped cleanly: the
dispatch pass in flight was over after 750ns, inside the 5s this hub allows.
Nothing queued was lost and no attempt was numbered on the way out.
EXIT=0
```

`SIGINT` was exercised the same way and prints the same thing with `SIGINT`
in it, exit 0, wait 2.395 µs.

**The bound, and it is a proposal.** `STOP_WITHIN = 5 s`.

- It is not invented here. Invariant 2 binds a *runner* to five seconds and
  says nothing about the hub; `07` **V9** proposes the same figure for the
  hub, mirroring the runner, and this is that figure made real.
- It is reachable on the deployment's filesystem. The thing waited for is
  **one pass**, and a pass is almost entirely `fsync`: `08` measures a
  junction at `N` = 1 000 at **9.970 s** on ZFS/SATA, and `11 §7`
  recomputes the same pass on the class the deployment runs on — ext4 on
  NVMe — at **3.128 s**. Three seconds fits inside five with room for the
  rest of the stop.
- It is **not** reachable on ZFS at that depth: 6.949 s in `11`'s own cell,
  9.970 s in `08`'s. A hub with a thousand queued jobs on ZFS overruns and
  exits non-zero. That is said out loud in the constant's doc comment rather
  than hidden.
- After ORCH-083 and the rest of the tier the same pass is 129 ms (`08`), so
  the bound stops being marginal anywhere.
- Five also leaves room inside docker compose's own ten-second default
  grace, which is the thing that would otherwise `SIGKILL` the stop path
  before it finished.

**Past the bound.** The hub says so by name and exits anyway. The store is
left exactly as a `SIGKILL` at the same instant would have left it: every
write in `durable.rs` is a synced temporary renamed into place or a synced
append, so a reader sees the whole old file or the whole new one, and
`JobDirStore::open` already recovers an attempt the hub died inside (the
four killed-hub tests). **An overrun is a crash; it is not a corruption.**

**How much of the window a clean stop removes.** Today, before the tier, it
removes the *partiality* of a pass cut in half — some jobs of the pass
judged and the rest not, and in the worst case an attempt numbered on disk
whose `AssignJob` never reached its runner. Once ORCH-083 lands, the tier's
crash window is the events the page cache has not yet written back. A clean
stop removes **all** of that window for the ordinary stop, because the pass
completes and the process leaves of its own accord with nothing in flight —
and, when S4 lands, that is where its flush step belongs (the place is
marked in `let_the_pass_finish`'s doc, and there is nothing to flush today
because every write is already synchronous). **It removes nothing from a
power cut**, a host hard-reset or a `SIGKILL`: in those the page cache dies
with the machine and the window is exactly what the tier opened, no smaller.
So the trade is: the *frequent* stop stops being a crash, the *rare* one
stays one.

**Runners and requesters in flight.**

- A **runner** mid-job: its control link ends, its job keeps running (D17),
  its verdict is spooled to its own disk until a hub acknowledges it (E-44,
  D34), and it redials with backoff. The hub sends it nothing on the way
  out — in particular **no `Refused`**, which is terminal (D13) and would
  kill it, and **no `CancelJob`**, which would kill its job. The hub that
  comes back starts the lost-after clock from its own start, so the runner
  has the whole of `--lost-after` to redial. Proved: the e2e test asserts
  the runner is alive after the stop and that its held verdict (exit 7, plus
  output produced after the hub went) is recorded by the second hub.
- A **requester following logs**: the link ends under it. `herdctl` answers
  on its no-verdict path — exit **125**, the reason on stderr, the job's id
  printed — and never invents a verdict. `herdctl wait <id>` against the
  hub that comes back gets the job's own code. Asserted.
- A **requester submitting** during the stop: refused by name, before
  anything is minted, with a reason that says to submit again once the hub
  is back. E-05 makes that retry safe.

**Idempotence.** `State::stop_now()` returns `true` only the first time. A
second signal is read by the same handlers — which is why they are a value
and not a fresh registration — and answered with a line and nothing else: a
stop under way is never hurried, because hurrying it is what would turn it
back into the crash it exists to stop being. The bound is what keeps that
safe, since once a handler is installed `SIGTERM` can no longer kill the
process. `SIGKILL` remains unhandleable and the store comes up from one.

**No protocol change.** `PROTOCOL_VERSION` stays 2 and `RECORD_VERSION`
stays 1. No frame was added, removed or changed; nothing is sent on the way
out at all.

**Tests.** Five new, all green.

- `herd-hub/tests/stopping.rs`, real `herd-hub`, `herd-runner` and
  `herdctl` processes and a real `kill(2)`:
  - `a_hub_asked_to_stop_leaves_cleanly_and_the_store_is_whole` — a job
    really running, a real `herdctl wait` following it, twelve jobs queued
    behind a label nothing offers. `SIGTERM`. Asserts: **exit code 0**,
    which *is* the bound (the hub times its own wait with an `Instant` and
    exits non-zero on an overrun); the three sentences it must print; the
    runner alive; the running job still `running` and not cancelled; all
    twelve queued jobs still `queued` and none half-assigned; the state root
    whole (no `.tmp-` left behind, every `job.json` parses and names a
    state, every line of every `events.jsonl` parses); the follower's 125,
    its reason and its printed id; then a second hub on the same root and
    port reporting `13 job(s) not over` and `0 quarantined`, the runner's
    held verdict recorded as exit 7 with its late output, and
    `herdctl wait` returning 7.
  - `a_burst_of_stop_signals_is_still_one_clean_stop` — 201 `SIGTERM`s.
    Asserts exit 0, the queued job untouched, the runner alive, the state
    root whole, and the verdict recovered by the hub that comes back.
- `herd-hub/src/server.rs`:
  - `the_stop_waits_for_the_pass_in_flight_and_gives_up_at_its_bound` —
    holds the claim lock from a thread; `let_the_pass_finish` answers `None`
    at a 50 ms bound, and `Some` once the lock is free. **This is where the
    bounded wait is proved exactly**, because a pass can be held open here
    and cannot be from outside the process.
  - `a_hub_that_is_stopping_starts_no_dispatch_pass` — a serving hub writes
    the queued job's `waiting_for`; a stopping hub writes nothing and leaves
    it `Queued`.
  - `a_second_stop_signal_changes_nothing`.

**Timing margins, and why.** The only wall-clock assertion in the e2e file
is `took < 30 s`, three times the bound plus room, and it exists to catch a
hub that does not come back at all. The real bound is asserted through the
**exit code**, which the hub derives from its own `Instant`: re-measuring it
from outside would add signal delivery, process teardown and this host's
scheduler to a figure the hub already knows exactly, and that is precisely
the shape of an assertion a loaded shared host turns red for no product
reason. The unit test asserts orderings (`None` then `Some`), never a
duration. Both files were run three times in a row on this workstation with
no variation.

## Not done, and why

1. **No `docs/decisions.md` entry.** The brief did not ask for a `D-NEXT`
   entry and the contract forbids writing one unasked. `STOP_WITHIN = 5 s`
   is a figure that wants recording — see *Decisions needed*.
2. **No graceful WebSocket close on the way out.** The process exits and its
   sockets go with it, so a peer reads a reset and a clean stop is, on the
   wire, indistinguishable from a kill. Closing each peer's socket properly
   would give the follower a better sentence without touching the protocol,
   but it changes which branch a *runner* takes on an ending link, and D104
   records that branch (`Ok(())`, a clean close) as one that measurement says
   never happens today. Changing it deserves its own task and its own
   measurement, not a line smuggled into this one. The current behaviour is
   now **asserted** rather than merely tolerated, so a future change to it is
   a decision and not a surprise.
3. **The submission refusal's wording is not asserted by a test.** The flag
   that produces it is (`a_hub_that_is_stopping_starts_no_dispatch_pass`
   exercises `State::stopping`), and the refusal is three lines above the
   mint in `accept_submissions`, but making a real `herdctl submit` land
   inside a stop that is microseconds wide is a race, and a test that
   depended on winning it would be flaky by construction. Closing it
   honestly needs a wide stop, which needs a pass held open, which needs a
   test door the hub does not have.
4. **`docs/status.md`, `docs/coverage.md` and `docs/plan.md` untouched.** The
   brief scoped the board to the ORCH-081 row.
5. **The "a second signal arrived" line is not exercised by any test.** It
   only prints while a drain is running, and a drain here is microseconds.
   The behaviour behind it — the second signal changing nothing — is proved
   by the unit test and by the 201-signal burst.

## Gates

`cargo fmt --all --check`:

```text
$ nix develop --command cargo fmt --all --check
FMT_EXIT=0
```

`cargo clippy --workspace --all-targets -- -D warnings`:

```text
$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Checking herd-hub v0.3.7
    Checking herd-runner v0.3.7
    Checking herdctl v0.3.7
    Finished `dev` profile [unoptimized + debuginfo] target(s)
```

`cargo build --workspace`:

```text
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 26.69s
```

`./scripts/check-docs.sh`:

```text
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/test.sh`: **red on this workstation, for host reasons, and the
same reasons are red on pristine `cd59ef5`.**

```text
$ nix develop --command ./scripts/test.sh
error: 2 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test slots`
…
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were
 skipped, and the suite did not finish (cargo test exited 101),
 so this list is a floor and not a count.
test suite: FAILED (cargo test exited 101)
```

Two distinct host causes, neither this branch's, and both separated by
experiment rather than by assertion:

1. **ORCH-069, the memory constants.** `herd-hub/tests/slots.rs:744`
   (`the_derived_j_is_recorded_and_is_what_the_job_is_given`) and the
   `admission` failures. The runner here announces **2.6 GiB**, not the
   host's 8 GiB, because an enclosing `memory.high 18 GiB on /chatons.slice`
   already holds 15.3 GiB — so `admission.rs:419`'s
   `"3 GiB free of its 7 GiB memory admission, 5 GiB needed"` can never be
   reached and the tests ahead of it time out. **Checked against a pristine
   worktree at `cd59ef5`**, built and run on its own: `admission` fails
   there too and *worse* — four tests rather than two, the set drifting with
   free memory.

```text
# worktree at cd59ef5, nothing from this branch
$ nix develop --command cargo test -p herd-hub --test admission -- --test-threads 1
failures:
    a_killed_hub_reads_the_reservations_back_off_the_records
    a_refusal_carries_the_free_figure_and_the_needed_figure
    a_starved_reservation_waits_then_starts_and_leaks_nothing
    the_load_is_on_the_placement_line_and_stops_nothing
test result: FAILED. 1 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 139.56s
```

2. **Host contention, at `real_herd/mod.rs:649`.** That line is
   `herdctl … never came back` — the harness's 30 s `PATIENCE` on one
   `herdctl` process, not a refusal and not a product answer. `uptime` read
   **load average 20.62** while this was running, with several agents
   compiling and running suites on this machine. A batch `killed_hub` run
   reported four failures; a second batch run reported a *different* three,
   all at that one line, which is the signature of contention and not of a
   defect. Run individually under the same load, the four tests that matter
   to this change are all green:

```text
$ nix develop --command cargo test -p herd-hub --test killed_hub <name> -- --test-threads 1
--- a_hub_asked_to_stop_mid_job_loses_nothing
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 17 filtered out; finished in 20.98s
--- a_connection_that_served_starts_the_redial_counters_over
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 17 filtered out; finished in 12.28s
--- a_health_cadence_of_zero_is_refused_on_both_sides
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 17 filtered out; finished in 10.59s
--- a_busy_runner_asked_to_stop_is_gone_in_five_seconds
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 17 filtered out; finished in 11.16s
```

   The first two are the ones this change could plausibly have broken:
   `a_hub_asked_to_stop_mid_job_loses_nothing` sends the very `SIGTERM` this
   PR reinterprets, and
   `a_connection_that_served_starts_the_redial_counters_over` is D104's —
   the one that cares which branch a runner takes on an ending link. Both
   green.

The new tests, run three times in a row under the same load:

```text
$ nix develop --command cargo test -p herd-hub --test stopping -- --test-threads 1
--- run 1 ---
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 21.85s
--- run 2 ---
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 22.25s
--- run 3 ---
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 21.92s

$ nix develop --command cargo test -p herd-hub --lib -- stop
test server::tests::a_second_stop_signal_changes_nothing ... ok
test server::tests::a_hub_that_is_stopping_starts_no_dispatch_pass ... ok
test server::tests::the_stop_waits_for_the_pass_in_flight_and_gives_up_at_its_bound ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 177 filtered out; finished in 0.12s
```

**Skips, copied and never counted as passes (D27).** `scripts/test.sh`
printed **at least 12**, every one of them cgroup v2 delegation on this
unprivileged host (`Permission denied` creating a cgroup under
`/sys/fs/cgroup/chatons.slice/…`), and the script says itself that the
figure is a **floor and not a count** because the run did not finish. They
are in the task's scratchpad verbatim; none of them is this branch's and
none is reported as a pass. Separately, `slots.rs:744` is **skipped** on a
GitHub runner — so "green on CI" is never said of it here either.

## Risks and findings

1. **The bound is not reachable on ZFS at `N` = 1 000.** `11 §7` measures
   6.949 s and `08` 9.970 s for a pass at that depth on ZFS/SATA, against a
   5 s bound. The deployment is ext4 on NVMe, where the same pass is
   3.128 s, so the bound holds *there*; a hub on ZFS with a deep queue will
   exit non-zero on stop until the tier lands. This is written into the
   constant's doc comment, not left for someone to rediscover.
2. **Once a handler is installed, `SIGTERM` can no longer kill the hub.**
   That is the point, and it makes the bound load-bearing: if
   `let_the_pass_finish` could wait for ever, the hub would be unkillable by
   `SIGTERM`. It cannot — the bound is a hard `Instant` comparison with no
   path around it — and `SIGKILL` is deliberately untouched.
3. **A clean stop still looks like a reset on the wire.** Finding 2 under
   *Not done*. It costs nothing decisive (the follower gets 125 and the id,
   the runner spools and redials) but it means the consumer cannot tell a
   stop from a crash by watching a client.
4. **`killed_hub.rs`'s `a_hub_asked_to_stop_mid_job_loses_nothing` now
   exercises a different path than its name suggests.** It sends `SIGTERM`
   and used to be testing the kill; it now tests the stop. Its assertions all
   still hold and it is green, but the file's own header still says a hub
   killed by a signal ends the link "with a reset", which is now only true
   of `SIGKILL` by intent — though in practice, per finding 3, still true of
   both. Not edited: the brief scopes this task to `main.rs`, `server.rs` and
   one test, and rewording another file's prose to match is a separate,
   deliberate act.
5. **The premise of the brief held.** `SIGTERM` really did kill the hub
   outright on `cd59ef5`; nothing in the code refuted it.
6. **The workstation is at load average 20.6 and that is now a distinct,
   reproducible source of red.** Several agents are compiling and running
   suites on it at once, and a `herdctl` that takes more than the harness's
   30 s `PATIENCE` fails a test at `real_herd/mod.rs:649` with
   `herdctl … never came back`. Two batch runs of `killed_hub` produced two
   *different* sets of failures at that one line; all of them pass run
   singly. This is separate from ORCH-069 and has no board row; it is worth
   one, because the next agent to see it will spend the same hour I did
   proving it is not theirs.

## Decisions needed

**`STOP_WITHIN = 5 s` — is this the hub's bound, and should it be written
down as a decision?**

- *Recommendation:* adopt 5 s, as `07` V9 proposes, and record it in
  `docs/decisions.md` through `herd-merger` as a `D-NEXT` entry that says
  three things: the figure mirrors invariant 2's runner bound; it is
  justified at `11`'s 3.128 s ext4 pass at `N` = 1 000 and **not** at `08`'s
  9.970 s ZFS pass; and an overrun is a crash and not a corruption. It is a
  *proposal* in this branch — stated as such in the code — because the hub
  has never had a figure of this kind written anywhere, and inventing one
  silently in a constant is how a number nobody agreed to becomes a promise.
- *Fallback:* leave it unrecorded and treat the constant as an
  implementation detail. Cheaper now, and it means the first operator to see
  a non-zero exit on `docker compose stop` has nothing to read about why.

## Resume point

`herd-merger` integrates the PR on green CI and allocates the `D-NEXT`
number if Alexa adopts the bound. Then **ORCH-083** is unblocked: its
dependency list is ORCH-075, ORCH-079 and ORCH-081, and this is the last of
the three that was open. ORCH-083's own flush step belongs in
`let_the_pass_finish`, between the claim lock coming free and the return —
the place is named in that function's doc comment, and the 5 s bound has to
cover whatever it costs.

## Processes

Started and ended by this task, all in its own worktree and scratchpad:

- `cargo`/`rustc` under `nix develop`, foreground, each reaped by its own
  command.
- Two `herd-hub` processes started by hand on ports 18787 and 18788 with
  state roots under this task's scratchpad, each signalled by its own PID
  (`kill -TERM`, `kill -INT`) and waited for. Both exited 0.
- The test harness's own `herd-hub`, `herd-runner` and `herdctl` children,
  started and reaped by `real_herd`.
- One `cat` of my own (PID 3460537) left blocked on stdin by a malformed
  shell line, and its parent shell (3460536); both killed by PID.

No `pkill`, no `killall`, no kill by name. No process not started by this
task was signalled.
