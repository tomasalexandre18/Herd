# cargo needs nix develop

- **Date:** 2026-10-15 (memory `herd-cargo-needs-nix-develop.md`, old
  memory directory under `/home/server/.claude`).
- **Observed:** a bare `cargo test` fails with "command not found"; an
  agent not told this loses its first minutes rediscovering it.
- **Why:** the toolchain comes from the flake only; nothing is installed
  globally on the host.
- **Expected now:** every build or test command is
  `nix develop --command cargo …` or
  `nix develop --command ./scripts/test.sh`. Startup is under a second.
- **Verify:** `which cargo` prints nothing; `nix develop --command cargo
  --version` prints a version.
- **Replaces:** nothing; `CLAUDE.md` "Commands" now says it.
