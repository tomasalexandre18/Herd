# Gotchas — lessons that cost something once

One file per lesson, read on demand, never all at once. Each file states:
the date, what was observed, why it happens, the behaviour expected now,
how to verify it, and the rule it replaces (if any). A lesson that becomes
a rule in `CLAUDE.md` or an agent file stays here as its evidence.

Provenance: the first five were reconciled on 2026-10-24 from the two
Claude auto-memory directories (H2 of the orchestration remediation).
The memory directories are a per-machine convenience; this index is the
versioned source.

| File | Date | One line |
|---|---|---|
| `cargo-needs-nix-develop.md` | 2026-10-15 | `cargo` is not on PATH; every Rust command goes through `nix develop --command` |
| `orchestrator-does-not-edit.md` | 2026-10-15, 2026-10-23 | the orchestrator briefs and verifies; every repository write is an agent's task |
| `docs-only-gates.md` | 2026-10-23 | a prose-only change runs `check-docs.sh`; the Rust gates are reported as not run, CI runs them |
| `own-scratchpad-no-pkill.md` | 2026-10-23 | one scratchpad subdirectory per agent; never `pkill -f` |
| `shared-config-dir.md` | 2026-10-24 | `CLAUDE_CONFIG_DIR` is shared with chatOnS; its `autoMode` persona does not apply to Herd |
| `decision-number-collisions.md` | 2026-10-16, 2026-10-23 | parallel branches take the same `D` number; the merger allocates, gaps stay |
| `invariants-are-quoted-not-reflowed.md` | 2026-10-24 | an invariant's text changes only as a quoted proposal to Alexa; a rewrite diffs the section word for word |
| `brief-names-the-worktree.md` | 2026-10-24 | a brief names the agent's worktree; the hook cannot keep an agent out of the checkout, only the brief can |
| `build-weight-off-root.md` | 2026-10-24 | `/` filled to 100 % with agent `target/` dirs; preflight before a heavy phase, `CARGO_TARGET_DIR` on `/mnt/ssd_980`, delete it after |
| `a-decision-branch-cannot-be-all-green.md` | 2026-10-25 | a branch writing a `D-NEXT` entry fails `check-docs.sh` by design; three green gates plus that one note is the deliverable |
| `a-gate-joined-by-a-semicolon-is-advice.md` | 2026-10-30 | a status you did not look at is a status you did not get: `;` throws a gate's status away and a pipe replaces it with `tail`'s; join with `&&`, and read `${PIPESTATUS[0]}` or do not pipe a gate at all |
| `a-perl-escape-recodes-the-whole-file.md` | 2026-10-30 | `\x{2013}` in a `perl -i` substitution double-encodes every multibyte character already in the file; use raw bytes under `LC_ALL=C` |
| `a-drafted-title-is-measured-unfenced.md` | 2026-10-30 | a title drafted inside a fence is unchecked; a `decisions.md` header is not exempt, and the longest one is already exactly 79 |
| `an-attribute-census-needs-an-anchor.md` | 2026-10-30 | an unanchored `#[ignore]` count reads 51 where the attribute lines are 38; doc comments quoting it inflate the figure |
| `a-delegated-scope-blinds-the-preflight.md` | 2026-10-30 | an uncapped `systemd-run --user --scope -p Delegate=yes` reads no ceiling at any level, so `bench-preflight.sh` check 4 says *reading unavailable* and clears; give every such scope its own `MemoryMax` |
| `delegation-is-a-property-not-a-file.md` | 2026-10-30 | `Delegate` and `DelegateControllers` on the unit answer *does this delegate*; `cgroup.subtree_control` and a knob's presence do not, and `app.slice` holds `io.pressure` with no `io` controller |
| `bytes-written-is-the-workspace-not-the-job.md` | 2026-10-30 | `bytes_written` is the apparent size of `$HOME` plus `$TMPDIR` plus the workspace, sampled every 200 ms; an ad-hoc job's build is not in it, a sparse file trips the cap having written nothing, and neither a redirected `CARGO_TARGET_DIR` nor a split gate changes any of it; and `du -sb` implies `--apparent-size`, so it confirms this bug with the bug — measure real disk with `du -s --block-size=1` and `stat -c %b` |
