# store-guards — handoff 2026-10-30

- **Item:** the audit of 2026-10-30, findings A1 and A2
  (`2026-10-30-audit-one-source-of-truth.md`)
- **Agent:** herd-step
- **Branch / commit:** `store-guards` / `d6bbc97`, based on
  **`origin/pack-registry` `6f409cb`** — not on `main` — with
  `origin/main` `351c826` (batch 11) merged in at `8294d76`. The merge was
  clean: no conflict, 48 files, 10850 insertions, 631 deletions, nothing
  in `herd-hub/src/store.rs`.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`.
  This branch is a **descendant of `pack-registry`**, so landing it lands
  that branch's pack registry, `RECORD_VERSION` 2 and `LAYOUT_VERSION` 2
  as well.

## Done

Two defects closed in `herd-hub/src/store.rs`, one commit each, both able
to lose an operator's bytes while every test stayed green.

**1. `7bc9568` — the one join of a record-supplied path that asked
`herd_proto::check_relative` nothing.** `collection_is_whole` now asks it
before the join, short-circuiting with `&&`, so a path this hub will not
write is a file that can never be whole and nothing outside the
collection's directory is so much as measured. It is the join whose
answer destroys bytes: `collection_is_whole` is what `collection_state`
and `collection_owed` read to tell a runner its collection has landed,
and the runner then drops its only copy (D19, narrowed by D221).

**2. `d6bbc97` — a frame that omits `sha256` turned verification off.**
`write_output_chunk` compared under
`!entry.sha256.is_empty() && real != entry.sha256`; `OutputFile.sha256`
is `#[serde(default)] String`, so a frame that **omits the key**
deserialises to `""`, the condition was false whatever arrived, and every
chunk of that file was renamed into `files/` unverified — five lines
under the comment promising that verifying before the rename is what
makes `files/` "a directory of whole, verified files and nothing else"
(E-53).

**The decision on the empty digest, and its words.** The refusal is **of
the bytes, by name, before one of them is written** — beside the existing
`check_relative` refusal in `write_output_chunk`, not at the rename. The
sentence is:

```
{path}: this job's record names it with no sha256, and this hub does not write
bytes it cannot verify. The {bytes} byte(s) stay with the runner: a manifest
entry with an empty digest was edited on this hub or sent by a peer that
omitted the field, so the job has to be run again rather than resent
```

Why a refusal and not "accepted into a state that is visibly not
verified": a third state would have to be carried on the record, read by
the sweeper, answered to a requester and explained to an operator — four
surfaces, for a shape **no `herd-runner` produces** (the streamed
carriage's only manifest producer is `herd-runner/src/outputs.rs:202`,
which announces a digest computed by `hash_file`). Refusing keeps one
meaning for `files/` and puts the problem where somebody can act on it,
in the runner's log, naming the file. Refusing the bytes is **not**
refusing the entry (D175): the manifest entry stays on the record, the
collection stays owed, the runner keeps its only copy — asserted, both.

Why before the first byte rather than at the rename: the far end would
otherwise send the whole file to have it thrown away, and a refusal at
the earliest point is the same answer for less work.

**The comparison before the rename is now unconditional** (`real !=
entry.sha256`). That is a removal of a special case, not a second guard:
with the empty case refused above there is nothing for an `is_empty()`
here to excuse, and if the early guard is ever deleted this comparison
refuses the empty digest too, so the arrangement fails closed either way.

**Does the refusal reject a legitimate streamed collection? No, and it is
checked rather than argued.** `KeptFile::sha256` (`herd-hub/src/job.rs`),
`write_output_chunk` and the streamed carriage **all arrived in one
commit** — `d114f03`, 2026-10-16, "move the protocol to 2 and give an
output two carriages" (`git log -S` on each of the three). So a record
written before the digest existed carries no streamed collection and has
no chunks to send. `KeptFile::sha256`'s own "empty on a record written
before this existed" is true of **inline** records, whose bytes never
reach this function.

