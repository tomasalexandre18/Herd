# bytes-written-measured — handoff 2026-10-30

- **Item:** ORCH-268 (its `bytes_written` half, measured)
- **Agent:** herd-step
- **Branch / commit:** `bytes-written-measured` / see the agent's report,
  based on origin/main `8af300e`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

One gotcha,
`docs/orchestration/gotchas/bytes-written-is-the-workspace-not-the-job.md`,
with its index row in that directory's `README.md`. It carries the
mechanism, the reconciliation of all four field figures, and the table
of eight probe jobs. No product file is touched: this task measures and
writes down, and changes no behaviour.

**What `bytes_written` measures, from the code.**
`Workspace::measure()` (`herd-runner/src/root.rs:443`) calls
`measure_tree(&self.base, &self.root)` (`:498`). The path is the job's
own workspace base, `<data-root>/jobs/<job-id>/`, holding `work/` (the
cwd), `home/` (`HOME` and every `XDG_*`), `tmp/` (`TMPDIR`, `TMP`,
`TEMP`, `XDG_RUNTIME_DIR`), `in/`, `out/` and `ran`, and nothing else.
It is a **full recursive walk** summing `metadata.len()` — apparent
size, no symlink followed — taken on a blocking thread every
`POLL_INTERVAL` = **200 ms** (`confine.rs:106`; the walk at
`job.rs:1489`-`:1510`), plus one final walk after the job's processes
are killed (`job.rs:574`). `bytes_written = max(peak_disk, final_disk)`
(`job.rs:622`-`:624`).

There is no job without a workspace: `open_workspace` always makes the
base. What an **ad-hoc** command lacks is `in/` and `out/`, which
`open_exchange` makes only for a declared command. So the walk always
has a path — it is simply a path that an ad-hoc job's real work is not
in, because `cd <somewhere> && …` leaves it.

**The three field figures are reconciled, and none of them is the
build.**

- **89 MB** (`j0000000043`, `j0000000044`) is the **cargo registry**,
  re-downloaded into a throwaway `$HOME` every job because `HOME` is
  inside the workspace and `CARGO_HOME` is never set. Proved to the
  byte: my `j0000000061` recorded `194282312` and its measured `$HOME`
  was `194282312` — `.cargo/registry` `190801624` plus `.cache/nix`
  `3415152`, 6,491 files. My `j0000000060` recorded `89586215` against
  a `$HOME` of `89342924`, which is the field figure.
- **8.08 GiB** and **21.08 GiB** (`j0000000036`, `j0000000040`) are a
  200 ms poll landing inside the lifetime of the suite's **sparse**
  fixtures. `herd-runner/src/packstore.rs` `plant_pack` (`:3682`)
  creates pack objects with `File::set_len` at 1, 2, 4, 6 and 15 GiB,
  and `testing::TempDir` (`testing.rs:141`) puts them under
  `std::env::temp_dir()` — which is `TMPDIR`, which is inside the
  measured base.

**So nobody ever wrote 21 GB.** Running that fixture subset alone, a
0.5 s sampler inside `j0000000060` caught the measured base at
**21,564,710,785 apparent bytes against 109,563,904 real** — 20.08 GiB
of nothing, about 104 MiB of disk — in a window lasting **0.11 s**, and
the runner's own 200 ms poll **missed it** and recorded 89 MB.
`j0000000040` had the opposite luck and a landing's gate died of it.
The 250× span is not a property of the work; it is whether a poll
landed in that window.

**The cap is evadable, and the hypothesis about `bench-preflight.sh` is
refuted.** 300 MiB written into the job's cwd is counted exactly; the
same 300 MiB written into the worktree the job `cd`'d to, or into
`$HERD_BUILD_ROOT`, is counted as **zero**. The two paths are measured
identically — not at all — so `scripts/bench-preflight.sh:320` is
**cap-neutral**: it moves bytes from one unmeasured path to another. It
guards the root filesystem, which is its job, and it needs no row on
this account. The cap binds only a **declared** command, whose tree is
checked out into `work/`: it works on the product's own path and is
void on the path this project uses Herd through.

## Not done, and why

- **No product change, by instruction.** The fix is small and is
  drafted below as a recommendation. A bound that starts firing
  correctly turns hosts that run jobs today into hosts that stop them,
  and a bound that stops firing on sparse files is still a change to
  what the product promises under invariant 2. Both are Alexa's.
