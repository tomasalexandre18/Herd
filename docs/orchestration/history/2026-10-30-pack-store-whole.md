# pack-store-whole — handoff 2026-10-30

- **Item:** M11's pack half, one decided repair on `pack-store`: finding 1 of
  `2026-10-30-pack-store-kept.md`, settled the way that file recommended.
  `ingest` refuses a declared directory that holds more than a pack, so the
  **success** path destroys nothing either.
- **Agent:** herd-step
- **Branch / commit:** `pack-store` / `5a6fccc` (code) plus this handoff, on
  top of `0f56d38`, based on origin/main `1a8c1ae`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger` names
  the landing here, and fills the record of `templates/landing.md`

## Done

### 1. The defect, closed where its own doc already claimed it was

`PackStore::ingest`'s doc said *"Anything else is refused by name rather than
guessed at"*. The code read the manifest, placed the `kind f` objects and
ignored every other name, and `leave_packs` then `fs::remove_dir_all`'d the
whole declared path. So a job that left `{out}/packs/build.log` beside its
pack had that log **deleted**, on the success path, by the module the previous
task had just taught not to delete on the failure path. The hard-linked
objects survived; nothing else did.

`nothing_beyond_a_pack(dir)` now runs **before anything is parsed or placed**,
and refuses any name directly under the declared directory that is not
`manifest.tsv`, `pack.meta` or `objects/`. The refusal travels the path that
already existed: `leave_packs` renames the whole directory to
`<data root>/packs/refused/<job>-<attempt>/<declared path>`, intact, and says
so. Nothing new was invented for it.

**Why refuse rather than remove only the three parts:** removing only the
three parts is Herd deciding, unasked, that a declared directory is part pack
and part output, and it weakens ORCH-153's *"gone from `{out}`, not excluded
from it"* into *"partly gone"*. The refusal keeps one rule with one outcome.

**The refusal, verbatim**, as the unit test's own fixture produces it (one
stray name beside a valid three-part pack):

> `<declared dir>` holds "build.log", which a pack directory does not have,
> and a pack directory is manifest.tsv plus optionally pack.meta plus objects/
> and nothing else. The whole of it is kept as it is rather than partly taken
> apart: which of those bytes are a pack's and which are the job's own output
> is the declaration's to say, and taking the three parts out of here would
> have removed the rest of it with them

and on the job's own `skipped` list and on stderr, composed with the sentences
that were already there — one vocabulary, not a second one:

> this job declared that it leaves "pack" at "packs" and that is not a pack
> this runner can keep: `<the refusal above>`. Its 4487 bytes (4.38 KiB) are
> kept as `…/packs/refused/j1-1/packs`: a declared artefact is never collected
> as the job's output, so it is out of the output directory — left where it
> is, one pack this runner cannot read would put the whole record over its
> output cap — and it is kept there for as long as it is on this disk. Nothing
> in this runner reads it, offers it or gives it to a job again, nothing here
> will ever remove it, and removing it is yours to do

Only the names **directly under** the declared path are read. What is inside
the three parts is their own business, and a pack whose `objects/` carries a
file no `kind f` row names is still the pack its manifest says it is — see
finding 1 below, which is the one hole this leaves open on purpose.

### 2. Two tests, and the demonstration was executed, not reasoned about

- `packstore::tests::a_directory_holding_more_than_a_pack_is_refused_
  and_the_same_one_without_it_ingests`
  — one fixture, both halves. A valid three-part pack with `pack.meta` and one
  `build.log` beside it is refused, the refusal is asserted to name the stray,
  the three parts and the keeping, and **nothing of it reaches the store**
  (`objects/` and `manifests/` both empty: refused before an object is placed,
  not half-ingested and then regretted). The same directory with that one name
  removed then ingests, its id is the sha256 of its manifest's own bytes, its
  object is in the store and `holds_whole` is true. A check that refused
  everything would pass the first half and fail the second.
- `outputs::tests::a_job_s_own_file_beside_a_declared_pack_is_kept` — the
  end-to-end, through `collect`. The log, the manifest and the object are each
  read back from `packs/refused/j1-1/packs` and compared byte for byte, which
  is what says **whole** rather than *kept minus the three parts*; the record
  carries `results.json` alone; `{out}/packs` is gone (ORCH-153, unchanged);
  and the store holds no object of it.

**Red when the check is removed.** `nothing_beyond_a_pack(dir)?;` commented
out, `cargo test -p herd-runner --lib -- packstore outputs`:

```
test packstore::tests::a_directory_holding_more_than_a_pack_is_refused_and_the_same_one_without_it_ingests ... FAILED
test outputs::tests::a_job_s_own_file_beside_a_declared_pack_is_kept ... FAILED