**Two tests, both in `herd-hub/src/store.rs`'s `mod tests`:**

- `a_record_whose_path_climbs_out_is_never_judged_whole` — a record
  written to the disk with `../../../loot.bin` as its one manifest path,
  and **a real file of exactly that entry's length at the escaping join's
  target**. The fixture is asserted first, *through the very join the
  guard protects* (`file_length(&kept.join("../../../loot.bin")) ==
  Some(10)`), so the `false` verdict that follows is proof the guard
  produced it and not the filesystem — which is how "nothing outside the
  collection directory was read" is established without an I/O tracer.
  Then the collection is asserted **still owed**. The known-good half, in
  its own store, writes an ordinary `files/results.json` and asserts
  `Complete` and `collection_is_whole`.
- `a_chunk_whose_record_carries_no_digest_is_refused_and_not_kept` — the
  frame is **deserialised from JSON with the `sha256` key left out**, and
  `frame.files[0].sha256.is_empty()` is asserted as the premise, so the
  test proves `#[serde(default)]` is the way in rather than assuming it.
  Then `Refused`, three assertions on the refusal's words, `files/` empty,
  the manifest entry still on the record, the collection still owed. The
  known-good half, in its own store, lands a chunk whose digest matches
  and reads the bytes back out of `files/`.

Each test holds the bad case **and** its known-good case, deliberately:
that is what makes the demonstration below fail *both* tests.

**Remove-the-check-and-watch-it-fail, run.** Both production guards taken
back out (the `check_relative` call restored to the old one-liner, the
digest refusal block deleted, the comparison restored to
`!entry.sha256.is_empty() && real != entry.sha256`), tests untouched:

```
test store::tests::a_record_whose_path_climbs_out_is_never_judged_whole ... FAILED
test store::tests::a_chunk_whose_record_carries_no_digest_is_refused_and_not_kept ... FAILED
panicked at herd-hub/src/store.rs:5754:9:
a path that climbs out of the job's output directory can never be whole, whatever is out there at the right length
panicked at herd-hub/src/store.rs:5836:13:
bytes nothing can verify must not be written into files/
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 255 filtered out
```

The file was then restored and is **byte-identical**:
`sha256 de477ffd189df7ca12f480824c87a64bcc42d6e6991b7fe101e23a84ce54351a`,
measured before the removal and after the restore, and that is the file
`d6bbc97` carries.

**`cargo test -p herd-hub --lib` before anything was written** (the tree
at `8294d76`, `herd-hub/src/store.rs` restored to its committed state for
the run): `test result: ok. 255 passed; 0 failed; 0 ignored; 0 measured;
0 filtered out; finished in 9.04s`, exit 0, **no `HERD-SKIPPED` line in
that target**. The test target compiled, so the figure is evidence about
tests and not about `cargo check`. After the change: **257 passed**, the
two new ones named.

## Not done, and why

- **The digest *shape* check.** `herd-proto` is not this task's to edit,
  and `"zz"` or a 63-character digest is still a well-formed frame today.
  Drafted as a board row below. The hub now refuses the one case that
  disabled verification outright (an **empty** digest); a malformed
  non-empty digest is accepted onto the record and then fails every
  comparison, which is loud rather than silent.
- **`collection_is_whole` still has no digest condition.** It judges a
  file whole on its path and its length. Extending it to "and the record
  names a digest" would change behaviour for **inline** records written
  between `a479643` and `d114f03`, which legitimately carry empty
  digests, and those records reach `collection_is_whole` by paths that
  have nothing to do with chunks (`store.rs` sweep and retention
  callers). Out of this brief, and it wants its own measurement.
- **`docs/operating.md` on the `files whose record carries no digest`
  line.** `docs/*` is outside this task's paths. Drafted as a board row.
- **The audit's finding 0 and A3–A10.** Not this brief.

