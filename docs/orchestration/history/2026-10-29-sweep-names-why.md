# sweep-names-why — handoff 2026-10-29

- **Item:** ORCH-NEXT (no board row existed; one is proposed below)
- **Agent:** herd-step
- **Branch / commit:** `sweep-names-why` / `05fbccb` (code) and the commit
  that carries this file, based on origin/main `5d422bf`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`herd-runner/src/sweep.rs`, and nothing else. 348 insertions, 27
deletions, one file.

**The refusal now names the path and the errno, and nothing else about it
moved.** The sentence the first consumer quoted is unchanged word for
word up to `It is left exactly as it is`, and what follows it is new:

```
/mnt/.../outputs/j0000000013-1 could not be read all the way through, so
what this runner can see of it is not the whole collection; it is NOT
offered, because a part of a manifest offered as the whole of one reads
to the hub as a collection it already holds. It is left exactly as it
is. The first path it could not read is /mnt/.../outputs/j0000000013-1/
shut: Permission denied (os error 13). do: make that path readable to
the user this runner runs as — a job that left its output owned by
another user is what this usually is. This sweep reads and never
repairs: it has changed no mode, no owner and no name here, and the next
start offers the whole collection as soon as the path reads
```

(that line is this branch's own test output, read with `--nocapture`, not
a mock-up; `janitor.rs`'s shape — path, error, one action.)

**The reason was already in hand and was being thrown away.** `walk` had
four sites that turned a `std::io::Error` into `whole = false` and
dropped it: `read_dir` on a sub-directory (`Err(_)`), a `readdir` that
fails part way through the iterator, an entry's `symlink_metadata`, and
the entry bound. So the fix **keeps** it — no second walk. `walk` now
returns `Option<NotWhole>` in place of `bool`:

- `NotWhole.unreadable` holds **one** `Unreadable { path, why, and_more }`
  — the first unreadable path in **path order**, the errno's own words
  (`Permission denied (os error 13)`), and a count of the rest. Path order
  and not `readdir` order, for the reason `pick_up` already sorts: a
  refusal an owner cannot compare with yesterday's is one they cannot
  follow.
- `NotWhole.bound` is the entry bound, said as the bound and accusing no
  path: *"The sweep's bound of 50000 entries ran out inside it, so the
  rest of it was never looked at and no path in it is at fault. do:
  nothing — the next start looks again"*.
- Cost: one `PathBuf` and one `String` per spool being refused, zero
  syscalls added. The walk is still one `readdir` per directory and one
  `lstat` per entry.

**Five claims, five tests** (`herd-runner/src/sweep.rs`, all green, none
skipped on this host):

1. `a_spool_that_will_not_read_through_is_not_offered_and_says_which_path`
   — the old test, extended: the line names that path, carries
   `Permission denied`, carries `do: make that path readable`, and
   invents no second path. Built with a mode the test removes itself
   (0o000 on a sub-directory), which is the consumer's own hypothesis.
2. `several_unreadable_paths_name_the_first_and_count_the_rest` — three
   shut directories are **one** line, naming `a-shut`, saying `and 2
   further path(s)`, and never naming `c-shut`.
3. `a_refusal_names_the_errno_in_the_kernels_own_words` — **new, and
   uid-independent**: an `ENOENT` any host can produce proves the
   sentence carries the path, `No such file or directory`, `os error 2`
   and a `do:`, plus the path-order rule. So a root host that can only
   skip 1 and 2 still has the claim.
4. `a_spool_that_reads_through_is_still_offered_with_nothing_said` —
   **who is refused did not change**: `walk` returns `None`, `pick_up`
   says nothing, the spool is offered.
5. The same test 1 asserts the spool is left exactly as it was — the
   readable file's bytes unchanged, and the shut directory's mode still
   `0o700` (only the test's own `put_back` touched it). The walk reads;
   it never repairs.

`the_sweep_stops_at_its_bound_and_says_so` was updated to the new
signature and now also asserts the bound names itself and accuses no
path.

**Not touched, as the brief required:** `herd-hub/src/` (zero lines),
`herd-runner/src/reclaim.rs` (zero lines — it still does not touch
`outputs/`, and `the_reaper_does_not_delete_a_spool_this_sweep_would_
have_resumed` still proves it), no version number, no flag. `herd-proto`
untouched.

## Not done, and why

- **`535691 kB` is NOT fixed, deliberately, and the cause is named
  below.** The formatter is shared — `herd_proto::size::show_bytes`, 33
  call sites across three binaries — so per the mid-task instruction
  ("if the formatter is shared and fixing it reaches beyond `sweep.rs`,
  stop and tell me") this task stopped at the diagnosis. See *Risks and
  findings*.
- No board row was written: `board.md` is not in this task's paths. The
  row is proposed below for whoever owns the board.
- No `D-NEXT` entry: the brief asked for none, and nothing here changes a
  decision. The behaviour change is "a refusal says why", which is
  invariant 9 and D161 applied, not a new choice.

## Gates

Full bare `./scripts/gate.sh` on `05fbccb` — the code commit, the whole
of the Rust change — **GREEN**, verbatim:

```
gate: GREEN sha=05fbccb9fabb2d07a1774d77dd117c9e2711161b steps=0-8:ok host=servernixos load1=2.07 load1-end=2.36 skips=at-rest
```

Steps, same run: `step 1 check-docs.sh — ok (1s)`, `step 2
check-orchestration.sh — ok (24s)`, `step 3 nix flake check — ok (1s)`,
`step 4 cargo fmt — ok (2s)`, `step 5 cargo clippy — ok (8s)`, `step 6
test suite — ok (192s)`, `step 7 the skips — ok`,
`the skip list matches the expectation exactly.`

`check-orchestration.sh`, measured on this branch: **`check-orchestration:
269 passed, 0 failed`** — exactly `main`'s baseline. The unlanded branch
that moves it to 285 is not in this tree.

Receipt, skip list and log of that run, kept outside the worktree. `$S`
below is this session's scratchpad root, named in full once:

```
S=/mnt/ssd_980/herd/claude/tmp/claude-1002/\
  -mnt-ssd-980-herd-project/\
  3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad

$S/sweep-names-why-gate2.log                       the GREEN run
$S/sweep-names-why-receipts/\
  05fbccb9fabb2d07a1774d77dd117c9e2711161b.receipt the receipt
$S/sweep-names-why-receipts/skips-05fbccb9fabb.txt the 12 skip lines
$S/sweep-names-why-receipts/\
  test-output-05fbccb9fabb.txt                     the suite's output
$S/sweep-names-why-gate.log                        the RED first run
```

A first run, `046b448`, was **RED** on `4:cargo-fmt` and is reported red:
`gate: RED sha=046b448929b127749808602c6ca5517708573eed failed=4:cargo-fmt
skips=at-rest`. One `assert!` line rustfmt wanted split; `cargo fmt --all`
fixed it, the commit was amended to `05fbccb`, and the gate above is that
tree.

**The last run**, on the tree that includes this file, is
`./scripts/gate.sh` run bare with `--out $S/sweep-names-why-gate3.log`,
its receipt under this worktree's `target/herd-gate/`. Its verbatim
verdict is in the agent's report, not in this file.

**Skips, by name — 12, the at-rest list, matching
`scripts/expected-skips/local.txt` exactly, and every one of them a
cgroup-delegation or io-ceiling skip this host cannot make:**
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
`load-excused-skips: none`. **No test of this change skipped**: this host
runs as uid 1002, so both mode-based cases ran and their assertions were
made.

## Risks and findings

**1. `535691 kB` — cause named, not patched. A divisibility branch, not a
threshold.** The offer line calls `herd_proto::size::show_bytes`
(`herd-proto/src/size.rs:70`), re-exported by `herd_runner::size`. It
walks `[GiB, GB, MiB, MB, KiB, kB]` and returns the **first unit the
figure is an exact multiple of**, falling through to raw bytes only when
it is a multiple of none. Measured against the consumer's figure
`535691000`:

| unit | scale | `>= scale` | remainder |
|---|---|---|---|
| GiB | 1073741824 | no | — |
| GB | 1000000000 | no | — |
| MiB | 1048576 | yes | 917240 |
| MB | 1000000 | yes | 691000 |
| KiB | 1024 | yes | 760 |
| kB | 1000 | yes | **0** |

So `535691 kB`, with no exact byte count, by the formatter's design. One
job in 75 is not a threshold and not a typo: it is the one job whose
total bytes happened to be a round multiple of 1000 and of nothing
larger. **Every measured figure in the runner inherits it** — the same
`show_bytes` prints `sweep.rs`'s offer (`:151`) and foreign-state-root
(`:166`) lines and `outputs.rs:251`/`:347`/`:348` — and any of them will
print `kB`, `MB` or `GB` the day its figure lands on a round decimal.

**And there are two formatters, and this line uses the one meant for
something else.** `show_bytes` exists so that a cap an operator *typed*
reads back in the family they typed it in — `200GB` → `200 GB` (D148,
D152), which is a deliberate property with its own test
(`a_decimal_size_reads_back_in_the_family_it_was_written_in`). The
project's *measured*-figure convention lives in the hub:
`herd-hub/src/report.rs:514` `bytes_said` and
`herd-hub/src/retention.rs:801` `said_bytes`, identical bodies,
deliberately duplicated — exact count **always**, binary approximation
beside it once the exact figure stops being readable, `1136 bytes (1.11
KiB)`. A measured spool weight is that kind of figure: nobody typed it,
and its decimal family means nothing.

So the defect is **a cap formatter used for a measurement**, and the fix
is a choice between (a) giving the runner the hub's `bytes_said` rule for
measured figures and leaving `show_bytes` to caps — which changes every
runner line that prints a weight, and wants golden-text assertions — and
(b) changing `show_bytes` itself, which breaks the `200GB` read-back
D148/D152 chose and touches 33 call sites in three binaries including
`--help` defaults (`herd-hub/src/main.rs:164`). **Recommendation: (a), as
its own task with its own gate run**, and it should also fold the hub's
two identical copies into one place. Not done here, and `herd-proto` was
not touched.

**2. An unreadable *file* does not make a spool not-whole, and cannot.**
The walk never opens a file — one `lstat` per entry is the bound that
makes a 65 GB sweep as cheap as an empty one. A regular file moded 0o000
still `lstat`s fine, so it reads as a whole collection here and the hub
discovers the problem when the bytes are streamed. What actually stops a
walk is a **directory** it may not read or traverse. That is consistent
with the consumer's hypothesis (a pack tree a build job left owned by
another user is a directory), and it is why the tests use directories.
The brief's phrase "a spool with an unreadable file inside it" is
therefore not reachable as written; this is written into the module doc
so the next reader does not re-derive it.

**3. Their 50 refusals are almost certainly 50 `EACCES` on pack
directories, and this change will say so at their next start.** Nothing
here proves it — no figure of theirs was read, and nobody logged into
anything. The line is what will prove it.

**4. The argument is now measured, not asserted.** The same run offered 75
collections the hub already held whole, 37.6 GiB agreeing on both sides
(their report). The sweep's sentences are the only window an operator has
onto those decisions. That figure is in the module doc.

**5. Small risk, named:** the refusal line is now long, and names the
spool path twice — once as the spool, once inside the first unreadable
path. Deliberate: an operator pastes an absolute path into a shell. If a
reviewer prefers the relative path inside the spool, that is a one-line
change in `NotWhole::sentence`.

## Decisions needed

**One, and it is the one the mid-task instruction asked me to stop at.**

- *Question:* which fix for the byte-unit defect — give the runner's
  measured figures the hub's `bytes_said` rule (exact count always, binary
  approximation beside it), or change the shared `show_bytes`?
- *Recommendation:* the first, as **its own task with its own gate run and
  golden-text assertions**, scoped to the runner's measured call sites
  (`sweep.rs:151`, `:166`, `outputs.rs:251`, `:347`, `:348`), leaving
  `show_bytes` to the caps it was written for. Fold `report.rs:514` and
  `retention.rs:801` into one function while there.
- *Fallback:* do nothing. `535691 kB` is the right number in a surprising
  unit, not a wrong number, and the consumer can act on it. It is not a
  reason to hold this change.

## Resume point

`herd-merger` takes `sweep-names-why` at the handoff commit into the next
batch; the GREEN receipt for `05fbccb` and the one for the handoff commit
are both named under *Gates*. Nothing else on this branch.

Then, as a separate task: the byte-unit fix above, brief written from
*Risks and findings* §1.

**Proposed board row, `## Owed`:**

```
| ORCH-NEXT | **A measured byte figure in the runner prints in a decimal unit with no exact count.** The first consumer's v0.4.1 sweep printed `535691 kB` for `j0000000740` where the other 74 jobs printed bytes. Cause: `herd_proto::size::show_bytes` returns the first unit the figure divides exactly — a divisibility branch, not a threshold — and it exists for caps an operator *typed* (`200GB` → `200 GB`, D148/D152), not for measurements. The project's measured-figure convention is the hub's `bytes_said`: exact count always, binary approximation beside it | consumer report 2026-10-29 (v0.4.1, job `j0000000740`, hub figure 535691000 bytes claimed and measured); diagnosed by `sweep-names-why`, 2026-10-29 | herd-step | owed | Give the runner's measured call sites the hub's rule and leave `show_bytes` to caps; fold `report.rs:514` and `retention.rs:801` into one. Wants golden-text assertions and its own gate run — 33 `show_bytes` call sites in three binaries, and a `--help` default among them | none | `herd-proto/src/size.rs:70`; `herd-runner/src/sweep.rs:151`, `:166`; `herd-runner/src/outputs.rs:251`, `:347`, `:348`; `herd-hub/src/report.rs:514`; `herd-hub/src/retention.rs:801`; `herd-hub/src/main.rs:164`; divisibility table in `docs/orchestration/history/2026-10-29-sweep-names-why.md` |
```

## Processes

No long-running process was started: no hub, no runner, no daemon. The
only processes were `cargo`, `rustfmt`, `nix` and the suite's own
children, all foreground, all exited. Nothing was killed; **PID 3046930,
the stray `herd-hub` from an earlier task, was left exactly as it is**,
and no `pkill`, `killall` or kill by name was used.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/sweep-names-why`, off the root
filesystem, and the gate's own `target/herd-gate/` inside the worktree:
both deleted once the branch was pushed, after the receipts above were
copied out to the scratchpad.