---- outputs::tests::a_job_s_own_file_beside_a_declared_pack_is_kept stdout ----
panicked at herd-runner/src/outputs.rs:1199:46:
the job's own file is still on this disk: Os { code: 2, kind: NotFound, message: "No such file or directory" }

---- packstore::tests::…_is_refused_and_the_same_one_without_it_ingests stdout ----
panicked at herd-runner/src/packstore.rs:1866:38:
that is more than a pack: Held { manifest: PackManifest { id: "8bac80e6…", bytes: 321, rows: 3, files: 1 }, meta: Some([…]), objects: 1, objects_added: 1, bytes_linked: 24, bytes_copied: 0 }

test result: FAILED. 28 passed; 2 failed; 0 ignored; 0 measured; 197 filtered out
```

That `NotFound` **is the defect**: the pack ingested, the declared directory
was removed whole, and the job's log was not on the disk to read. The
kept-bytes assertions were deliberately moved to the front of the test so the
demonstration fails on that line and not on a bookkeeping assertion before it.
Both files were restored and are byte-identical afterwards:
`packstore.rs` sha256
`bc5d4af24d258723c60a4fea7558bc1e3065d54a92d41b14001aad2d0ad98f92`, and
`outputs.rs` sha256
`39465380fe78673d41aa2b74f214689efa724deb161bfb912324c00536a38f8d`.

### 3. The test target compiles its tests on this tree

Checked **before** anything was written, because the predecessor's only
command was `cargo check -p herd-runner --lib`, which does not compile
`#[cfg(test)]` code and hid twelve unreachable tests behind an `E0061`:

```
$ nix develop --command cargo test -p herd-runner --lib -- packstore outputs
test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 197 filtered out; finished in 0.11s
```

30 passed on the committed tree, the two new tests being the difference.

### 4. Three of the four one-liners, and a fourth that is a doc

1. **Taken.** `lay_out`'s refusal path no longer subtracts two independently
   computed lengths: `needed.len().saturating_sub(missing.len())`. The
   previous task's demonstration died in `panic_const_sub_overflow` *inside*
   the refusal, so nobody read the sentence.
2. **Taken, doc only.** `place_object`'s *"nothing else knows that name"* was
   stronger than the code: on the hard-link path the staged file is the same
   inode as the job's file under `{out}`, so the job has a second name for it.
   The doc now says the staging name is the only name **inside this store**,
   that what makes the hash trustworthy is the job's processes being gone
   before `leave_packs` runs, and what a later caller that ingested a live
   directory would have to do instead.
3. **Taken, and it was one line.** `apparent_bytes` is now called in the one
   match arm whose sentence carries the figure, so a 557-file pack is no
   longer walked on the success path for a figure nobody reads. Still measured
   before anything moves, because arguments are evaluated before the call, and
   the comment says so.
4. **Taken as a doc, behaviour unchanged as instructed.** `REFUSED_DIR` now
   says that a kept pack keeps the modes the job left it — nothing under
   `refused/` restores permissions, the record walk does that under `{out}`
   and the reaper under `jobs/` — so a pack whose job chmodded its directories
   to `0000` is kept shut and its owner may need `sudo` to read their own
   bytes. It is in the doc and not in the refusal sentence: a mode caveat on
   every refusal, including the ones where modes are fine, would be noise in
   the line an operator actually reads.

### 5. The semantic merge conflict, routed mid-task

`packstore::tests::an_object_name_that_is_not_a_digest_is_refused_
rather_than_joined_onto_a_path` asserted
`why.contains("sixty-four lowercase hex")` — `packstore::object_name`'s own
spelling. On a tree that merges `pack-variants`, the bad row is refused
earlier by `herd_proto`'s `check_object_digest`, whose sentence spells the same
rule *"64 lowercase hex digits"*: right behaviour, red test, and green on each
parent alone.

It now asserts the property instead — that the refusal names the value it will
not take (`"escaped"`) and the shared half of the rule (`"lowercase hex"`) —
with the two other assertions unchanged: nothing was joined onto `objects/`,
and upper case is refused too. The test's doc records why the spelling of
sixty-four is not what it is about, so nobody tightens it back.

**Verified on this tree**, which is the only tree I can run:

```
$ nix develop --command cargo test -p herd-runner --lib -- an_object_name_that_is_not_a_digest
test packstore::tests::an_object_name_that_is_not_a_digest_is_refused_rather_than_joined_onto_a_path ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 226 filtered out; finished in 0.00s
```

Here the refusal still comes from `packstore::object_name`, whose sentence
reads *"an object's name is sixty-four lowercase hex characters"* and therefore
contains `lowercase hex`. The merged tree's sentence contains it too. I could
not run the merged tree and do not claim to have.

## Not done, and why

1. **`object_name` is still a second sentence for `herd-proto`'s rule.** The
   real repair is one rule with one wording, and it cannot be made here:
   `check_object_digest` does not exist on this branch and the code would not
   compile. Board row drafted below.