## Gates

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/store-guards`. Each heavy phase
behind **two** `./scripts/bench-preflight.sh` clears 20 s apart, and the
first two clears of the night were *not* acted on — see *Risks and
findings* 4.

Runs made **before** this file was committed, on the tree of `d6bbc97`:

```
=== build ===   nix develop --command cargo build --workspace
build EXIT=0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 26.44s

=== suite ===   nix develop --command ./scripts/test.sh
suite EXIT=0
test suite: PASSED
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
(herd-hub lib target: test result: ok. 257 passed; 0 failed; 0 ignored)

=== clippy ===  nix develop --command cargo clippy --workspace --all-targets -- -D warnings
clippy EXIT=0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.41s

=== fmt ===     nix develop --command cargo fmt --all --check
fmt EXIT=0

ALL FOUR RUST GATES GREEN
```

`./scripts/check-docs.sh` → `docs ok`, exit 0 (run on the tree before
this file; run again with it, in the agent's report).

**Skips: 12, and not one of them is counted as a pass** (D27). Their
assertion names are **identical, as a set, to
`scripts/expected-skips/local.txt`** (`diff` of the names: no output).
All twelve are cgroup v2 delegation or a hard IO ceiling this host cannot
carry; none touches `herd-hub/src/store.rs` or anything this branch
changed. One `#[ignore]` in the `spool_offer_answered` target and the
14 `ignored` in the two cost-surface targets are the suite's own standing
shape, unchanged by this branch.

**The last run, named and not quoted:** bare `./scripts/gate.sh` — never
inside `nix develop` — on the committed tree including this file, with
`env -u HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` (ORCH-209), its output to
`…/scratchpad/store-guards/gate-sh.log`. The verdict is in the agent's
report. The branch is pushed only on a green one.

## Risks and findings

