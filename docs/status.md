# Status — 2026-10-12

State of the project right now. Replaced every session, never appended to.

## Where we are

Milestone **M0 (method)** is done and approved. No code exists yet, by design.
Next up: M1, the end-to-end `herdctl run -- echo hello`.

## What works

Nothing executable. What exists is the working agreement:

- `CLAUDE.md` — roles, vocabulary, 8 invariants, commands, git rules.
- `docs/decisions.md` — D1 Rust, D2 vocabulary, D3 four crates, D4 WebSocket
  + JSON + runner dials out, D5 deployment shape, D6 what is left open.
- `docs/plan.md` — M1 to M6, one session each.

`github.com/tomasalexandre18/Herd` now has `main` as its default branch,
holding exactly this content. The repo was empty before today.

## What is not done, and why

- **No toolchain yet.** Rust is not installed on the server; it arrives with
  the Nix flake in M1. Every command in `CLAUDE.md` is therefore the agreed
  contract, not yet verified. `docker` and `gh` are present and working.
- **Nothing runs.** D1 (Rust) is approved; the workspace itself is M1 work.
- **Six technical questions deliberately unanswered** (see D6): auth model,
  scheduling policy, isolation mechanism, log retention, job input model.
  They wait for the chatOnS requirement sheet.

## Environment facts (discovered this session)

- Server: NixOS, kernel 6.18, i5-10600K, 12 threads, 31 GB RAM (~11 GB free
  right now), `/dev/kvm` present, Intel iGPU at `/dev/dri/renderD128`.
  Nix with flakes enabled, docker 29.7.
- GitHub: `tomasalexandre18` authenticated via `gh` (scopes `repo`,
  `workflow`, `read:org`). **No issues yet** — the requirement sheet has not
  arrived.
- Caution: the SSH key on this machine is a deploy key for another repository
  (`torgaddon`); git uses the HTTPS remote with the `gh` credential helper.

## Next action

Start **M1** on branch `m1-end-to-end`: Nix flake with the dev shell, cargo
workspace with the four crates, the five first protocol messages, and the
end-to-end test that runs `echo hello` through hub + runner and checks the
exit code. Waiting on Alexa's go.
