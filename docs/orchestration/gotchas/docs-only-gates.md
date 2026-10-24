# Docs-only gates

- **Date:** 2026-10-23 (memory `docs-only-gates.md`; Alexa on the
  architecture document: *"on skip pour lui il y a literalement pas
  besoin"*).
- **Observed:** proving the Rust gates on a prose change means a cold
  `target/` in a fresh worktree, about ten minutes of compile to learn
  nothing.
- **Why:** the Rust gates cannot fail on a prose change, and CI runs all
  four on every PR anyway (`ci.yml` has no path filter).
- **Expected now:** a change touching only `CLAUDE.md`, `README.md`,
  `docs/`, `.claude/` and `.github/*.md` runs `./scripts/check-docs.sh`
  locally. The report says the Rust gates were **not run** and that CI
  covers them. It never says they passed. A brief for such a task carries
  no "exactly N `HERD-SKIPPED`" expectation, since no suite runs.
- **Verify:** the PR's Evidence block shows `check-docs.sh` output and
  the sentence about the Rust gates; CI's `check` job is green on the PR.
- **Replaces:** the 2026-10-13 `CLAUDE.md` reading "all four green before
  any commit" for prose; it still governs code.