1. **The audit's A1 reachability is *narrower* than it said, and this is
   the line it asked for.** A1 names two shapes that can carry an
   escaping path into `collection_is_whole`: a hand-edited record, and "a
   record written by a build from before D131". **The second does not
   exist.** `KeptFile`, `record_outputs`'s intake `relative_path` check
   and `herd_proto::check_relative` itself all arrived in **one commit**,
   `a479643` (2026-10-15, "bring a job's declared output home with its
   verdict") — `git log -S` on each. A record written by a build with no
   intake check therefore has no `outputs` key at all: it deserialises to
   an empty file list, and `collection_is_whole` iterates nothing. So the
   only shape that reaches the unguarded join is **a record edited on the
   hub's disk**, which is exactly the shape `store.rs` already says it
   does not trust. The defect was real and is closed; its probability is
   one shape, not two.
2. **A2's premises all hold, and one of its arguments is stronger than it
   claimed.** Re-measured: `len() == 64`, `len() != 64` and "sixty-four"
   in a digest context return **zero** hits over `git ls-files '*.rs'`
   (the only two "sixty-four" hits are about connections and megabytes).
   `report.rs:444-445` does count `undigested_files` / `undigested_bytes`
   and `:765` does print the line; `docs/operating.md` does not mention
   it. And the shape A2 feared — a legitimate frame reaching the guard
   with no digest — **cannot come from a `herd-runner`**: the streamed
   carriage's one manifest producer always hashes
   (`herd-runner/src/outputs.rs:202`), and the carriage and the digest
   field are the same commit. So refusing is cheaper than A2 assumed: it
   can break nothing that works today.
3. **The audit's "nine joins, eight guarded" does not reproduce as
   written, and the defect does.** On `origin/main`, `rg` for a join of a
   record-supplied path in `herd-hub/src/store.rs` matches **9** lines —
   but one of them (`store.rs:6607` on `main`) is inside `mod tests`. In
   production code there are **8**, of which **7** asked `check_relative`
   and one did not. A1's own list of "eight guarded" then names five
   `store.rs` sites plus four sites in `server.rs`, `herdctl` and the
   runner, which is a different population again. Nothing in the finding
   turns on the arithmetic: the one unguarded join is where A1 says it
   is, in the function A1 says it is, and it is now guarded.
4. **The preflight's two clauses were met twice at the start of the
   night and starting there would have been wrong.** First two readings,
   25 s apart: `ok, safe to start`, with slice room **9335** then
   **9359 MiB** over an 8192 MiB floor — about 1.1 GiB of headroom
   against a suite that grows ~4 GiB in the same cgroup. Waited instead,
   polling. Three minutes later the same script read room **8255**, then
   **8195**, then **8006 MiB** — i.e. **below its own floor**: a run
   started on that cleared knife edge would have been building while the
   slice went negative. The phases here were run at room 12.4–13.8 GiB,
   margin ≥ 4.2 GiB. `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set,
   exported or passed anywhere; the extra wait was a stricter condition
   on top of the script's, never a lifted one. **Worth making mechanical:
   the script's own note says "all four figures are of this instant
   only", and tonight that was not a caveat, it was the measurement.**
5. **This branch is a descendant of `pack-registry`, so it cannot be
   landed without it.** The two commits touch only
   `herd-hub/src/store.rs` and only in places `pack-registry` does not,
   so a cherry-pick onto `main` would apply — but the gated tree is this
   one, and a cherry-picked tree is a tree nothing gated.
6. **No record or protocol shape moves.** No new field, no version
   number, no frame change. The refusal is a new `ChunkLanded::Refused`
   sentence, which `job.rs`'s own doc comment on that variant already
   describes as "each is named rather than retried silently".
7. **A runner facing the refusal re-offers rather than giving up.** The
   collection stays owed, so the hub keeps asking and keeps refusing, and
   the spool is **not** condemned (D221's barrier needs an answered
   silence, and this is an answer). That is the safe direction — bytes
   kept, not destroyed — and it is bounded by the re-offer's own cost
   (ORCH-205: about two chunks per collection). It is visible in the
   runner's log by name, which is what makes it an operator's problem and
   not a silent loop.

## Drafted for `herd-merger`

Two `docs/decisions.md` entries and two `board.md` rows, in full.

### Decision entry one

```markdown
## 2026-10-30 — D-NEXT The one relative-path check is asked where a collection is judged whole

**Context.** `herd_proto::check_relative` is "the one check, used by the
runner that produces the path and by the hub that writes the file, so a
path cannot be judged acceptable at one end and dangerous at the other
(invariant 4)". Eight joins of a record-supplied path onto a job's
directory exist in `herd-hub/src/store.rs`'s production code; seven asked
it and `collection_is_whole` did not — the one whose answer destroys
bytes, because it is what tells a runner its collection has landed and the
runner then drops its only copy (D19, D221). A record whose manifest path
climbs out with `..` was measured against a file **outside** the job's
directory, and a stranger that happened to be the manifest's length read
as whole.

**Choice.** `collection_is_whole` asks `check_relative` before the join,
short-circuiting, so nothing outside the collection's directory is so much
as measured, and a path this hub will not write is a file that can never be
whole — the collection is never acknowledged and the runner keeps its bytes.

**Reachability, narrowed by measurement.** `record_outputs` refuses such a
path at intake, and that check arrived in the **same commit** as the
record's `outputs` list and as `check_relative` itself (`a479643`,
2026-10-15, D131) — so a record written by a build with no intake check has
no `outputs` at all and iterates nothing. The audit that found this named a
second shape, "a record written by a build from before D131"; that shape
does not exist. The only one that reaches the join is a record **edited on
the hub's disk**, which `store.rs` already says it does not trust: "the
record is a file an operator may have edited, and a path read off a disk is
no more trustworthy than one read off a socket". Probability one shape,
cost unrecoverable, price one function call.