2. **Nothing beyond the top level of a declared directory is checked** — a
   file under `objects/` that no `kind f` row names still goes with the pack
   when the pack ingests. Finding 1 below; refusing on it would refuse real
   consumer packs whose tool left an object behind, which is the
   "check that refuses everything" failure in the other direction.
3. **`herd-proto`, `herd-hub`, `herdctl`, `lib.rs`, `sweep.rs`, `reclaim.rs`,
   `scripts/`, `decisions.md`, `board.md`, `plan.md`** — all on the brief's
   do-not-touch list and untouched. `docs/plan.md`'s stale sentence about the
   pack path is still stale.
4. **The removal delay and the archive path** for a kept pack: still parked
   for Alexa, still not a precondition for this branch.
5. **No frame, no socket, no fetch, no registration**, and `declared_leaves`
   still returns an empty map: unchanged.

## Gates

All four Rust gates were run on the committed code, **before** this handoff
was written, with `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-store` and
`./scripts/bench-preflight.sh` clear before each heavy phase.

```
$ nix develop --command cargo fmt --all --check
(no output)   exit 0

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.68s
exit 0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.59s
exit 0

$ nix develop --command ./scripts/test.sh
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
exit 0

$ ./scripts/check-docs.sh          # on the tree before this file
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit 0
```

**Skips: 12, every one the same host shape, none of them new and none of them
masking this change.** All twelve are cgroup v2 delegation (`this runner
cannot create a cgroup under /sys/fs/cgroup/chatons.slice/…`, Permission
denied):
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
`a_declared_ceiling_changes_what_the_inert_weight_line_says`,
`a_job_cannot_fork_past_its_own_pid_ceiling`,
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`,
`a_job_s_record_carries_the_pressure_it_ran_under`,
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`,
`an_io_weight_that_cannot_bite_is_said_out_loud`,
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`,
`no_declared_ceiling_leaves_io_max_empty`,
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`,
`the_runner_says_which_memory_ceiling_it_enforces`. The previous task's run
had those twelve plus the two flood benches (`flood-fifty`,
`flood-two-fifty`); this host was quiet enough for the benches to run, so they
are not skipped here. A `HERD-SKIPPED` line is a skip and never a pass (D27).

**The last run, not quoted here** because it is made on the tree this file is
committed into: bare `./scripts/gate.sh` (never inside `nix develop` — it
nests `TMPDIR` and panics `confinement.rs` on the 108-byte `SUN_LEN`), whose
output goes to the agent's report and to
`<scratchpad>/pack-store/gate-whole.log`. The branch is pushed only on a green
verdict; a red one is reported red and nothing is pushed.

## Risks and findings

### 1. The hole this leaves on purpose: inside `objects/`

Only the names directly under the declared directory are checked. A file
under `objects/` that no `kind f` row names is still removed with the pack
when the pack ingests, so the destruction path is narrowed and not closed.
Deliberate: `objects/` is the pack tool's own directory, and a pack whose tool
left an object behind — a row deleted from a manifest, an object staged for a
`kind d` row — is still the pack its manifest says it is, so refusing on it
would refuse real consumer packs. If that is to be closed too, the honest
shape is a refusal naming the object that no row claims, and it wants the
first consumer's own pack measured against it before it ships.

### 2. The runner's digest check and `herd-proto`'s are two sentences for one
rule, and they have **already** drifted

Not a risk, a measurement: the drift surfaced as a red test on a third tree
(item 5 above). This is the sixth time in one night that a rule written in two
places, or written in prose and enforced nowhere, has been found on this
project — and this one did not merely risk drifting. Row drafted below.

### 3. Nothing in the previous handoff's findings was refuted by the code

Findings 1 to 6 of `2026-10-30-pack-store-kept.md` all read correctly against
the tree. Finding 1 is what this task settled; 2, 3 and 4 are taken above; 5
is a doc now; 6 was its author's re-read and still holds. One correction of
emphasis, not of fact: finding 2 said `lay_out` can panic and named it
`lay_out` in the heading and `packstore.rs:433` in the body — the line is
inside `lay_out`, and the previous file's *Risks* heading 2 says `lay_out`
while its own decision list called it `packstore.rs:433`. Same place.

### 4. What a reader of the refusal cannot tell

The refusal names at most three stray names and then counts the rest
(`STRAYS_NAMED`). An operator with four hundred stray names reads three and a
count, and has to look at the kept directory for the rest — which is where the
sentence already sends them.

## Decisions needed

1. **Finding 1 — whether `objects/` is checked too.** *Recommendation:* leave
   it, and measure the first consumer's own 557-file pack against a draft
   refusal before deciding; a check here that is wrong refuses every real
   pack. *Fallback:* refuse an `objects/` entry that no `kind f` row names,
   which closes the path entirely at the cost of that risk.
