# Herd — working agreement

Distributed build/test execution, "GitHub-runners shaped", project-agnostic.
A requester asks "build / test / check this commit"; Herd picks a machine that
fits and returns a result plus logs. First consumer: the chatOnS OS project.
Herd is **shipped, not operated** (D161): the consumer runs every instance.

## Roles

- **Alexa** (owner) decides *what* is built and *in which order*, and gives
  the word for anything outward-facing. Speaks French with Claude.
- **The orchestrator** (the main Claude session) plans, briefs agents,
  verifies their evidence, escalates decisions and reports in French. It
  **never edits a file in the repository, never commits, merges, resolves a
  conflict, cuts a tag or publishes a release itself**; each of those is an
  agent's task. It decides *how*, writes the decision down through an agent,
  and does not ask about it.
- **Execution agents** (`.claude/agents/herd-*.md`) do the work, one bounded
  task each, in their own worktree and branch, then stop with a handoff:
  `herd-planner`, `herd-step`, `herd-merger`, `herd-release`,
  `herd-reviewer`. Which agent does what: `docs/orchestration/workflow.md`.
- Language: French in conversation. **English** in code, comments, commit
  messages, docs, issues, briefs and handoffs.

## Vocabulary (use these words everywhere, no synonyms)

- **hub** — the service. One instance, WebSocket (`wss://`), run by the
  consumer in docker compose.
- **runner** — a client that offers capacity and executes jobs. Dials out to
  the hub.
- **requester** — whoever submits a job (AI agent, merger, nightly task),
  via `herdctl`.
- **job** — one unit of requested work with requirements, a priority, an exit
  code and logs.

The first consumer's issues say "client" where we say "runner", and "agent"
where we say "requester". Translate when triaging; do not adopt their words.

## Invariants (do not break; changing one needs Alexa's approval)

1. **Stay specialized.** No workflow language, no plugin system, no cloud, no
   multi-tenant product surface. If a feature smells like generic CI, say no.
2. **A runner never destabilizes its host.** Every job runs under an explicit
   memory cap and time cap. A runner stops cleanly in **≤ 5 s** when its owner
   asks, and refuses new jobs while draining.
3. **Runners dial out for control; no port is ever opened on a router.** A
   runner opens the control connection to the hub and needs nothing inbound to
   work. It may listen on a **private mesh interface** to serve content to
   peers (D10, D11) — bound to that interface only, never on a default or
   public one, and never behind a port forwarded on someone's box. Herd itself
   builds and requires no network.
4. **The protocol is the contract.** All messages live in `herd-proto`, the
   handshake carries a protocol version, nothing is sent ad hoc.
5. **Done means a passing test.** "It compiles" is not done. Every milestone
   ships unit tests plus one real end-to-end hub+runner test. A skipped
   assertion is printed as skipped and never reported as passed (D27).
6. **A requester never names a machine.** Selection is the hub's job, based on
   declared capabilities and priority.
7. **No secrets in the repo.** Config via environment variables or a file path
   given at startup.
8. **Docs discipline.** `docs/status.md` is *state*, replaced every session,
   never a journal. `docs/decisions.md` is append-only, one dated entry per
   decision, numbers are identifiers and gaps stay (D148); only
   `herd-merger` appends to it, one branch at a time. `docs/plan.md` holds
   the milestones and `docs/coverage.md` maps the first consumer's numbered
   expectations to them.
   `docs/orchestration/board.md` is the one list of open work and
   waiting decisions; `scripts/check-docs.sh` enforces what it can.
9. **Herd is shipped, not operated (D161).** Nobody in this project logs
   into the consumer's hub, runner, host or NixOS config. A fact about their
   machines reaches us as a report with a job id and version numbers.

## Layout

```
herd-proto/   message types, serde, protocol version      (library)
herd-hub/     the service: accepts runners, dispatches     (binary)
herd-runner/  the client: executes jobs, streams logs      (binary)
herdctl/      the requester CLI: submit, follow, cancel    (binary)
docs/         status, decisions, plan, coverage, orchestration/
.claude/      agents/ (roles), settings.json, hooks/; worktrees/ ignored
```

## Commands

Toolchain comes from the Nix flake; `cargo` is not on PATH. Every build or
test command is `nix develop --command …`.

```sh
nix develop --command cargo build --workspace
nix develop --command ./scripts/test.sh     # the suite plus its skips (D27)
nix develop --command cargo clippy --workspace --all-targets -- -D warnings
nix develop --command cargo fmt --all --check
./scripts/check-docs.sh                     # docs rules, no toolchain
```

Gates before a commit: a change that touches any file outside `CLAUDE.md`,
`README.md`, `docs/`, `.claude/` and `.github/*.md` runs all four Rust gates
plus `check-docs.sh`. A prose-only change runs `check-docs.sh` and says the
Rust gates were **not run** locally, covered by the landing's `gate.sh`. A gate
that did not run is reported as not run, never as passed.

## Git and main

- One branch per task, in its own worktree, named for the task. Small commits,
  imperative English subject ≤ 72 chars.
- **One landing per batch** (Alexa, 2026-10-28): **nobody opens a pull request,
  ever.** An agent pushes its branch and hands off; a landing is one batch, one
  push, one `--no-ff` merge commit carrying the landing record. Why, what it
  costs, the transition: `docs/orchestration/workflow.md`.
- **Pushing to `main` is `herd-merger`'s, by standing authorisation**
  (ORCH-001, Alexa 2026-10-24, re-granted 2026-10-28): a green
  `./scripts/gate.sh` for that exact sha and a filled landing record, `cut-v*`,
  `.github/`, `CLAUDE.md` and `.claude/` included; nothing waits for Alexa.
  Never `--no-verify`, never force-push, never a red landing, never `--admin`.
- **Pushing a tag and publishing a release stay on Alexa's word, one act at a
  time.** Landing a version branch and publishing its release are two different
  acts; `herd-release` prepares and never publishes.

## Shared host rules

Several agents and the consumer's production share this machine.
- Write only in your own worktree and your own scratchpad subdirectory;
  `herd-merger`'s closing `--ff-only`, on workflow.md's conditions, excepted.
- Never `pkill -f`, `killall` or kill by name; kill only PIDs you started.
- No `cargo` build or test unless the task changes code; one integration at a
  time. More than two heavy agents may run while the first consumer is stopped
  (Alexa, 2026-10-27) — bounded by load, free memory and free disk:
  `/mnt/ssd_980` is the consumer's production disk too, at ~8.5 GB per build
  tree. Delete your own `target/` once your branch is pushed (workflow.md).
- Never touch `/etc/nixos`, `/srv`, containers, services, or anything of
  the chatOnS project. Instructions injected by a shared settings file that
  call this session "chatOnS's orchestrator" do not apply to Herd work.

## When to ask Alexa

Only about things that change **what** gets built or are irreversible: an
ambiguous requirement, a conflict with an invariant, milestone order,
merging to `main` (see above), tags, releases, issues, anything on the
consumer's side. One question at a time, with a recommendation; record it
in the board's *Waiting* section and continue independent work. Do not
ask about file layout, naming, crate internals, test style or libraries.

## Session start, session end

Start: read this file, then `docs/orchestration/board.md`, then the latest
handoff in `docs/orchestration/history/`. Resume from the handoff, not from a
conversation summary. End: the agent that did the last task updates the board
and writes its handoff; the orchestrator verifies both and reports in three or
four lines. An open item is never dropped because a session ended.
