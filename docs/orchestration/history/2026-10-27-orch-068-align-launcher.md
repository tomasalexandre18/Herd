# orch-068-align-launcher — handoff 2026-10-27

- **Item:** ORCH-068 (plus two `CLAUDE.md` rule changes decided 2026-10-27)
- **Agent:** herd-step
- **Branch / commit:** `orch-068-align-launcher`, based on origin/main
  `cd59ef5`
- **PR:** see the branch's pull request

## Done

**1. The launcher is aligned on the live one (ORCH-068).** Alexa decided
2026-10-27 that the repository follows the file the sessions on this host
are really started by, `/mnt/ssd_980/herd/claude/env.sh`, because it is
the one in use and the one that works.

Three behavioural changes in `ops/agents/env.sh`:

- `GH_CONFIG_DIR` is exported. It was absent, and `HOME` is redirected
  into the state root, so `gh` read `$XDG_CONFIG_HOME/gh`, which does not
  exist on this host. A session started by `ops/agents/session.sh` as
  shipped could not `gh pr create`, could not `gh pr merge`, and could
  not `git push` either: `origin` is https and `~/.gitconfig` sets
  `credential.https://github.com.helper = !gh auth git-credential`, which
  reads the same directory. It now defaults to the host user's
  `~/.config/gh` (taken from `getent passwd`, so a re-source cannot
  resolve it into the Herd home) and `HERD_GH_CONFIG_DIR` overrides it. A
  path, never a token: nothing of the login is in this repository
  (invariant 7). It is deliberately outside the state root and the guard
  does not cover it.
- `CARGO_HOME` moved from `$HERD_STATE_ROOT/cargo` to
  `$HERD_CLAUDE_ROOT/cargo`. `/mnt/ssd_980/herd/cargo` does not exist;
  `/mnt/ssd_980/herd/claude/cargo` holds the 101 MB registry in use.
- `PATH` now puts the live launcher's four directories in front, in its
  order: Herd's own `dtach`, `/run/wrappers/bin`,
  `/etc/profiles/per-user/<user>/bin` (where `claude` is on this host)
  and `/run/current-system/sw/bin`. What was inherited stays behind them
  rather than being dropped, so the file is still sourceable on a host
  that is not this one — the guard at the bottom needs `realpath` and
  `mkdir`, and neither is in the four.

`ops/agents/session.sh` warns, without refusing, when `GH_CONFIG_DIR`
holds no `hosts.yml`, so a session learns at the start of a task that it
cannot open a pull request rather than at the end. Its stale hint
pointing at `/mnt/ssd_980/chatons/ops/agents/tools/bin/claude` is gone:
reaching for another project's copy is the failure the directory exists
to prevent, and `PATH` now carries this host's own.

**2. The drift is caught from now on.** `scripts/check-orchestration.sh`
has a new section, 27 assertions: it copies `env.sh` and `session.sh`,
rewrites the one hardcoded `HERD_STATE_ROOT=` line to a scratch
directory, sources them from a hostile environment (someone else's
`HOME`, `CHATONS_*` markers, an ambient `ANTHROPIC_API_KEY`) and asserts
the result — every state path under the sandbox root, `CARGO_HOME` under
the **Claude** root, the directories created, the API key and the
markers gone, `GH_CONFIG_DIR` set, absolute, a `gh` directory and
**outside** the state root, `HERD_GH_CONFIG_DIR` honoured, the `PATH`
prefix and its inherited tail, the guard still refusing a path that
escapes, `session.sh` starting a session, warning when `gh` has no
login, and refusing without credentials and no terminal. The live state
root is never touched by any of it. The control was seen red: with
`export GH_CONFIG_DIR` deleted and `CARGO_HOME` put back at
`$HERD_STATE_ROOT/cargo`, the run is `132 passed, 6 failed` and names
both defects.

**3. Two rules in `CLAUDE.md` "Shared host rules"**, both Alexa's word of
2026-10-27, with the detail in `docs/orchestration/workflow.md`
"Concurrency and host rules" so that `CLAUDE.md` stays under its 150-line
limit (it is at exactly 150):

- *"tu peux envoyer plus de lourd sans abuser, chatOnS qui est la grosse
  source de demande est a l'arret"* — more than two heavy agents may run
  while the first consumer is stopped, judged on load, free memory and
  free disk rather than on a count; the ceiling of two returns the day
  the consumer resumes.
- An agent removes its own `target/` as soon as its branch is on the
  origin — pushed, not merged, and never before its own gates have run;
  its own scratchpad subdirectory only, by a literal path. The reason
  written down is not tidiness: `/mnt/ssd_980` is the same filesystem the
  consumer's production writes to, and 48 GB of abandoned build trees is
  a seventh of the free space the hub's 200 GB output budget draws on.

