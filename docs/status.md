# Status — 2026-10-12

State of the project right now. Replaced every session, never appended to.

## Where we are

M0 (method) done and approved. The first consumer's requirement sheet arrived
as **issue #1** the same day; the plan has been rebuilt against it. Still no
code, by design. Branch `plan-rework` pushed, awaiting approval to move `main`.

## What works

Documentation only:

- `CLAUDE.md` — roles, vocabulary (+ the consumer's word-for-word translation),
  8 invariants, commands, git rules.
- `docs/decisions.md` — D1 Rust, D2 vocabulary, D3 four crates, D4 WebSocket +
  JSON, D5 deployment shape, D6 what was left open, D7 (superseded), **D8**
  hub persistence as a directory per job, **D9** warm caches (Herd carries
  one, Herd is not one), **D10** the data path (Herd is never in the byte
  path).
- `docs/plan.md` — **M1..M18**, each one session, with the critical path, the
  parallel tracks, 13 decisions to settle before the code that depends on
  them, and the conflicts and risks.
- `docs/coverage.md` — every expectation E-01..E-55 mapped to a milestone.

## The requirement sheet, in figures

55 expectations: **44 must**, 9 should, 2 could (recounted from the sheet).
All 44 musts are placed in a milestone; nothing is unplaced. Only E-38 and E-39
(should) are deliberately unscheduled, with reasons; E-55 (could) was promoted
to the default by D10.
Nine acceptances are gated on chatOnS's own deliverables and close in M18;
fixture tests stand in until then.

## What is not done, and why

- **No toolchain yet.** Rust is not installed; it arrives with the Nix flake
  in M1, so every command in `CLAUDE.md` is still the agreed contract rather
  than a verified one. `docker`, `nix` and `gh` are present and working.
- **Two decisions are Alexa's.** Q10, the licence (outward-facing). And one
  raised by D10: her PC's rule is "accepts no inbound connection" (E-34), but
  serving a blob to a peer means listening on the mesh interface — until she
  rules, the PC is outbound-only (pushes to and pulls from a keeper, never
  serves). Q5 (the warm caches) was hers and is settled as D9; Q1..Q4, Q6..Q9,
  Q11..Q13 are mine, and D8, D9, D10 are taken.
- **Nothing is deployed and no runner exists.** First exposure over `wss://`
  is M16, on purpose: deploying earlier would freeze the public formats
  (the `ran on:` line, the status JSON, the event payloads) on a surface still
  moving.
- **Issue #1 is unanswered.** Replying on GitHub is outward-facing and waits
  for Alexa's go.

## Environment facts

- Server: NixOS, kernel 6.18, i5-10600K, 12 threads, 31 GB RAM (~11 GB free),
  no swap, `/dev/kvm` present, Intel iGPU at `/dev/dri/renderD128`, nix with
  flakes, docker 29.7.
- GitHub: `tomasalexandre18` authenticated via `gh` (scopes `repo`, `workflow`,
  `read:org`). One open issue (#1) with two comments carrying the full sheet.
- Caution: this machine's SSH key is a deploy key for another repository
  (`torgaddon`); git uses the HTTPS remote with the `gh` credential helper.

## Next action

Alexa approves the plan and answers Q5 (warm caches). Then **M1** on branch
`m1-end-to-end`: Nix flake dev shell, the workspace's four crates, herd-proto
v1, and the in-process hub+runner+requester test that runs a command and
propagates its exit code.