2. **Board row, for after the batch lands** (the merger allocates, and this is
   not an edit to `board.md`, which is on my do-not-touch list):

   > **The runner's digest check delegates to `herd-proto`'s, so one rule has
   > one sentence.** `packstore::object_name` and
   > `herd_proto::pack::check_object_digest` enforce the same rule — 64
   > lowercase hex characters before the column is joined onto a path — in two
   > wordings. They have already drifted: on a tree holding both `pack-store`
   > and `pack-variants` the row is refused by the second sentence, which
   > spelled "64 lowercase hex digits" where the first spells "sixty-four
   > lowercase hex", and a test pinned to one speaker went red although the
   > behaviour was right. Needs a tree that holds both branches, so it cannot
   > be done before the landing. A rule in two places does not merely risk
   > drifting here — it has drifted, and the drift surfaced as a red test in a
   > third place.

### Drafted decision entry

**This supersedes only the first paragraph of the drafted entry in
`2026-10-30-pack-store-kept.md`** — the one beginning *"A declared pack leaves
`{out}` whether or not it could be stored"* — which said that a declared pack
is outside the record in both outcomes without saying what makes a directory a
pack. Everything after that paragraph in that draft still holds, and the
paragraph that this one contradicts is only its claim that a directory which
*reads* as a pack is one. The merger allocates the number; the entry is given
here in full so nothing of it lives only in a report (D226).

```markdown
## D-NEXT — 2026-10-30 — a pack directory is the three parts and nothing else,
and a directory that is more than a pack is refused whole

**`manifest.tsv`, optionally `pack.meta`, `objects/`, and no other name.** A
declared directory holding anything else is refused by name — before any object
is placed — and goes to `packs/refused/<job>-<attempt>/<declared path>` intact,
by the path D-NEXT (a pack Herd cannot read is set aside and kept) already
built.

**Why, and it is not tidiness.** A declared directory that ingests has
`leave_packs` `remove_dir_all` **all** of it, which is ORCH-153 — the pack is
gone from `{out}`, not excluded from it. So before this, a job that left
`{out}/packs/build.log` beside its pack had that log destroyed on the *success*
path, by the module that had just been taught to keep a pack on the failure
path. The objects survived under `objects/`; nothing else did.

**Refused, rather than removing only the three parts.** The fallback sounds
honest and invents a rule: it is Herd deciding, unasked, that a declared
directory is part pack and part output, and it weakens "gone from `{out}`, not
excluded from it" into "partly gone". Which of those bytes are the pack's is
the declaration's to say, and the declaration says the directory is a pack. One
rule, one outcome: the whole directory is kept, and the operator is told what
was found in it.

**It makes the module's own doc true.** `ingest` already claimed "anything else
is refused by name rather than guessed at" and nothing beneath it kept the
claim. That is the family this project has now found six times in one day — a
rule stated in prose and enforced nowhere — and this is the second time it was
found in this very file.

**The scope is the top level, deliberately.** What is inside the three parts is
their own business: a pack whose `objects/` holds a file no `kind f` row names
is still the pack its manifest says it is, and refusing on it would refuse real
packs whose tool left an object behind. The destruction path is narrowed, not
closed, and closing it wants the first consumer's own pack measured against a
draft refusal first.
```

## Resume point

Unchanged from the previous handoff: the plan's **S9** (the peer fetch client
and D10's three paths) and the fold-in that puts the pack payloads on
`ToHub`/`FromHub`. Nothing in this task changed this module's interface. The
fetch-chain agent's tree merges this branch with `pack-variants` and was
blocked on item 5 above, which is now fixed on this branch.

```sh
cd <scratchpad>/pack-store/wt   # branch pack-store
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-store
./scripts/bench-preflight.sh && nix develop --command ./scripts/test.sh
```

## Processes

No background process, no watcher, no `pkill`, no kill by name, and no PID of
any other agent or of the consumer's live hub (3006543) and runner (3021002)
touched. `./scripts/bench-preflight.sh` was run before every heavy phase and
cleared each time — 12385 MiB of slice room at load 4.08 for the first
compile, 16268 at 1.15, 15952 at 1.14, and 14798 MiB at load 1.69 before the
suite, all against an 8192 MiB floor and a ceiling of 9.00. One other heavy
agent was gating when this task started (the `pack-registry` suite, PID
3658308, not mine and not touched) and had finished before the gates here; at
no point were there two suites beside it. `/sys/fs/cgroup` was read by the
preflight only and nothing was written under it; no cache was dropped;
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set and no phase was run past a
refusal.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-store`, outside the worktree,
removed with `rm -rf /mnt/ssd_980/herd/build/pack-store` once the branch is
pushed (gotcha `build-weight-off-root`).
