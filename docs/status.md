# Status — 2026-10-12

State of the project right now. Replaced every session, never appended to.

## Where we are

Milestone **M0 (method)**, awaiting approval. No code exists yet, by design.

## What works

Nothing executable. What exists is the working agreement:

- `CLAUDE.md` — roles, vocabulary, 8 invariants, commands, git rules.
- `docs/decisions.md` — D1 Rust, D2 vocabulary, D3 four crates, D4 WebSocket
  + JSON + runner dials out, D5 deployment shape, D6 what is left open.
- `docs/plan.md` — M1 to M6, one session each.

Repository initialised locally, branch `m0-method` pushed to
`github.com/tomasalexandre18/Herd` (the repo was empty).

## What is not done, and why

- **No toolchain yet.** Rust is not installed on the server; it arrives with
  the Nix flake in M1. Every command in `CLAUDE.md` is therefore the agreed
  contract, not yet verified. `docker` and `gh` are present and working.
- **No stack validation.** D1 (Rust over Go) is a recommendation pending
  Alexa's approval; it is the one thing that is expensive to undo.
- **Six technical questions deliberately unanswered** (see D6): auth model,
  scheduling policy, isolation mechanism, log retention, job input model.
  They wait for the chatOnS requirement sheet.
- **`main` does not exist on GitHub yet** — waiting on approval to move it.

## Environment facts (discovered this session)

- Server: NixOS, kernel 6.18, i5-10600K, 12 threads, 31 GB RAM (~11 GB free
  right now), `/dev/kvm` present, Intel iGPU at `/dev/dri/renderD128`.
  Nix with flakes enabled, docker 29.7.
- GitHub: `tomasalexandre18` authenticated via `gh` (scopes `repo`,
  `workflow`, `read:org`). Repo `Herd` is public and was empty. **No issues
  yet** — the requirement sheet has not arrived.
- Caution: the SSH key on this machine is a deploy key for another repository
  (`torgaddon`); git uses the HTTPS remote with the `gh` credential helper.

## Next action

Alexa approves or rejects **D1 (Rust)**. Then M1: flake + workspace +
end-to-end `herdctl run -- echo hello` with its test.
