# release-notes-0-4-1 — handoff 2026-10-29

- **Item:** ORCH-162 (the half that unblocks `orch162-notes`)
- **Agent:** herd-step
- **Branch / commit:** `release-notes-0-4-1` / `6e10c02`, based on
  `orch162-notes` `80f2f3e` with `main` `1a8c1ae` merged in (`0bb92f4`,
  clean, no conflict)
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`deploy/release-notes/0.4.1.md` exists: 129 lines, 6549 bytes, max 72
columns. Its sibling `0.4.0.md` is 276 lines, 13100 bytes, max 72
columns, so the new file is the same shape and about half the length --
v0.4.1 is one fix and v0.4.0 was thirteen changes.

Shape, matched against `0.4.0.md` and `deploy/release-notes/README.md`:

- no `#` title (the generated half carries the heading), five `##`
  sections, no trailing `---` (`release.yml` appends the rule itself)
- it restates none of what the generated half or the standing notice
  already carries: no file names, no `sha256sum` lines, no `docker
  load`, nothing out of `deploy/release-notice.md`
- it opens on the same operator paragraph `0.4.0.md` opens on
  (invariant 9, D161, "Nothing in this project deploys anything")

**Nothing in it is authored here.** v0.4.1 is already tagged, already
published, and its human half is already on the public page. Lines 6-129
are the bytes of
`docs/orchestration/history/2026-10-29-release-v0.4.1-body.md` below its
`---` separator, and `cmp` says byte-identical. Lines 1-4 are the bytes
of `0.4.0.md`'s first paragraph. Nothing was retyped, re-wrapped or
re-encoded: the file was spliced out of the two sources with raw-byte
perl `index`/`substr` and installed with `cp`, which is why the mojibake
detector below has nothing to find. No reader of the release page sees
anything change.

Why satisfy the guard rather than narrow it: narrowing it to versions
that have a tag, or to versions above a floor, would weaken the one
mechanism that stops the next version bump forgetting its notes, which
is the whole point of `orch162-notes`. The guard met a bump that landed
before it existed (the workspace has been at 0.4.1 since batch 7); it
was right to refuse, and the right answer is the missing file.

## Not done, and why

- **Nothing outside `deploy/release-notes/0.4.1.md` and this handoff.**
  `git status` on the committed tree shows those two paths and nothing
  else. `scripts/check-docs.sh`, `scripts/check-orchestration.sh`,
  `.github/workflows/release.yml`, `0.4.0.md`, the README, every version
  number, `docs/decisions.md` and `docs/orchestration/board.md` are
  untouched.
- **No board row and no decision entry.** Both are the merger's to
  place; the draft text is under *Decisions needed* below.
- **The four Rust gates were NOT RUN locally.** This change touches no
  crate -- one new file under `deploy/` -- and three other agents were
  compiling on this host, so `cargo` was deliberately not invoked. They
  are covered by the batch's own `./scripts/gate.sh` at integration,
  which is `herd-merger`'s run and not mine. A gate that did not run is
  not a gate that passed.

## Gates

Runs made **before** this file was committed, on the tree that carries
`0.4.1.md`.

`./scripts/check-docs.sh` **before** the file existed -- the refusal the
brief quotes, reproduced:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
deploy/release-notes/0.4.1.md does not exist: the workspace is at 0.4.1 and a release's notes are a file in the tree, not a paste into the page (ORCH-162). Write it, or put the version back.
docs check failed
exit=1
```

`./scripts/check-docs.sh` **after** writing it:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit=0
```

The `note:` line about D170 is a plain `echo` at `check-docs.sh:122`,
not a call to `note()`, so it does not set `fail`. It is present in both
runs and on `main` as well.

**The guard still bites.** With the new file moved aside:

```
deploy/release-notes/0.4.1.md does not exist: the workspace is at 0.4.1 and a release's notes are a file in the tree, not a paste into the page (ORCH-162). Write it, or put the version back.
docs check failed
exit=1
```

and with it present but truncated to empty, the rule's other half:

```
deploy/release-notes/0.4.1.md is empty: an empty notes file publishes an empty half, which is the defect this rule exists to catch (ORCH-162).
docs check failed
exit=1
```

After each, the file was restored and `cmp` confirmed it byte-identical
to the committed bytes, and `check-docs.sh` returned to `docs ok`.

`./scripts/check-orchestration.sh`:

```
check-orchestration: 291 passed, 0 failed
```

291 and not `main`'s 285 because `orch162-notes` adds exactly six cases
of its own, all six green and all six ORCH-162's: *no notes file for the
workspace version*, *a blank notes file*, *release.yml no longer reads
the notes directory*, *the second assembly drops the notes*, *an absent
notes file made fatal*, *notes for versions that are not the
workspace's, one of them blank*. 285 + 6 = 291, so the delta is
accounted for and not a difference to explain away.

