# v10-quarantine — handoff 2026-10-29

- **Item:** none allocated. The brief calls this **V10**, "one damaged
  record must not stop the hub from starting". `board.md` already uses
  "V10" for two other things (ORCH-005/ORCH-032's live validation V10, and
  `07-target-architecture.md`'s V10 under ORCH-089), so the row proposed
  below asks for an `ORCH-nnn` of its own rather than the letter.
- **Agent:** herd-step
- **Branch / commit:** `v10-quarantine` / see the agent's report, based on
  origin/main `cd0f1b7e30aa7401a11dc8ced14d41726c0bc18e`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**A `job.json` this hub cannot read quarantines that one job. It no longer
refuses the whole state root, so the hub starts and binds its port.**

`herd-hub/src/store.rs`, `read_stored`: the three `failed("reading", …)`
calls and the record-version mismatch returned `Err`. That `Err` reached
`recover` (`store.rs:1097`), `open_with` (`:481`) and `main` (`main.rs:358`)
through three `?`s, and `Hub::bind_with` is at `main.rs:434` — seventy-six
lines later. All four now return `Ok(Stored::Quarantined(..))`, the shape
that existed three lines away for a label set today's rules reject (D52).
A fifth case joins them: a `job.json` whose bytes the filesystem will not
hand back for any reason other than `NotFound` (`read_optional`'s `?` was
the same wall). `NotFound` is still `Stored::Missing`, unchanged.

What that buys, each one asserted by a test named under *Gates*:

- One torn record costs **neither** of the two healthy jobs beside it: both
  are still in `unfinished_jobs()`, in order.
- The hub **binds a port** on such a root and serves: the end-to-end case
  starts a hub and a runner on a root whose middle record is torn, both
  healthy jobs run to `finished`, and their logs are byte for byte right.
- The damaged record is **named with its path** at every start (`recover`'s
  line now prints `jobs/<id>/job.json` whatever the reason says), is
  **counted** on the line that says the hub is listening (`main.rs:437`,
  untouched — it already printed `quarantined.len()`), and is **listed** by
  `JobDirStore::quarantined()`.
- A requester asking about it gets a **sentence**: `quarantine_refusal`
  (`server.rs:6198`) already turned the store's refusal into D13's refusal
  frame; it could not reach it before, because `quarantine_reason` errored.
  `herdctl cancel` now comes back with "… is quarantined and this hub will
  not act on it: …", never "no such job".
- The record is **left exactly as it was found**, asserted byte for byte.

**The quarantine is durable.** `index/state.json` gains `quarantined`, a
list of `{id, why}`, written by `write_index` — the file this store already
rewrites at every start and after every change. `#[serde(default)]`, so
**no version number moves**: `INDEX_VERSION` stays 1, `LAYOUT_VERSION`
stays 1, `RECORD_VERSION` is untouched, and an index written before this
field reads as a hub that set nothing aside. It is evidence, never the
decision: the record is re-read at every start, which is the only way a
repaired record or loosened rules can let a job out, so a stale entry is
overwritten by the start that disagrees. Nothing reads it back.

**The one case recovery cannot answer is answered too.** A record that
stops reading *after* the hub started — not by this hub's hand, which
publishes `job.json` by `rename` after an `fsync`, but an operator or a
failing disk can — used to fall out of `unfinished_jobs()` in silence,
because that walk already skipped anything that was not `Stored::Read`. It
now goes through `set_aside_now`: onto the quarantine list, into the index,
and onto stderr **once**, not once per dispatch pass (D151).

Files changed, and nothing else:

- `herd-hub/src/store.rs` — owned exclusively, as the brief says.
- `herd-hub/tests/crash_points.rs` — ORCH-080's 308-state census. It
  **asserted the defect**: `open_refused()`, `assert!(!quarantined, "D52's
  quarantine is for labels only")`, and two findings named
  `a_torn_record_refuses_the_whole_state_root` and
  `one_torn_record_takes_every_other_job_with_it`. Those expectations are
  what V10 changes, so they are rewritten rather than deleted: the census
  now asserts `refusals == 0` and `set_aside == 4 * 11 * 4`; the two
  findings are
  `a_record_this_hub_cannot_read_quarantines_one_job_and_the_hub_starts`
  and `one_torn_record_costs_neither_of_the_two_healthy_jobs_beside_it`.
  No assertion was weakened to pass: each one was replaced by the opposite
  claim about the same constructed state, and the four damaged record
  shapes now exercise the events and temporary axes too (176 states that
  used to stop at the open).
- `herd-hub/tests/durability.rs` — one new `#[tokio::test]`, appended after
  D52's own case, which it is modelled on.

`herd-hub/src/server.rs` is **not touched**, so the two sibling tasks in it
have no overlap with this branch at all.

## Not done, and why

- **No entry in `docs/decisions.md`.** The brief did not ask for one and
  only `herd-merger` writes that file. Proposed text is under *Decisions
  needed*; this is a behaviour change to D52's scope and should be recorded.
- **`docs/remediation/13-crash-point-enumeration.md` is now stale** and is
  not edited: §4.2's heading, the figure `176 refuse the whole state root`
  in its §3 table (line 201), its §5 point 5 (line 477), the two test names
  at lines 48-49, and `docs/remediation/README.md:42`'s row all state the
  refusal as current behaviour. That dossier is the one that **proposed,
  without making, that a parse failure quarantine the record the way a
  rejected label set already does** — this branch makes it. A finding, not
  an edit: the dossiers are a record of an investigation and several
  siblings are in flight.
- **`usable_index` is unchanged.** A quarantined record still sets the whole
  index aside, so a root holding one pays a full record walk at every start
  (`collections_owed`-shaped: 0.87-1.31 s per 100 000 records, D152's
  figure, via `walk_cost.rs`). That is the existing D52 rule and the brief
  forbids touching the queue; noted as a cost, not a defect.
- **No board row written**, as instructed; proposed below.

## Gates

Run before this file was committed, on the same code:

```
$ nix develop --command cargo fmt --all --check
(no output)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.57s

$ nix develop --command cargo test -p herd-hub --lib
test result: ok. 208 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ nix develop --command cargo test -p herd-hub --test durability
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ nix develop --command cargo test -p herd-hub --test crash_points -- --ignored --test-threads=1
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

308 states built
  0 refuse the whole state root at `JobDirStore::open`
  176 open with the damaged record quarantined by name (V10)
  44 open with the job silently gone from the queue
  4 leave `events.jsonl` AHEAD of `job.json` (D103's one rule)
  231 leave a stale temporary that nothing removes

$ ./scripts/check-orchestration.sh
check-orchestration: 269 passed, 0 failed
```

The baseline the brief named — 269 passed / 0 failed at five consecutive
landings — is unmoved, measured on this tree.

**The last run**, on the tree with this file in it: `./scripts/gate.sh`,
bare, from the worktree root. Its output goes to the gate's own log and
receipt under `.gate/` (the path the run prints), and the verdict line is
quoted verbatim in the agent's report, not here — this file is in the tree
the gate is about.

Skips: named in the agent's report from that run's skip list, never as a
count (D27). The two flood benches decline above a one-minute load of 8 and
print the figure they read; this host is shared with several sibling builds.

## Risks and findings

1. **`herd-hub/tests/crash_points.rs` expectations were rewritten.** This is
   the one thing a reviewer should look at first. It is not a test edited to
   go green: the file's whole purpose is to state what the hub does with each
   of 308 constructed states, and 176 of those answers are what this branch
   changes. Every rewritten assertion asserts the opposite claim about the
   same state, and the census asserts the new figure rather than printing it.
2. **A transient I/O error on one `job.json` now quarantines that job for the
   life of the hub** instead of failing the read. The reason names the `io`
   error, so it does not read as a rules judgement, and the next start
   re-reads the record — but a hub that is already running will not pick the
   job up again. The alternative was the old behaviour, which was a dead hub.
3. **`retention`'s preview and the hub now classify the same record
   differently**: the preview puts an unreadable `job.json` on `scanned.unread`
   (`retention.rs:530-600`) and the hub puts it on the quarantine list. Both
   are honest and neither is wrong — the preview has its own reader by design
   — but an operator comparing `herd-hub --retention-preview` with the hub's
   startup lines will see one record under two names. `retention.rs` is not
   touched; `NotOurs::Quarantined`'s doc ("the record is readable and …") is
   now narrower than the hub's quarantine.
4. **`Judged::of`'s doc claim still holds** ("a quarantined record never
   becomes a `JobRecord` at all"), and so does `select`'s precondition: a
   record that does not parse never reaches retention's judgement.
5. Nothing in the tree reads `index/state.json`'s new field, so a reviewer
   who wants it believed rather than merely written must say so; the field's
   doc argues for keeping it unbelieved.

## Decisions needed

- **Record the behaviour change?** Recommendation: yes, a `D-NEXT` entry by
  `herd-merger` at integration, along the lines of: *"A `job.json` the hub
  cannot read quarantines that one job instead of refusing the state root.
  D52 quarantined a record whose labels today's rules reject; the same
  answer now covers a record that is torn, empty, NUL-filled, of a record
  version this build does not read, or whose bytes the filesystem will not
  return. The record is named with its path at every start, counted,
  written to `index/state.json`, kept out of the queue and never rewritten.
  What still refuses a whole state root is a root the hub cannot enumerate.
  No format version moves."* Fallback: no entry, and the behaviour lives in
  `store.rs`'s doc comments and this handoff alone — which would leave the
  remediation dossier's proposal looking unanswered.
- **An `ORCH-nnn` for this work**, since "V10" is taken twice on the board.

**Proposed board row** (for the orchestrator or `herd-merger` to write, not
written here):

```
| ORCH-nnn | A `job.json` the hub cannot read made `JobDirStore::open` fail, which stops `main()` seventy-six lines before `Hub::bind_with`: one damaged record held every healthy job on the state root hostage and a container restarted into the same wall. Live on the first consumer's machine, unrepairable from outside the container (D161) | V10 brief, 2026-10-29 | herd-step, then herd-merger | owed (branch `v10-quarantine` pushed) | An unreadable record now takes the quarantine path a rejected label set already took (D52): named with its path, counted, written to `index/state.json`, out of the queue, record untouched. ORCH-080's 308-state census goes from `176 refuse the whole state root` to `0`. No version number moves. Residue: `docs/remediation/13-crash-point-enumeration.md` §4.2 and `README.md:42` still state the refusal | — | `history/2026-10-29-v10-quarantine.md` |
```

## Resume point

`herd-merger` takes branch `v10-quarantine` into the batch's integration and
runs `./scripts/gate.sh` bare on the gated sha. Nothing else is owed on this
branch. To see the evidence again, from a worktree of it:

```sh
nix develop --command cargo test -p herd-hub --test crash_points \
  -- --ignored --nocapture --test-threads=1 every_crash_point
```

## Processes

PIDs started and ended: only `cargo`/`nix develop` children of this
session's own `bash` calls, each of which exited on its own; none killed, no
`pkill`, no kill by name. Nothing was started on the consumer's instance and
nothing outside this worktree and this session's scratchpad was written.

Own build tree: `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/v10-quarantine`,
outside the worktree and off the root filesystem, deleted once the branch was
pushed (the agent's report says so). The gate's receipts are kept.
