# Shared CLAUDE_CONFIG_DIR

- **Date:** 2026-10-24 (remediation H0).
- **Observed:** the Herd orchestrator and the chatOnS orchestrator both
  run with `CLAUDE_CONFIG_DIR=/mnt/ssd_980/chatons/claude/config`. That
  directory's `settings.json` has an `autoMode` block whose
  `environment` says *"This session is chatOnS's orchestrator"* and whose
  `allow` list grants `sudo systemctl`, `nixos-rebuild switch`, docker
  operations on the hub and pushes to `/etc/nixos`.
- **Why:** both sessions were started from the same user configuration.
  Auto memory is still separate (it is keyed by project directory), but
  user-level settings are not.
- **Expected now:** Herd work ignores that persona and those allowances
  (D161: Herd is shipped, not operated). `CLAUDE.md` says so. The clean
  fix is outside the repository and is the owner's: a separate
  `CLAUDE_CONFIG_DIR` for Herd, or moving the `autoMode` block into the
  chatOnS project's own `.claude/settings.json`.
- **Verify:** `tr '\0' '\n' < /proc/<pid>/environ | grep CLAUDE_CONFIG_DIR`
  on the orchestrator's PID; `/status` in the session shows the setting
  sources.
- **Replaces:** nothing.