**4. Documents my change made false.** `docs/orchestration/state-root.md`:
the `CARGO_HOME` row and the registry-seeding command are one directory
deeper; a new section says what is deliberately *outside* the root and
why; the `session.sh` paragraph names the new warning; and the "nothing
of chatOnS's supervision is here" paragraph now separates the `dtach`
**binary** Herd keeps under its own root, which `env.sh` puts on `PATH`,
from chatOnS's socket and supervisor, which are still not here. ORCH-067
covers the rest and was left alone.

## Not done, and why

- **Nothing was written, created or removed under
  `/mnt/ssd_980/herd/claude`**, the live state root. `.credentials.json`
  was never opened. The live `env.sh`, `herd.sh` and `start-herd.sh` were
  read only. Making the live file converge on the shipped one is the
  owner's act, not an agent's.
- **No test proves the shipped file equals the live one.** It cannot: the
  live root is out of scope and a check that read it would be the thing
  the boundary forbids. What a human must compare, and when: after any
  edit to `/mnt/ssd_980/herd/claude/env.sh`, or to `ops/agents/env.sh`
  outside this check, run `diff <(sed -n '/^R=/,$p'
  /mnt/ssd_980/herd/claude/env.sh) ops/agents/env.sh` by eye and look at
  four things only — the state root, `CARGO_HOME`, `GH_CONFIG_DIR` and
  the `PATH` prefix. Everything else in the shipped file is deliberate
  extra (see below).
- **`docs/decisions.md` untouched.** The brief asked for no `D-NEXT`
  entry.
- **The board's other rows untouched**, `herd-merger` holds the file.
  Only the ORCH-068 row changed, from `blocked` to `done`.
- **ORCH-076 not closed.** The orchestrator's disk measurement answers it
  (`/mnt/ssd_980` is `/dev/nvme0n1p1`, ext4, and carries the consumer's
  hub volume and runner data root), but that is another row and another
  owner.

## Gates

`./scripts/check-docs.sh`:

```text
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/check-orchestration.sh`:

```text
== ops/agents: the launcher gives a session what it needs (ORCH-068)
ok    env.sh keeps its state root on one rewritable line
ok    env.sh: HOME=<sandbox>/stateroot/claude/home
ok    env.sh: CLAUDE_CONFIG_DIR=<sandbox>/stateroot/claude/config
ok    env.sh: XDG_CONFIG_HOME=<sandbox>/stateroot/claude/home/.config
ok    env.sh: XDG_RUNTIME_DIR=<sandbox>/stateroot/claude/run
ok    env.sh: TMPDIR=<sandbox>/stateroot/claude/tmp
ok    env.sh: TEMP=<sandbox>/stateroot/claude/tmp
ok    env.sh: HISTFILE=<sandbox>/stateroot/claude/home/.bash_history
ok    env.sh: CARGO_HOME=<sandbox>/stateroot/claude/cargo
ok    env.sh: ANTHROPIC_API_KEY=<unset>
ok    env.sh: ANTHROPIC_BASE_URL=<unset>
ok    env.sh: CHATONS_ROOT=<unset>
ok    env.sh: HC=<unset>
ok    env.sh creates home
ok    env.sh creates config
ok    env.sh creates run
ok    env.sh creates tmp
ok    env.sh creates cargo
ok    GH_CONFIG_DIR is an absolute gh directory outside the state root
ok    HERD_GH_CONFIG_DIR overrides the default
ok    PATH puts dtach, the wrappers, the per-user profile and sw/bin in front
ok    PATH keeps what it inherited behind the four
ok    the guard refuses a state path outside the root
ok    session.sh starts the session and says nothing when gh is logged in
ok    session.sh warns, and still starts, when gh has no login
ok    session.sh refuses without credentials when no terminal can log in
ok    ops/agents names paths only, never a token (invariant 7)

check-orchestration: 138 passed, 0 failed
```

(The sandbox path is `$TMPDIR/herd-orch-check.XXXXXX`; it is abbreviated
above and printed in full by the run.)

`nix develop --command cargo fmt --all --check`: exit 0, no output but
`warning: Git tree '<worktree>' is dirty`.

`nix develop --command cargo build --workspace`:

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 27.45s
```

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`:

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.49s
```

`nix develop --command ./scripts/test.sh`: **RED, exit 101**, two
failures, both ORCH-069 and both this host:

```text
failures:
    a_killed_hub_reads_the_reservations_back_off_the_records
