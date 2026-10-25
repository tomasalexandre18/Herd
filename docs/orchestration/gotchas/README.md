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