- **The full suite was not run under instrumentation.** The
  `packstore::` subset was enough to produce the 20 GiB apparent peak
  and so to explain the field figures; a full instrumented suite run
  would only raise the peak and would cost a slot against a 20 GiB
  budget it is a lottery against.
- **`peak_memory_bytes` is noted, not investigated.** It read
  **3,117,056** — 3 MB, the top-level `bash` — for a job that ran a
  cold cargo build, because with no cgroup `process_rss_bytes` reads
  one pid and not its children. Same file, same shape, ORCH-268's other
  half, and not this task's scope.

## Gates

Prose only: no file outside `docs/` is touched, so the four Rust gates
were **NOT RUN** locally, covered by the batch's own `./scripts/gate.sh`
at integration, which is `herd-merger`'s run and not this agent's.

Run before this file was committed, on the gotcha and the index row:

    $ ./scripts/check-docs.sh
    docs ok

The last run is `./scripts/check-docs.sh` on the committed tree,
including this handoff; its output is in the agent's report and not in
the tree it gates.

No skip: `check-docs.sh` prints none, and no `HERD-SKIPPED` line was
produced by anything this task ran. The standing twelve and the
`#[ignore]` census at 38 belong to the Rust suite, which was not run
here.

## Risks and findings

1. **A capped job has no verdict and writes no receipt.** That is what
   made batch 22's gate say neither yes nor no, and it is unchanged. The
   cap that produced it fires on luck, so the same silence can happen
   to any gate submitted as one job through our runner. Until the
   figure or the budget changes, **a gate job's disk cap is a lottery**,
   and the right response to one is to report it (as the brief already
   requires) and never to resubmit hoping for a kinder sample.
2. **The remedy the project has been using was never tested against
   the thing it was meant to remedy.** Splitting a gate into one job
   per gate and redirecting `CARGO_TARGET_DIR` were confounded in every
   successful run, and neither ever touched the measured path.
   `j0000000036` settles it: **one undivided `gate.sh` with the target
   redirected still recorded 8.08 GiB**, because the figure is about
   `$HOME` and `$TMPDIR` and not about `target/`. What kept
   `j0000000043` and `j0000000044` at 89 MB is that they ran only
   `cargo build` and so never reached the sparse fixtures. The
   inheritable rule is therefore **not** "split" and **not**
   "redirect": it is *do not run the hub's and the runner's own unit
   tests inside a workspace whose apparent size is the cap*. This is
   recorded so the next orchestrator does not inherit a remedy that
   was never measured.
3. **A job cannot be given an environment variable.** `job_environment`
   (`confine.rs:772`) carries only `PATH`, `LANG`, `LC_ALL`, `TZ` and
   `TERM` past `env_clear`, so a requester cannot set `CARGO_HOME`, and
   every cargo job on every runner re-downloads its registry into a
   `$HOME` that is deleted when the job ends. That is the entire
   content of the disk figure for a build job, and it is bandwidth and
   wall time paid once per job. Named as a finding; it carries a
   product question and is not this task's to answer.
4. **The walk's own cost.** The measured base held 6,491 files after a
   `cargo fetch` and is walked five times a second for the job's whole
   life. A three-hour gate is on the order of 54,000 full-tree stat
   walks. Not measured here; flagged because it is the same code path
   and `walk_cost.rs` already exists for questions of this shape.

## Drafted for `herd-merger`

The `docs/decisions.md` entry, in full:

```markdown
## 2026-10-30 — D-NEXT The disk cap measures a workspace, not a job

`bytes_written` in a job record, and the disk cap enforced against it,
are the **apparent size of the job's workspace, sampled**: a recursive
walk of `<data-root>/jobs/<job-id>/` summing `metadata.len()`, taken
every 200 ms and once more at the end, reported as
`max(peak, final)` (`herd-runner/src/root.rs:443`, `:498`;
`confine.rs:106`; `job.rs:574`, `:622`-`:624`). Measured 2026-10-30
with eight jobs through our own instance, each differing in one path.

**Three consequences, each proved by a probe rather than argued.**

1. **It is not bytes written.** 300 MiB written into the job's cwd is
   counted exactly (`j0000000055`, `314572800`). The same 300 MiB
   written into the worktree an ad-hoc command `cd`s to
   (`j0000000056`) or into `$HERD_BUILD_ROOT` (`j0000000057`) is
   counted as **zero**. Every job this project submits is ad-hoc and
   leaves its workspace, so for every one of them the build is not in
   the figure. The cap binds a **declared** command, whose tree is
   checked out into `work/`, and is void on the path we use Herd
   through.
2. **A sparse file trips the cap having written nothing.**
   `truncate -s 8G` inside the workspace recorded `8589934592` against
   **16,384 bytes** of disk occupied (`j0000000059`).
3. **The cap fires on a sample, so it fires on luck.** The runner's own
   test suite plants sparse pack objects of 1 to 15 GiB under `TMPDIR`,
   which is inside the measured base (`packstore.rs:3682`,
   `testing.rs:141`). A sampler inside `j0000000060` caught the base at
   **21,564,710,785 apparent bytes against 109,563,904 real** in a
   window of **0.11 s**, which the 200 ms poll missed; that job
   recorded 89 MB. `j0000000040` — batch 22's gate — caught it, was
   stopped at `limit 21474836480`, and so had **no verdict and no
   receipt**. Nobody wrote 21 GB. About 104 MiB of disk was occupied.

**What the field figures were.** The 89 MB of `j0000000043` and
`j0000000044`, against trees their agents measured at 16 G by `du -sh`,
is the **cargo registry** re-downloaded into a throwaway `$HOME`:
`j0000000061` recorded `194282312` and its `$HOME` measured
`194282312`, being `.cargo/registry` `190801624` plus `.cache/nix`
`3415152`. The 8.08 GiB of `j0000000036` and the 21.08 GiB of
`j0000000040` are the sparse fixtures. The 250× span measured nothing
about the work.

**What this entry decides: nothing about the bound.** It records what
the figure is, so that it stops being read as what its name says. Two
beliefs are **withdrawn**, both measured false: that redirecting
`CARGO_TARGET_DIR` lowers a job's exposure to the cap, and that
splitting a gate into one job per gate does. Neither touches the
measured path, and `j0000000036` — one undivided `gate.sh`, target
redirected, 8.08 GiB — is the counter-example to both.
`scripts/bench-preflight.sh:320` is **cap-neutral** and keeps its
sentence: it moves bytes from one unmeasured path to another, and the
root filesystem it guards is a different bound.

**The recommendation, not landed.** "Bytes written" is what the cgroup
already knows: `io.stat`'s `wbytes`, summed over the job's cgroup,
counts writes wherever they land and cannot be inflated by a file that
writes nothing. That makes the figure truthful and the cap unevadable
in one change, and it is absent where there is no cgroup, which is the
shape D23 and D30 already give an unmeasurable figure. If the walk is
kept instead, the two things it conflates must be separated and named
apart — a **workspace quota** in apparent size, which is a legitimate
bound, and **bytes written**, which is I/O — and a single 200 ms sample
must stop being enough to kill a job. **Not landed here**: a bound that
starts firing correctly turns hosts that run jobs today into hosts that
stop them, and the inverse is live now, because the cap currently kills
jobs that wrote nothing.
```

The `board.md` rows, in full. One for *Waiting*:

```markdown
| ORCH-NEXT | **What should `bytes_written` measure, and what should the disk cap bound?** Measured 2026-10-30: the figure is the apparent size of the job's workspace, sampled every 200 ms, so it excludes an ad-hoc job's build entirely, counts a sparse file at its declared length, and kills on a single sample. 300 MiB into the cwd counts exactly; the same 300 MiB into the worktree or into `$HERD_BUILD_ROOT` counts as zero. A sparse 8 GiB file recorded 8589934592 against 16,384 bytes of disk. Batch 22's gate was stopped at a 20 GiB cap by sparse test fixtures occupying about 104 MiB, and a capped job has no verdict and no receipt, so a landing's gate said neither yes nor no | `bytes-written-measured`, 2026-10-30, eight probe jobs through our own instance; ORCH-268's `bytes_written` half | Alexa | waiting | **Measure the job's I/O, not a tree:** `io.stat` `wbytes` summed over the job's cgroup counts writes wherever they land and cannot be inflated by a file that writes nothing, which makes the figure truthful and the cap unevadable in one change, absent without a cgroup in the shape D23 and D30 already give. It is Alexa's because it is what a job record asserts and what invariant 2 promises, and because a bound that starts firing correctly turns hosts that run jobs today into hosts that stop them | Keep the walk and separate the two things it conflates: a **workspace quota** in apparent size, which is a legitimate bound, and **bytes written**, which is I/O; and stop letting one 200 ms sample kill a job. Cheaper, and it leaves the product unable to say what a job wrote | `herd-runner/src/root.rs:443`, `:498`; `herd-runner/src/confine.rs:106`; `herd-runner/src/job.rs:574`, `:622`-`:624`; `docs/orchestration/gotchas/bytes-written-is-the-workspace-not-the-job.md`; `history/2026-10-30-bytes-written-measured.md`; ORCH-268; **D23**, **D30**, invariant 2 |
```

