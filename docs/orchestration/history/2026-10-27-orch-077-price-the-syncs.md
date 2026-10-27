# orch-077-price-the-syncs — handoff 2026-10-27

- **Item:** ORCH-077
- **Agent:** herd-step
- **Branch / commit:** `orch-077-price-the-syncs` / `69a34de`, based on
  `origin/main` `2d177eb`
- **Worktree:** `…/scratchpad/orch-077/wt` (scratchpad
  `…/scratchpad/orch-077/work`); nothing written in
  `/mnt/ssd_980/herd-project`
- **PR:** [#90](https://github.com/tomasalexandre18/Herd/pull/90)

## Done

`herd-hub/tests/sync_prices.rs` (new, 3 `#[ignore]`d cases) and
`docs/remediation/11-sync-prices.md` (new), with a row and three
corrections in `docs/remediation/README.md`.

The three durable operations of `record_waiting` are named **R** (the
`sync_all` on the record's temporary), **D** (the `sync_dir` after the
rename) and **E** (the `sync_data` on the appended event line). All eight
subsets of `{R, D, E}` are timed over a replica of `record_waiting`;
every-sync-on is the control and must land on the store's own figure.
Three phases: each sync call alone with the preceding write outside the
timer, the eight-subset lattice, and a whole pass at `N` = 1 000
comparable with `08`'s junction. Both filesystem classes in one session on
one host, selected by `TMPDIR`; each phase prints the mount it landed on.
Nothing under `herd-hub/src` is touched.

### The three figures, per filesystem

Each sync call alone, median (phase 1):

| sync | ext4 on NVMe | ZFS on the SATA mirror |
|---|---|---|
| **R** record `fsync` | **989.1 µs** (iqr/med 7.3 %) | **1.65 ms** (102.9 %) |
| **D** directory `fsync`, after R | **991.1 µs** (3.9 %) | **1.32 ms** (117.4 %) |
| **E** event `fdatasync` | **983.4 µs** (3.3 %) | **1.28 ms** (4.5 %) |

Marginal price of each sync inside a whole waiting write (phase 2),
measured both added to nothing and added last to the other two:

| sync | ext4 alone | ext4 added last | ZFS alone | ZFS added last |
|---|---|---|---|---|
| **R** | 1.05 ms | 1.00 ms | 1.39 ms | 974 µs |
| **D** | 1.04 ms | 930 µs | 1.48 ms | 1.22 ms |
| **E** | 963 µs | 907 µs | 1.59 ms | 1.32 ms |

One whole waiting write: ext4 `---` 51.2 µs, `R--` 1.10 ms, `RDE`
3.00 ms, store 3.11 ms. ZFS `---` 76.5 µs, `R--` 1.46 ms, `RDE` 3.97 ms,
store 4.14 ms. **The control holds in both cells** (3.00 against 3.11;
3.97 against 4.14), so the replica is the same experiment as the hub.

A whole pass at `N` = 1 000 (phase 3):

| cell | `RDE` today | `R--` keep one | `---` no sync |
|---|---|---|---|
| ext4 on NVMe | **3.128 s** | **1.085 s** | 52.85 ms |
| ZFS/SATA | **6.949 s** | **2.150 s** | 78.61 ms |

### H-1 does not hold

H-1 as the brief states it — on NVMe one `fsync` is tens-to-low-hundreds
of microseconds rather than the 1.19 ms `08` derived on ZFS/SATA, so
keeping one sync may collect nearly the whole prize — is **refuted on both
halves**:

1. One `fsync` on ext4/NVMe is **989 µs**, five to fifty times above the
   hoped range, and only **1.3x** better than the ZFS/SATA figure it was
   contrasted with. The class the deployment runs on is not an order of
   magnitude faster per sync; it is a third faster.
2. The three syncs are **additive, not redundant**. On ext4 the three
   marginals sum to 3.05 ms against a measured `RDE` of 3.00 ms, and the
   "alone" and "added last" columns agree within 11 %. So keeping one and
   dropping two collects **64.5 % on ext4 and 64.3 % on ZFS** — two syncs
   out of three, not "nearly the whole prize". The kept sync still costs
   1.10 ms, **21x the 51 µs unsynced floor**, and is 35 % of what the
   write costs today.

This does not argue against the increment: a third off is a real third
off and it keeps an integrity guarantee the unsynced path lacks. It argues
against pricing it as nearly free.

### `08`'s junction figure partly survives

`08`'s 9.970 s at `N` = 1 000 was ZFS/SATA. The same pass on ext4 is
**3.128 s** — **3.2x smaller, not an order of magnitude**. So the plan is
right to stop quoting 9.97 s for the deployment, and the correction is 3x
rather than 10x; and the problem `08` identified **does not go away on
ext4**, where three seconds of durable writing still dominates a pass at
`N` = 1 000.

## Not done, and why

- **`docs/orchestration/board.md` is untouched**, as the brief directs.
  The rows are below, verbatim, for the merger.
- **No `Dxxx` allocated**, and `docs/decisions.md` is untouched.
- **ORCH-074's actual question is not answered.** Whether `events.jsonl`
  can end up ahead of `job.json` on ext4 after a power cut needs a crash
  test (`dm-log-writes`), not a stopwatch. This piece prices the syncs and
  says nothing about which of them the ordering needs.
- **No `10-ordering-on-ext4.md`.** It does not exist (see Risks), so
  nothing was written to it or against it.

## Gates

All four Rust gates plus both doc scripts, since the diff adds a test
file. `CARGO_TARGET_DIR` inside the agent's scratchpad; one `cargo` at a
time.

`nix develop --command cargo fmt --all --check` — clean, exit 0 (after
`cargo fmt --all` applied four rustfmt diffs in the new file).

`nix develop --command cargo clippy --workspace --all-targets -- -D
warnings` — clean:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.44s
```

`nix develop --command cargo build --workspace`:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.84s
```

`./scripts/check-docs.sh` — exit 0:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/check-orchestration.sh` — exit 0:

```
check-orchestration: 111 passed, 0 failed
```

`nix develop --command ./scripts/test.sh` — **RED, exit 101, and not
mine.** One target failed, `-p herd-hub --test slots`:

```
thread 'the_derived_j_is_recorded_and_is_what_the_job_is_given' (2518729) panicked at herd-hub/tests/slots.rs:744:9:
  left: "built with -j3"
 right: "built with -j7"
```

This is **ORCH-069**, unedited. Two things in it are evidence for that
row rather than against it:

- **A fifth round, and it splits the two tests again.** `admission.rs`
  was **green** this round (`test result: ok. 5 passed; 0 failed`), so
  only one of ORCH-069's two assertions failed. The row's "the failure
  mode is not stable even here" holds, and "both reproduce" does not.
- **It confirms the fourth round's correction.** The placement line reads
  `8.4 GiB lent, 1.6 GiB kept as its own floor` and `-j 3 from 8693 MiB
  and 20 threads, 6519 MiB for this slot (3 of 4), 1792 MiB per build
  thread`. The declared 8 GiB **is** honoured here; the `-j` shortfall
  comes out of the per-thread arithmetic, not a lower announcement.

**The new file does not run in the suite:** `running 3 tests` /
`3 ignored` / `0 passed; 0 failed`.

### Skips, with the host named (ORCH-070)

**AT LEAST 12** assertions could not run, **on this workstation**. It is a
**floor and not a count** because the run was red and did not finish —
D179's wording, working as intended:

```
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were
 skipped, and the suite did not finish (cargo test exited 101),
 so this list is a floor and not a count.
```

All 12 are the cgroup-v2-delegation and ZFS-`io.max` family. **CI's
`check` reports 15 on the same tree**; the gate is judged on CI's number,
not this one. No skip is counted as a pass (D27).

## Risks and findings

1. **The plan this task was briefed to serve does not exist.**
   `docs/remediation/10-ordering-on-ext4.md` is on **no ref** — not
   `origin/main` at `2d177eb`, not any remote or local branch — and the
   string `H-1` and `ordering-on-ext4` appear nowhere in the repository.
   `git ls-tree` over every ref returns only `09-runner-self-limits.md`
   for the `09`-`11` range. The brief quotes the plan's hypothesis in
   full, so the measurement was well specified without it and ORCH-074
   and ORCH-053 motivate it independently; the work therefore went ahead.
   But **nothing in the repository records H-1**, so this piece states
   the hypothesis it refutes in its own words. If that plan exists outside
   the repository it should be committed, because `11` now answers it.
2. **`10` is unclaimed, so the gap is deliberate.** The brief's reason for
   skipping it — "`10` is taken by the plan" — does not hold. `11` was
   taken as instructed and `10` left free; the README now says why, under
   its existing convention that a gap means *not written*, never *lost*.
   `09` is written and on an unmerged branch (PR #85), so it has no row
   yet either.
3. **The brief's host description was stale, in a way that mattered.** It
   reported load 0.38 and no `cargo` running. The first reading was
   **1.00**, and the first measurement round was taken at **4.44** — the
   decay of this agent's own release build. The host carries a steady
   service baseline (docker, grafana, cadvisor, media servers) at roughly
   0.6-1.0 with the CPU 96.7 % idle and iowait 0.0 %, so load here is not
   compute contention; but the rounds were re-taken under load 0.73-0.77
   anyway. The ext4 figures agree to within 3 % across load 4.44 and load
   0.77, so that class is insensitive. **ZFS is not**, see 4.
4. **One cell is reported as failed, not as a figure.** The first ZFS
   round, 60 samples per subset at load 3.35, put `R--` (4.85 ms)
   *above* `RDE` (4.07 ms) and missed its control badly — replica 4.07 ms
   against the store's 10.58 ms. Sixty samples do not survive ZFS's
   transaction-group bimodality. Re-run at 400 samples under load 0.73,
   the control holds to 4 %. `HERD_SAMPLES` exists for this reason, and
   **a ZFS figure from this harness at the default sample count should
   not be trusted**.
5. **A correction to `06`, which `08` noticed and did not diagnose.**
   `06` measured `sync_dir` at **6.6 µs** and predicted 2.39 ms for the
   record half; `08` measured 2.36 ms against a predicted 1.20 ms and
   recorded the gap without explaining it. The gap is **D**. `06`'s
   6.6 µs is a directory sync with nothing owed under it, while the
   `sync_dir` inside `write_file_with` always follows a `rename` and so
   always commits. In the position the code puts it in, D is **1.32 ms on
   ZFS and 991 µs on ext4** — about **two hundred times** `06`'s figure.
   Any arithmetic treating the directory sync as free is wrong by about a
   third of a waiting write.
6. **Nothing run here proves the device honours a flush.** What can be
   said: the ext4 mount carries **no `barrier=0`** — options are
   `rw,relatime,stripe=32` verbatim from `/proc/mounts`, and ext4's
   default is `barrier=1` — and the filesystem has `has_journal`; ZFS is
   at `sync=standard`. All three devices report `write back` in
   `/sys/block/*/queue/write_cache`, so every figure depends on a `FLUSH`
   being obeyed; the NVMe reports `fua 1`, the two SATA SSDs `fua 0`.
   **`nvme-cli` is not installed here**, so the drive's volatile write
   cache could not be queried directly and no available tool can show the
   controller obeys a flush rather than acknowledging it. The weaker
   evidence that stands in is the shape of the numbers: 989 µs for a
   1 KiB `fsync` on an NVMe drive is only explicable as a device round
   trip, and the same write without the flush costs 51 µs — a 19x gap a
   lying drive would have collapsed. Consistent with honest flushes; not
   a proof.
7. **These figures size a class, not the consumer's machine.** Their
   state root is ext4 on NVMe, which is this cell's class, but a
   per-flush cost is a property of the drive. This is a DRAM-less
   consumer Samsung 980; an enterprise NVMe with power-loss protection
   can be ten times faster per flush, a worse consumer drive slower.
8. **Everything is single-threaded** (`--test-threads=1`) and only
   `record_waiting` was priced. What these syncs cost when two runners
   junction at once is `06`'s lock question, not reopened. The hub's
   other durable writes use the same primitives and so presumably the
   same per-flush cost, but that is an inference.
9. **The local `main` in `/mnt/ssd_980/herd-project` is 80 commits behind
   `origin/main`** (at `860c4f5`). Read against that checkout, every
   artefact this brief names looks absent. Worth a `git fetch` before the
   next brief is written from it.

## Decisions needed

**None from this agent.** One question is sharpened rather than opened,
and it is already ORCH-074's: the increment "keep R, drop D and E" is now
priced at a third off rather than nearly free, so whether it is worth the
crash test ORCH-074 requires is Alexa's call on a 65 %-of-prize figure
instead of a 99 % one. Recommendation: put the figure in front of her with
ORCH-074's cost beside it; fallback: keep every write durable and accept
3.128 s per 1 000-job pass on the deployment's class.

## Board rows, verbatim, for the merger

For **Done**, or wherever ORCH-077 is filed — the row does not exist on
the board at `2d177eb` (highest id present: ORCH-074):

```
| ORCH-077 | Price the three `fsync`s of `record_waiting` separately, on both filesystem classes, because every durable-write figure in the dossier was taken on ZFS/SATA while the deployment runs on ext4/NVMe (ORCH-053) | ORCH-074, 2026-10-27 | herd-step (author), herd-merger (integration) | done | Done 2026-10-27 in `11-sync-prices.md`. **H-1 is refuted on both halves:** one `fsync` on ext4/NVMe is **989 µs**, not the tens-to-low-hundreds of microseconds hoped for and only **1.3x** better than `08`'s ZFS/SATA figure; and the three syncs are **additive, not redundant** — the three marginals sum to 3.05 ms against a measured 3.00 ms — so keeping one and dropping two collects **65 %** of the prize, not nearly all of it, the kept sync still costing 21x the 51 µs unsynced floor. **`08`'s 9.970 s junction at `N` = 1 000 becomes 3.128 s on ext4**: 3x smaller, not an order of magnitude, and still seconds, so the problem does not go away on the class that matters. Also corrects `06`'s 6.6 µs `sync_dir`, measured on a directory with nothing owed under it, by about two hundred times | — | `docs/remediation/11-sync-prices.md`; `herd-hub/tests/sync_prices.rs`; `history/2026-10-27-orch-077-price-the-syncs.md` |
```

For **Open** — the missing plan, which is a finding this task could not
act on:

```
| ORCH-078 | `docs/remediation/10-ordering-on-ext4.md` and its hypothesis **H-1** exist on **no ref** — not `origin/main` `2d177eb`, not any remote or local branch — yet two briefs have been written from that plan as if it were in the repository, and ORCH-077 was briefed as "the task this plan gates". `git grep` over every ref finds neither `ordering-on-ext4` nor `H-1`; `git ls-tree` finds only `09-runner-self-limits.md` in the `09`-`11` range. ORCH-077 went ahead because the brief quoted H-1 in full and ORCH-074 motivates the measurement independently, and `11-sync-prices.md` now refutes H-1 in its own words | finding of ORCH-077, 2026-10-27 | herd-planner | open | Either commit the plan, so that what `11` answers is on a ref, or record that no such plan exists and that ORCH-074 is the live question. `10` is left unclaimed in the dossier either way | — | `history/2026-10-27-orch-077-price-the-syncs.md` "Risks and findings" 1 and 2; `docs/remediation/README.md` numbering note |
```

For **ORCH-069**, an evidence line to append rather than a rewrite — the
row's text already predicts this:

```
Fifth round, 2026-10-27 on `orch-077-price-the-syncs` at `69a34de`: only `slots.rs:744` failed (`-j3` against `-j7`); `admission.rs` was **green** (5 passed, 0 failed). The placement line read `8.4 GiB lent, 1.6 GiB kept as its own floor` and `-j 3 from 8693 MiB and 20 threads, 6519 MiB for this slot (3 of 4), 1792 MiB per build thread`, so the declared 8 GiB was honoured in full and the `-j` shortfall came from the per-thread arithmetic — the fourth round's correction reproduces, and "both reproduce on this host" still does not.
```

## Resume point

**PR [#90](https://github.com/tomasalexandre18/Herd/pull/90) is open** against
`main`, filled in from the template with the figures above as Evidence.
Next: wait for CI `check` and `docs` to go green, then `herd-merger`
integrates under ORCH-001, adds the three board entries above and files
ORCH-078. **Do not merge from here.**

To re-take any figure:

```sh
TMPDIR=<dir on ext4> nix develop --command cargo test -p herd-hub \
  --test sync_prices --release -- --ignored --nocapture --test-threads=1
HERD_SAMPLES=400 TMPDIR=<dir on zfs> nix develop --command cargo test \
  -p herd-hub --test sync_prices --release -- --ignored --nocapture \
  --test-threads=1
```

## Processes

No long-lived process started and none killed; no `pkill`, no kill by
name. Everything ran in the foreground under `nix develop` except one
background shell waiting on `/proc/loadavg` (task `bkcmnvnq5`, an
`until`-loop on load, exited 0 on its own). Four `cargo` builds and six
`cargo test` runs, one at a time, `CARGO_TARGET_DIR` in the agent's
scratchpad. The ZFS cells used a per-run directory `/tmp/orch-077-$$`,
removed after each run and verified gone (`no leftovers`). Nothing written
in `/mnt/ssd_980/herd-project`, `/etc/nixos`, `/srv`, any container or any
service.