The **last** run is on the tree that includes this handoff, after this
commit and before the push: `./scripts/check-docs.sh` and
`./scripts/check-orchestration.sh` in the worktree, output in this
agent's report rather than in the tree it has just gated.

**Skips:** no skip. No `HERD-SKIPPED` line in either run.

## Risks and findings

- **`deploy/` is not width-checked, and that is a finding and not a
  complaint.** `check-docs.sh`'s 79-column rule iterates over
  `CLAUDE.md README.md docs/*.md docs/orchestration/*.md
  docs/orchestration/*/*.md` only, so nothing enforces the wrap on
  `deploy/release-notes/`. Both files there happen to be at 72 columns
  because they were written that way. A future notes file pasted as one
  long line would pass every gate. Low harm (GitHub soft-wraps a release
  body) and not mine to change.
- **The body says it does not restate the version numbers, and then
  does.** The internal half of
  `2026-10-29-release-v0.4.1-body.md`, above the separator, says the
  generated half already carries `PROTOCOL_VERSION: 2` and
  `RECORD_VERSION: 1` and that "nothing below restates those". The human
  half below the separator restates both, under *Nothing else changed*,
  and it needs to: the point being made there is that neither number
  moved, which is the reason a runner can be upgraded alone. `0.4.0.md`
  does the same thing in two places, and the README's prohibition names
  only "the sums, the file names or the notice". So the sibling file and
  the README both permit it and only that one internal sentence is
  overstated. Transcribed as written; flagged rather than edited.
- **The two standing cautions are not a duplicate of the notice.** The
  human half's *Two standing cautions* section carries the hub
  container's 5 s of grace and the `herd-hub report` reading before a
  start on an old root. `deploy/release-notice.md` was read in full and
  carries **neither** -- its 5 s sentence is about a *runner* draining
  under `systemctl stop` (invariant 2), a different claim about a
  different process. `0.4.0.md` carries both cautions itself, in
  *Upgrading and rolling back*. So the shape is consistent and nothing
  is said twice on the page.
- **Mojibake: measured, with the detector proven first.** A byte regex
  (`\xc3\xa2\xc2[\x80-\xbf]\xc2[\x80-\xbf]` three-byte,
  `\xc3\x82\xc2[\x80-\xbf]` two-byte) under perl with no `:utf8` layer.
  On a copy of the file deliberately double-encoded it reported 14
  mangled sequences and exit 1; on the clean copy, 0 and exit 0. Only
  then was its verdict on the committed file believed: **0 sequences**.
  The file is valid UTF-8 (`iconv -f UTF-8 -t UTF-8`) and its non-ASCII
  content is exactly 12 em dashes (U+2014) and 2 right arrows (U+2192),
  no typographic quotes at all. `grep -P` was not used for this; it is
  ugrep 7.8.4 here and its `-P` matches no `\xNN` byte pattern.
- **Nothing here touches the act D210 reserves.** No tag, no release, no
  edit to a published page, no new sentence about what v0.4.1 contains.
- **One premise of the brief refined, not refuted.** The brief says the
  branch "brings `0.4.0.md` only"; it also brings `README.md` in the
  same directory, which is what defines the file's shape. Nothing else
  in the brief was contradicted by the code.

## Decisions needed

None blocking. One draft for the merger to place, if it wants the
reasoning recorded rather than left in this handoff -- the orchestrator
chose it, so it may not need an entry at all:

> **D-NEXT (2026-10-29) -- a version bump that predates the notes guard
> gets its notes written, not the guard narrowed.** ORCH-162 made a
> release's notes a file in the tree and `check-docs.sh` requires the
> file for the version the workspace carries. The workspace had been at
> 0.4.1 since batch 7, so the guard refused its own branch's merged
> tree. Narrowing it -- to versions with a tag, or above a floor --
> would weaken the one mechanism that stops the next bump forgetting its
> notes. So `deploy/release-notes/0.4.1.md` was written instead, by
> transcribing the already-published human half out of
> `docs/orchestration/history/2026-10-29-release-v0.4.1-body.md`. A
> notes file for an already-published release authors nothing and is not
> the act D210 reserves.

## Resume point

For `herd-merger`: take `release-notes-0-4-1` and **not**
`orch162-notes` -- it contains that branch plus `main` plus the one file
that makes it green, so it is one branch instead of two. Then
`./scripts/gate.sh` on the batch, which is where the four Rust gates
are covered.

## Processes

No background process started and none killed; no PID to report. No
`cargo` invoked, so no `target/` and no `CARGO_TARGET_DIR` was created
and there is nothing to delete (`build-weight-off-root` does not apply
to a prose-only task). The worktree, left in place for the merger --
`git worktree remove` on it is safe once the branch is taken:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-release-notes-0-4-1
```