test result: FAILED. 4 passed; 1 failed; ... finished in 40.22s
error: test failed, to rerun pass `-p herd-hub --test admission`

failures:
    the_derived_j_is_recorded_and_is_what_the_job_is_given
test result: FAILED. 7 passed; 1 failed; ... finished in 2.35s
error: test failed, to rerun pass `-p herd-hub --test slots`

test suite: FAILED (cargo test exited 101)
```

`slots.rs:744` is `left: "built with -j1"` against `right: "built with
-j7"`; `admission.rs:361`, whose memory assertion is at `:419`, times out
at `real_herd/mod.rs:789` waiting for a job this host's memory never lets
run. Both are ORCH-069: a figure the hub derives from the live `MemTotal`
against constants written for a 17 GiB machine. **This diff changes no
Rust and no Cargo file at all** (`CLAUDE.md`, `docs/`, `ops/`, `scripts/`
only), so it cannot be their cause. On CI, `slots.rs:744` is **skipped**,
not passed — by D27 that is not a pass, and this handoff does not claim
one.

Skips: the suite reported **at least 12** skipped assertions and did not
finish, so that is a floor and not a count. Every one is cgroup v2
delegation this host does not grant (`Permission denied` under
`/sys/fs/cgroup/chatons.slice/...`) or a hard IO ceiling it will not
take: `a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
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

## Risks and findings

- **The full divergence list, with a verdict on each.** Live file =
  `/mnt/ssd_980/herd/claude/env.sh`, shipped = `ops/agents/env.sh`.
  1. `GH_CONFIG_DIR` — live only. **Live right**, taken; see above.
  2. `CARGO_HOME` — `claude/cargo` live, `<root>/cargo` shipped. **Live
     right**, taken: only the live path exists and holds the registry.
  3. `PATH` — live replaces it with four directories; shipped prepended
     two to what it inherited. **Live right about the four and their
     order**, taken; **shipped right to keep a tail**, kept, or the file
     stops working off this host.
  4. `HERD_STATE_ROOT`, `HERD_CLAUDE_ROOT`, `HERD_REPO` — shipped only.
     **Shipped right**, kept: `session.sh` needs `HERD_REPO`, and a
     worktree's copy must start a session in that worktree.
  5. The guard loop (`mkdir -p`, refuse a path outside the root) —
     shipped only. **Shipped right**, kept: it is the whole point of
     D174, and the live file creates nothing (`$HOME/.config` does not
     exist under the Herd home).
  6. Clearing chatOnS's markers — `compgen -e | grep '^CHATONS_'` live,
     `${!CHATONS_@} HC` shipped. **Shipped right**, kept: no subshell, no
     `grep`, and it also clears `HC`, which the live file leaves.
  7. The root as an exported variable vs a bare `R=`. **Shipped right**,
     kept.
  8. Root refusal, `claude`-binary check, `.credentials.json` check —
     shipped only, in `session.sh`. **Shipped right**, kept.
  9. Comments in French live, English shipped. **Shipped right** (the
     working agreement), kept.
  10. `HOME`, `CLAUDE_CONFIG_DIR`, the four `XDG_*`, `XDG_RUNTIME_DIR`,
      `TMPDIR`/`TMP`/`TEMP`, `HISTFILE`, the `ANTHROPIC_*` and
      `CLAUDE_CODE_USE_*` unsets, `LD_*`, `umask 077` — identical on both
      sides. Nothing to do.
- **Two files on the live root still start the real session** (`herd.sh`,
  `start-herd.sh`) and neither is in this repository. They source the
  live `env.sh`, not the shipped one, so this branch does not change what
  the running session does. Converging them is the owner's act.
- **`docs/orchestration/state-root.md` still says Herd has no slice on
  this host.** `start-herd.sh` puts the session in `chatons.slice` with
  `systemd-run --scope`. I did not touch that sentence: my change does
  not make it false, and it is ORCH-067's ground.
- **Re-sourcing `env.sh` is idempotent for `PATH`** (the prefix is
  skipped when already in front) but `HERD_GH_CONFIG_DIR` and the state
  root are read fresh each time, which is intended.

## Decisions needed

None. ORCH-068's question was answered before the task started.

## Resume point

`herd-merger` reviews and merges the pull request for
`orch-068-align-launcher` once CI's `check` and `docs` are green. Nothing
else is owed on ORCH-068.

## Processes

None started that outlive this task. The gates ran `nix develop --command
cargo …` and `./scripts/test.sh` in the foreground, in this worktree
only; all exited. No PID was killed. The worktree's `target/` is removed
once the branch is pushed, per the rule this branch adds.