**Evidence.** `herd-hub/src/store.rs` `collection_is_whole` and
`a_record_whose_path_climbs_out_is_never_judged_whole`, whose fixture puts a
real file of exactly the manifest's length at the escaping join's target and
asserts through that join that it is there — so the `false` verdict is proof
the guard, not the filesystem, produced it, and the guard's removal reddens
the test; **D19**, **D131**, **D175**, **D221**; invariant 4;
`history/2026-10-30-store-guards.md`.
```

### Decision entry two

```markdown
## 2026-10-30 — D-NEXT A chunk whose record names no digest is refused, never renamed

**Context.** `herd-hub/src/store.rs`'s `write_output_chunk` compared the
bytes it had just finished receiving against the manifest's digest under
`if !entry.sha256.is_empty() && real != entry.sha256`.
`herd_proto::OutputFile.sha256` is `#[serde(default)] String`, so a verdict
frame that **omits** the `sha256` key deserialises to `""`, the condition is
false whatever arrived, and every chunk of that file was renamed into
`files/` **unverified** — five lines under the comment promising that
verifying before the rename "is what makes `files/` a directory of whole,
verified files and nothing else" (E-53). Nothing in the workspace checks a
digest's shape either: `len() == 64` and `len() != 64` return zero hits over
all tracked `.rs` files. The hub already **counted** the condition —
`report.rs:444-445` feeds `undigested_files` / `undigested_bytes` and `:765`
prints it — so the detector existed and the enforcer did not.

**Choice.** The empty digest is a **refusal of the bytes, by name, before
one of them is written**, beside `write_output_chunk`'s `check_relative`
refusal: *"{path}: this job's record names it with no sha256, and this hub
does not write bytes it cannot verify. The {bytes} byte(s) stay with the
runner: a manifest entry with an empty digest was edited on this hub or sent
by a peer that omitted the field, so the job has to be run again rather than
resent"*. The comparison before the rename becomes unconditional
(`real != entry.sha256`), which removes a special case rather than adding a
guard: if the early refusal is ever deleted, the comparison refuses the
empty digest too, so the arrangement fails closed either way.

**A refusal and not a third state.** "Here but not verified" would have to
be carried on the record, read by the sweeper, answered to a requester and
explained to an operator — four surfaces, for a shape no `herd-runner`
produces: the streamed carriage's only manifest producer
(`herd-runner/src/outputs.rs:202`) announces a digest computed by
`hash_file`. Refusing keeps one meaning for `files/` and puts the problem in
the runner's log, naming the file.

**Refusing the bytes is not refusing the entry** (D175). The manifest entry
stays on the record, the collection stays owed, and the runner keeps its
only copy — the direction D19 and D221 say to err in. The cost is a re-offer
the hub refuses again, bounded by ORCH-205's measured two chunks, and
visible by name.

**It cannot stall a legitimate transfer.** `KeptFile::sha256`,
`write_output_chunk` and the streamed carriage all arrived in **one** commit
(`d114f03`, 2026-10-16, protocol 2), so a record written before the digest
existed carries no streamed collection and has no chunks to send.
`KeptFile::sha256`'s "empty on a record written before this existed" is true
of **inline** records only, and an inline record's bytes never reach this
function.

**Residue.** The digest's *shape* is still unchecked anywhere, and it
belongs in `herd-proto` beside `MAX_LABEL_LENGTH`'s model (ORCH-NEXT below).
`herd-hub report`'s `files whose record carries no digest` line is still
explained by no shipped document (ORCH-NEXT+1 below).

