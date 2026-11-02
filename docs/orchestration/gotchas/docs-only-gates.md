# Docs-only gates

- **Date:** 2026-10-23 (memory `docs-only-gates.md`; Alexa on the
  architecture document: *"on skip pour lui il y a literalement pas
  besoin"*).
- **Observed:** proving the Rust gates on a prose change means a cold
  `target/` in a fresh worktree, about ten minutes of compile to learn
  nothing.
- **Why:** the Rust gates cannot fail on a prose change, and CI runs all
  four on every PR anyway (`ci.yml` has no path filter).
- **Expected now:** a change confined to `CLAUDE.md`, `README.md`,
  `docs/`, `.claude/`, `.github/*.md` and only the markdown under
  `deploy/` runs `./scripts/check-docs.sh` locally. The report says the
  Rust gates were **not run** and that the landing's `gate.sh` covers
  them. It never says they passed. A brief for such a task carries no
  "exactly N `HERD-SKIPPED`" expectation, since no suite runs.
- **Verify:** the handoff's *Gates* section shows the `check-docs.sh`
  output and the sentence about the Rust gates; the batch's one landing
  runs `./scripts/gate.sh` on the gated sha, and `herd-merger`'s filled
  landing record carries that verdict. There is no pull request to look
  at: nobody opens one but `herd-merger`, once, for the batch's one
  landing (Alexa, 2026-10-28).
- **Replaces:** the 2026-10-13 `CLAUDE.md` reading "all four green before
  any commit" for prose; it still governs code.
