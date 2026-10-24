# remediation-dossier-index — handoff 2026-10-24

- **Item:** none allocated (the orchestrator opens the ORCH line; this
  agent does not touch `docs/orchestration/board.md`)
- **Agent:** herd-step
- **Branch / commit:** `remediation-dossier-index`, based on `origin/main`
  `0a2cf73`
- **PR:** none — `herd-merger` opens it

## Done

Two prose files, in English, nothing else touched:

- `docs/remediation/00-baseline.md` — lot T0-A of the audit mandate. The
  state of Herd at `0a2cf73`: what the software is, the commit and its
  relation to tag `v0.3.7`, the release line, the four crates with their
  measured sizes, the four Rust gates plus `check-docs.sh` and
  `check-orchestration.sh`, the loud-skip machinery (D27), the frozen
  `PROTOCOL_VERSION` 2 / `RECORD_VERSION` 1 and the three `!=` checks, the
  nine repository invariants, the two known-unstable tests with ORCH-022's
  evidence, the two real incidents (D169, D171) with their figures, a
  "verified / not verified" section, and the prerequisites for a later
  test campaign.
- `docs/remediation/README.md` — the index required by §10 of the mandate.
  One row per piece: `00-baseline.md`, `03a`, `03b`, `03c` with their
  branches and what each settles; `04a-cost-surface.md` and
  `04b-integrity-and-containment.md` announced as expected, with no claim
  about their content. Plus how the dossier reads (everything is a
  constat, no measurement has been made), the F/H/I marks, and the
  statement that nothing in the dossier authorizes a modification of the
  software — every decision is Alexa's, one at a time.

The three audit branches were fetched read-only
(`git fetch origin '+refs/heads/audit-*:refs/remotes/origin/audit-*'`) and
read with `git show`. None was modified.

## Not done, and why

- `docs/orchestration/board.md` — out of bounds by the brief; two other
  agents are working in parallel and the ORCH lines are the
  orchestrator's.
- `04a` and `04b` — being written by other agents; this branch only
  announces them.
- The mandate's `01`, `02`, `05`… pieces — not asked for. The index keeps
  their numbers free rather than renumbering.

## Gates

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

Exit 0. The `note:` line is informational, not a failure.

Rust gates **NOT RUN** locally — prose only, and the brief forbids
`cargo` on this shared host. CI runs them.

Prose was additionally held to 79 columns by hand: `check-docs.sh`'s
column loop covers `docs/*.md` and `docs/orchestration/*/*.md` and does
**not** reach `docs/remediation/`, so the rule was applied manually with
the same awk/shell measurement the script uses. Both files are clean.

No skip: no suite was run.

## Risks and findings

Four things in the brief that the repository corrects or cannot support.

1. **The 12 loud skips, and the 10 / 2 split, are `I`.** The split into
   "10 cgroup-v2 delegation and 2 ZFS `io.max`" is stated in **no** file
   in the tree. Worse, the count itself is contradicted inside the
   repository: `docs/status.md:49` and `docs/coverage.md:113` say **12**,
   while D151 (2026-10-16) records that **12 was the bug** — two skips
   interleaved onto one line by `writeln!` — and that after the
   `write_all` fix "the gate run for this branch reports **13**". The fix
   is in the tree (`herd-runner/src/testing.rs:46`) and the "12" in
   `coverage.md` was written on 2026-10-23 (`ff3a525`), a week after it.
   Either a skip site went away or a stale figure was restated. No
   `cargo` was allowed, so the baseline marks the count and the split
   **I** and says how to get them (`./scripts/test.sh`, two minutes).
2. **`follower_alive.rs:231` is the condition; the panic is at 230.** The
   `assert!` begins at line 230 and its condition `longest < spread / 2`
   is on 231. `docs/orchestration/board.md` (ORCH-022) and `03b §9` cite
   230; `03c` cites 231. Same assertion. The baseline says both numbers
   and why.
3. **"`PROTOCOL_VERSION` 2 and `RECORD_VERSION` 1 frozen since D73" is a
   shorthand, not the fact.** D73 (2026-10-13) sets the *regime*: both
   numbers free-running before M16, both **reset to 1** at M16, freeze
   from there. The pair is 2/1 and not 1/1 because D148 (2026-10-16)
   deliberately paid for protocol version 2. The baseline states D73 set
   the regime and D148 set the 2. The `!=` reading in the brief is
   exactly right and is now written down with its three sites
   (`server.rs:2141`, `store.rs:1143`, `store.rs:2297`).
4. **The tag warning in the brief is correct, and there is more to say.**
   `v0.3.7` is `3da48f5`; `0a2cf73` carries no tag. Beyond that: **no
   Rust, `Cargo.*` or Nix file differs between the two.** Verified with
   `git diff --name-only 3da48f5 0a2cf73 -- herd-proto herd-hub
   herd-runner herdctl Cargo.toml Cargo.lock nix flake.nix flake.lock`,
   which is empty. The 17 commits between are docs, `.claude/`, CI
   workflows and scripts. The mislabel still matters; the audited product
   is nevertheless `v0.3.7`'s product.

Two smaller ones:

- The mandate's §1.1 size table does not describe `0a2cf73`: it gives
  `herd-hub` 21 757 production lines and 649 test functions; measured
  here, 22 130 and 651. The other three crates match exactly. The
  baseline publishes its own measurements and says the mandate read an
  earlier commit.
- `CLAUDE.md` has **nine** invariants at `0a2cf73` and **eight** at
  `3da48f5`: invariant 9 ("Herd is shipped, not operated", D161) and the
  board clause in invariant 8 arrived with PR #71.

Risk this branch carries: the baseline quotes D169's and D171's figures as
*recorded*, not as re-measured. If either entry is itself wrong about its
own incident, the baseline inherits that. Nothing available to a reader of
this repository can settle it; the incident hosts are the consumer's
(invariant 9).

## Decisions needed

None from this branch. One worth putting in front of Alexa at some point,
with a recommendation: **run `./scripts/test.sh` once on a quiet host and
write the real skip count and its split into `docs/coverage.md`**, because
a count of what was *not* checked that is itself wrong is the exact defect
D27 exists to prevent, and the tree currently states two different
numbers. Recommendation: do it as part of the next code-touching branch,
not as a branch of its own. Fallback: leave both figures marked **I**
wherever they are used.

## Resume point

`herd-merger` opens the PR for `remediation-dossier-index`. The index's
`04a` / `04b` rows will need their branch names filled in once those two
branches exist; that is a one-line edit per row and belongs to whoever
merges them, not to a second pass here.

## Processes

None started, none ended. No `cargo`, no binary, no service.