**Evidence.** `herd-hub/src/store.rs` `write_output_chunk` and
`a_chunk_whose_record_carries_no_digest_is_refused_and_not_kept`, whose
frame is **deserialised from JSON with the key left out** so the premise is
asserted and not assumed, and which reddens when the refusal is removed;
**D19**, **D148**, **D175**, **D221**; E-53, E-54;
`history/2026-10-30-store-guards.md`.
```

### Board row one — the digest shape check in `herd-proto`

```markdown
| ORCH-NEXT | **`OutputFile.sha256`'s shape is stated in a doc comment and checked nowhere.** The field is `#[serde(default)] String` and its rule — "the sha256 of the file, lowercase hex" — lives only in prose at `herd-proto/src/outputs.rs:178`. Measured over `git ls-files '*.rs'`: `len() == 64`, `len() != 64` and "sixty-four" in a digest context return **zero** hits, so `"zz"`, `"0"` or a 63-character digest is a well-formed frame today. The hub now refuses an **empty** digest at the chunk path (D-NEXT), which closes the one case that disabled verification outright; a malformed non-empty digest is still accepted onto the record and simply fails every comparison — loud, but by accident | `store-guards`, 2026-10-30, re-measured from the audit's A2 | herd-step | open | **The check belongs in `herd-proto`, not in the hub**: invariant 4 says the protocol is the contract, and `herd_proto::capability::MAX_LABEL_LENGTH` (`capability.rs:62-76`) is the same situation already done right — a constant, a predicate beside it, one refusal everyone quotes. Shape: `pub fn check_digest(&str) -> Result<(), String>` beside `check_relative`, sixty-four lowercase hex, asked by `OutputFile::disagreement`, by the hub at intake and by the runner that produces the manifest. It touches a frozen-format surface, so it wants its own branch and its own `frozen_formats` thought — and the same pass should fold A3's four implementations of "forty lowercase hex" (three byte-identical private copies in `herdctl`, one different predicate in `herd-hub/src/server.rs:6954-6958`) into the same crate, since it is one defect twice | **D-NEXT** (the empty-digest refusal) | `herd-proto/src/outputs.rs:178`, `:189`; `herd-proto/src/capability.rs:62-76`; `herd-hub/src/store.rs` `write_output_chunk`; the audit's A2 and A3; `history/2026-10-30-store-guards.md` |
```

### Board row two — the operator-facing undigested line

```markdown
| ORCH-NEXT+1 | **`herd-hub report` prints `files whose record carries no digest  N` and no shipped document explains it.** `herd-hub/src/report.rs:444-445` counts it and `:765` prints it; `docs/operating.md` documents `report` at length and never mentions it (zero hits for `undigested`, "no digest", "without a digest"). With D-NEXT's refusal in place a non-zero figure now means exactly one thing on a hub of this build: entries written before the refusal existed, or edited since — bytes in `files/` that nothing ever verified | the audit's B5, 2026-10-30, re-measured by `store-guards` | herd-step | open | One paragraph in `docs/operating.md` beside the rest of the `report` section: what the line counts, that on this build it is a historical figure rather than a live one, and what to do with it — re-run the job, because the digest cannot be recovered from the bytes (the record is what said what they should be, E-54). Prose only, so `check-docs.sh` and the batch's own gate | **D-NEXT** | `herd-hub/src/report.rs:444-445`, `:765`; `docs/operating.md`; `history/2026-10-30-store-guards.md` |
```

## Decisions needed

**none.** The one judgement the brief delegated — what the refusal is —
was made and is recorded above, in the code and in the drafted entry. The
two drafted board rows are work, not questions.

## Resume point

`herd-merger`, when this batch lands: `store-guards` is a descendant of
`pack-registry`, so take `pack-registry` with it or not at all. Gated sha
is the tip of `store-guards` at the green `./scripts/gate.sh`.

Next task, if the orchestrator wants the class closed rather than the two
cases: ORCH-NEXT above, `herd_proto::check_digest` beside
`check_relative`, with A3's "forty lowercase hex" folded into the same
pass.

## Processes

PIDs started and ended: none held. Every heavy phase was a foreground or
backgrounded `cargo`/`nix develop` child of this agent's own shell,
started and reaped by it; nothing was killed, and no `pkill`, `killall`
or kill-by-name was used. The first consumer's hub and runner (PIDs
3006543 and 3021002) were not touched, read or signalled.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/store-guards` — deleted once
the branch was pushed; the figure and the `rm -rf` are in the agent's
report. The worktree itself is
`…/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/store-guards/wt` and
is left for the merger to read.