And one for *Owed*, which carries no product question — it labels the
figure rather than changing any bound:

```markdown
| ORCH-NEXT | **A job record asserts `bytes_written` without saying which path it covers, and the runner's own stderr already says *workspace*.** The record's field name says the job's writes; `Workspace::measure` and the stderr sentence *"its workspace could not be measured"* say a path. Measured 2026-10-30: for an ad-hoc job the two differ by everything the job built. Whatever Alexa decides about the bound, the record must stop asserting one as the other | `bytes-written-measured`, 2026-10-30; ORCH-268's D27 shape, one level up | herd-step | owed | Name the figure for what it covers, in `herd-proto`'s job record and in what `herdctl` prints, and test it against the known-bad case this task measured: a job that writes a known quantity outside its workspace must not read as a job that wrote nothing. **No bound changes and no cap moves** — this is the labelling half, and it does not wait on the *Waiting* row above | — | `herd-proto/src/lib.rs:1135`; `herd-runner/src/root.rs:443`; `herd-runner/src/job.rs:622`-`:624`; `docs/orchestration/gotchas/bytes-written-is-the-workspace-not-the-job.md`; `history/2026-10-30-bytes-written-measured.md`; ORCH-268; **D27**, invariant 5 |
```

ORCH-268's own cell should gain the reference to this measurement, since
its `bytes_written` half is now answered: the *about 11 GB* in that row
is a tree the job wrote **outside** its workspace, and the 287 MB it
records is the workspace. Its `peak_memory_bytes` half stands untouched.

## Decisions needed

**Raise our queue runner's `--job-disk-budget`, or stop submitting
`./scripts/test.sh` through it?** Not the product question above — an
operational one about our own instance, and the orchestrator's to take.
Today's `--job-disk-budget 20GiB` is enforced against a figure whose
peak is the suite's sparse fixtures, which that subset alone drove to
20.08 GiB. Recommendation: raise it well clear of the fixtures' apparent
total for as long as the figure means what it means, because the real
disk a suite job occupies is about 104 MiB and the bound is protecting
nothing. Fallback: keep the budget and accept that a gate job can lose
its verdict to a sample, which is what happened to batch 22.

## Resume point

Nothing is in flight. The next task is whichever of the two drafted rows
is taken up: the *Waiting* row needs Alexa, the *Owed* row does not and
can start at `herd-proto/src/lib.rs:1135` with the known-bad case this
task already measured — a job that writes a known quantity outside its
workspace must not read as a job that wrote nothing.

## Processes

PIDs started: none directly. **Eight** jobs were submitted through
`/mnt/ssd_980/herd/queue/bin/herdctl` — `j0000000054` through
`j0000000061`, inclusive, and nothing below `j0000000054` is this
task's — each run by the queue runner, all exited 0, none capped, none
re-run. No process was signalled, and nothing was killed.

`/mnt/ssd_980/herd/build/` is **empty** as this task ends. The only
removal this task ran was `rm -rf` on its own named subdirectory,
`/mnt/ssd_980/herd/build/bytes-written-measured`. Another agent's
`m12-p4-runner-custody` tree was named in this task's brief as live and
is no longer there; this task never listed that directory before
deleting its own path, so it cannot show the tree was already gone, and
says so rather than assuming it.

Own build tree deleted once the branch was pushed:
`/mnt/ssd_980/herd/build/bytes-written-measured`, **1.3 G** — dev
profile, `herd-runner` lib tests, `packstore::` subset only — of which
`target/` 953 M and the 301 M of probe blobs the path probes wrote. The
300 MiB blob `j0000000056` wrote into `probe-target/` inside the
worktree was removed with it.
