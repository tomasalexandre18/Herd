# `bytes_written` is the workspace's apparent size, not the job's writes

- **Date:** 2026-10-30 (ORCH-268's `bytes_written` half, measured).
- **Observed:** batch 22's gate died `stopped_by_cap / disk`, `limit
  21474836480`, `bytes_written 22636083365`, and **a capped job has no
  verdict and writes no receipt**, so a landing's gate said neither yes
  nor no. Three jobs doing the same class of work had recorded figures
  spanning **250×**: `j0000000040` 21.08 GiB (capped), `j0000000036`
  8.08 GiB (green), `j0000000043` and `j0000000044` about 89 MB (green)
  against trees their agents measured at **16 G** by `du -sh`, dev
  profile, `--all-targets`.
- **Why:** `Workspace::measure()` (`herd-runner/src/root.rs:443`) calls
  `measure_tree(&self.base, &self.root)` (`:498`). Three facts follow,
  and together they make the figure mean something other than its name.

  1. **The measured path is the job's own workspace base**,
     `<data-root>/jobs/<job-id>/`: `work/` (the job's cwd), `home/`
     (`HOME`, every `XDG_*`), `tmp/` (`TMPDIR`, `TMP`, `TEMP`,
     `XDG_RUNTIME_DIR`), `in/`, `out/`, `ran`. **Nothing else.** An
     ad-hoc command line that does `cd <somewhere> && …` — which is
     every job this project submits — does its work outside that base,
     and none of what it writes there is seen.
  2. **The sum is apparent size**, `metadata.len()` per regular file,
     following no symlink. A sparse file counts at its declared length
     and costs no disk at all.
  3. **It is a sampled high-water mark**: a full recursive walk on a
     blocking thread every `POLL_INTERVAL` = **200 ms**
     (`herd-runner/src/confine.rs:106`, the walk at
     `job.rs:1489`-`:1510`), plus one final walk after the job's
     processes are killed (`job.rs:574`). `bytes_written =
     max(peak_disk, final_disk)` (`job.rs:622`-`:624`).

  So the figure every one of our gate jobs recorded was its **cargo
  registry**, re-downloaded into a throwaway `$HOME` because `HOME` is
  inside the workspace and `CARGO_HOME` is never set — plus whatever
  sparse test fixture a 200 ms poll happened to land on.
  `herd-runner/src/packstore.rs` `plant_pack` (`:3682`) creates pack
  objects with `File::set_len` at 1, 2, 4, 6 and 15 GiB, and
  `testing::TempDir` (`testing.rs:141`) puts them under
  `std::env::temp_dir()`, which is `TMPDIR`, which is inside the
  measured base.

  **So nobody ever wrote 21 GB.** Batch 22's gate was killed by the
  apparent size of sparse files that occupied about **104 MiB** of
  disk, caught because one poll landed inside a window lasting
  **0.11 s**. The 250× span is not a property of the work; it is
  whether a poll landed in that window. The same figure measured the
  wrapper and not the job on the memory side too:
  `peak_memory_bytes` was **3,117,056** — 3 MB, the top-level `bash` —
  for a job that ran a cold cargo build, because with no cgroup
  `process_rss_bytes` reads one pid and not its children.

- **Expected now:** three things, until ORCH-268 is answered.
  1. **Do not read `bytes_written` as bytes written.** It is the
     apparent size of `$HOME` plus `$TMPDIR` plus the workspace,
     sampled. For an ad-hoc job it excludes the build entirely.
  2. **The disk cap does not bound an ad-hoc job.** Measured below:
     300 MiB written into the job's cwd is counted exactly; the same
     300 MiB written into a worktree, or into `$HERD_BUILD_ROOT`, is
     counted as **zero**. The cap binds only a *declared* command,
     whose tree is checked out into `work/`.
  3. **A gate submitted through Herd must not be capped by its own
     test suite.** `./scripts/test.sh` runs the sparse-fixture tests,
     so a gate job's figure is a lottery against a 20 GiB budget. This
     is the cap to raise or the figure to change — not a reason to
     re-run a job hoping for a kinder sample.

  **What does *not* work, and had been believed to:** redirecting
  `CARGO_TARGET_DIR` does not lower the figure, and splitting a gate
  into one job per gate does not either. Neither ever touched the
  measured path. `j0000000036` is the proof: one undivided `gate.sh`
  with the target redirected still recorded 8.08 GiB, because the
  figure is about `$HOME` and `$TMPDIR` and not about `target/`. What
  kept `j0000000043` and `j0000000044` at 89 MB is that they ran only
  `cargo build` and so never ran the sparse fixtures.
  `scripts/bench-preflight.sh:320` is therefore **cap-neutral**: it
  moves bytes from one unmeasured path to another. It guards the root
  filesystem, which is its job, and it neither defeats nor helps this
  bound.

- **Verify:** eight jobs through
  `/mnt/ssd_980/herd/queue/bin/herdctl`, `--label build
  --memory-mib 6000`, each writing the same known 300 MiB
  (`314572800`) with `head -c` and differing in one path. The figure
  moves with the path and with nothing else.

  | job | what it wrote, and where | `bytes_written` |
  |---|---|---|
  | `j0000000054` | nothing | `0` |
  | `j0000000055` | 300 MiB into `$PWD` (the workspace `work/`) | `314572800` |
  | `j0000000056` | 300 MiB into the worktree it `cd`'d to | `0` |
  | `j0000000057` | 300 MiB into `$HERD_BUILD_ROOT/…` | `0` |
  | `j0000000058` | 300 MiB into `$TMPDIR` | `314572800` |
  | `j0000000059` | `truncate -s 8G` in `$PWD`, 16 KiB of real disk | `8589934592` |
  | `j0000000060` | `cargo test -p herd-runner --lib packstore::`, target redirected | `89586215` |
  | `j0000000061` | `cargo fetch --locked`, target redirected | `194282312` |

  `j0000000059` is the sparse proof: 8 GiB recorded, **16,384 bytes**
  of disk occupied. A job can trip the cap having written nothing.

  `j0000000061` is the reconciliation, to the byte: its
  `bytes_written` of `194282312` **equals** its measured `$HOME`, which
  held `.cargo/registry` `190801624` and `.cache/nix` `3415152` across
  6,491 files. The 89 MB of `j0000000043` and `j0000000044` is the same
  thing, smaller because a build fetches only what it compiles —
  `j0000000060` recorded `89586215` against a `$HOME` of `89342924`.

  `j0000000060` is the sampling proof. It ran the sparse fixtures and a
  0.5 s sampler inside the job caught the measured base at
  **21,564,710,785 apparent bytes against 109,563,904 real** — 20.08
  GiB of nothing — while the runner's own 200 ms poll **missed it** and
  recorded 89 MB. `j0000000040` had the opposite luck and lost a
  landing's gate to it.

- **Replaces:** nothing. It gives ORCH-268 its measurement, and it
  withdraws the belief that a redirected `CARGO_TARGET_DIR` or a split
  gate changes a job's exposure to the disk cap.
