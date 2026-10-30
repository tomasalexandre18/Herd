# pack-store-kept — handoff 2026-10-30

- **Item:** M11's pack half. One decision of `pack-store` reversed — a
  declared pack Herd cannot ingest is **kept**, not removed — and the first
  execution of everything that branch wrote.
  `docs/orchestration/history/2026-10-30-pack-store.md` is its author's
  handoff; it is not amended, and this file sits beside it.
- **Agent:** herd-step
- **Branch / commit:** `pack-store` / `57febf2` (code) plus this handoff, on
  top of `6a91155`, based on origin/main `1a8c1ae`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger` names
  the landing here, and fills the record of `templates/landing.md`

## Done

### 1. The reversal: a pack Herd cannot read is set aside and kept

`pack-store` decided that a declared pack it could not ingest was **removed**
from `{out}`, and flagged the choice as one it was not sure was its own. The
orchestrator reversed it: **Herd does not destroy an operator's bytes.** The
pack still leaves `{out}` before the record walk — that is ORCH-153 and it
stands, because one unreadable pack left in place is ~400 MiB of ordinary
output over the job's cap, so the whole collection is refused and
`results.json` does not come back either, one failure becoming two with the
second one silent — but it is now **renamed aside**, not deleted.

**Where:** `<data root>/packs/refused/<job>-<attempt>/<declared path>`.

- `refused` beside `objects/` and `manifests/`, because those two are the
  store proper — content a runner can hand to a job — and this is the third
  thing a declared pack can turn out to be: bytes Herd holds and cannot read.
  The word is the one this module already uses in every sentence that says so
  (`is refused by name`), so an operator who reads such a line and then looks
  at the disk finds the same word on it. `rejected`, `quarantine`, `bad` and
  `orphans` would each be a second vocabulary for one fact; `pending` and
  `retry` would promise something no code here does.
- `<job>-<attempt>` is **not a new name**: it is `outputs::spool_path`'s own
  file name, the one the sweep and the janitor already spell for one
  attempt's bytes, and `collect` takes it from `spool_at` rather than growing
  a job id and an attempt number of its own. One speller, not two.
- The declared path is kept **inside** it, so the kept tree reads like the
  `{out}` it came from, and two artefacts one job declared at two paths
  cannot land on each other. It is safe to join because it has already been
  through `herd_proto::check_relative` at that point in the loop: normal
  components only, no `..`, not absolute.
- Created **on demand**, never by `PackStore::open`, for the reason `packs/`
  itself is: an empty directory on every runner's disk is a feature
  announcing itself before it exists.

**The sentence**, on `JobOutputs::skipped` and on stderr, in the register of
`sweep::set_aside` rather than a new one — the fixture's own figures:

> this job declared that it leaves "pack" at "packs" and that is not a pack
> this runner can keep: `<why>`. Its 7777 bytes (7.59 KiB) are kept as
> `/tmp/…/packs/refused/j17-2/packs`: a declared artefact is never collected
> as the job's output, so it is out of the output directory — left where it
> is, one pack this runner cannot read would put the whole record over its
> output cap — and it is kept there for as long as it is on this disk.
> Nothing in this runner reads it, offers it or gives it to a job again,
> nothing here will ever remove it, and removing it is yours to do

The move is one `rename` inside the data root, so ~400 MiB costs a directory
entry and not a copy — the same reason the spool lives under the data root.
**No path through `set_aside` removes anything**, including the path where the
rename fails: a pack that cannot be moved aside stays exactly where the job
left it, and the sentence says that too, because the record below will then
carry the pack's own files and somebody will ask why.

Parked, as briefed, and not preconditions: the removal delay and the archive
path. "Kept indefinitely, the operator's to remove" is the whole of the
policy, which is the answer the hub's quarantine took.

### 2. Nothing removes it, and nothing picks it up again — three proofs

- `a_pack_that_cannot_be_read_is_kept_rather_than_removed` — the bytes are
  read back out of the kept tree and compared, the sentence is asserted to
  name the figures, the path and whose the removal is, and it is asserted
  **not** to say the bytes are gone.
- `a_start_neither_removes_a_set_aside_pack_nor_picks_it_up` — composed over
  the runner's two **real** startup entry points, `confine::Confined::prepare`
  (which reaps a past life) and `Session::pick_up_collections` (the sweep),
  the way `sweep`'s own ORCH-164 regression test is, because that is the
  defect shape: two halves each with a green test of its own and nothing that
  ran the startup sequence. The kept bytes survive both, neither report names
  the refused area, and `PackStore::open` on the same root finds no object —
  a refused pack's objects are not the store's objects.
- `no_source_in_this_runner_removes_a_set_aside_pack` — over the crate's
  source, because this is an assertion about code that does not exist yet. No
  line in any non-test source both removes something and names the refused
  area, and the area is named in exactly one module. **The detector is fired
  on a known-bad line first** (`fs::remove_dir_all(store.refused_dir())?;`),
  so its silence over the real source means something.

### 3. The branch's test target did not compile, and now does

`collect` grew a fifth parameter in `26c5cc0` and
`outputs::tests::a_job_with_no_output_directory_is_silent` still passed four,
so `cargo test -p herd-runner` could not build **at all** — every test on this
branch was unreachable, not merely unrun. `cargo check -p herd-runner --lib`,
the one command that branch ever ran, does not compile `#[cfg(test)]` code.
One line.

### 4. Both demonstrations the author named were executed, not reasoned about

**The `kind f` completeness filter is load-bearing.** `PackStore::missing`'s
`manifest.objects_needed()` was replaced by a map over `manifest.entries()`,
one line. Four packstore tests went red, and
`completeness_is_computed_over_kind_f_rows_only` named the mechanism exactly:

```
assertion `left == right` failed: every `kind f` object is there, so the pack is complete
  left: {"2222222222222222222222222222222222222222222222222222222222222222": 0,
         "3333333333333333333333333333333333333333333333333333333333333333": 0}
 right: {}
```

Those two are the fixture's `kind d` and `kind l` rows: a good pack read as
missing two objects that no `objects/` entry will ever answer to. Reverted;
`packstore.rs` back to sha256
`f46db2d33ad140a583fcf8fae9a0b03bb3eb1a3ac3495e6dc02aecf30f8d34d0`.

**The extraction's regression test goes red on removal.** `collect`'s one
`leave_packs` call was passed an empty map. Three tests red, carrying the four
failures the test was written for: a `packs/…` path on the record
(`left: ["packs/some/tree/file.txt", "results.json"]`), the collection's
weight carrying the pack (`394514 bytes (385.27 KiB) in 5 file(s)` where the
record's own file is 11 bytes), the whole collection refused over the cap so
that `results.json` is lost too (`files: []`, `refused: Some("…no file came
back at all…")`), and `{out}/packs` still on the disk at walk time — which is
the same fact as the first, since the walk could only report those paths by
reading them. Reverted; `outputs.rs` back to sha256
`fd65eae5246bf185efa55605819f353b5fc9e6c1353ba1946c629641720f83e5`.

## Not done, and why

1. **`herd-proto`'s digest-column validation is left where it is.** Finding 1
   of `2026-10-30-pack-store.md` is right and is already assigned to whoever
   next holds `pack.rs`; `herd-proto` is on this brief's do-not-touch list and
   another agent is folding frame variants there now.
   `packstore::object_name` keeps the runner's own check meanwhile.
2. **`holds_object` still refuses a size disagreement rather than repairing
   it.** Deliberate, as its author said: overwriting would let a bad manifest
   destroy a good object.
3. **The removal delay and the archive path** for a kept pack: parked for
   Alexa, and not preconditions for this branch.
4. **No frame, no socket, no fetch, no registration**, and `declared_leaves`
   still returns an empty map: as briefed, unchanged.
5. **`docs/plan.md`'s stale sentence** about the pack path is still stale;
   `plan.md` is on the do-not-touch list.
6. **Nothing was changed about the success path's `remove_dir_all`** — see
   finding 1 below, which is a second decision and not mine to take.

## Gates

All four Rust gates were run on the committed code, **before** this handoff
was written, with `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-store` and
`./scripts/bench-preflight.sh` clear before each heavy phase.

```
$ nix develop --command cargo fmt --all --check
(no output)   exit 0

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.39s
exit 0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.43s
exit 0

$ nix develop --command ./scripts/test.sh
==============================================================
 14 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
exit 0
```

The first clippy run of this branch was **red**, and that is the finding of
item 3 above: `error[E0061]` on `outputs.rs:825`, `collect` called with four
arguments where it takes five. Fixed in `57febf2`, then green.

**Skips: 14, every one a host-shape skip, none of them new and none of them
masking this change.** Twelve are cgroup v2 delegation (`this runner cannot
create a cgroup under /sys/fs/cgroup/chatons.slice/…`, Permission denied):
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
`the_runner_says_which_memory_ceiling_it_enforces`. Two are the flood benches
refusing a shared host's load: `flood-fifty` and `flood-two-fifty`, at a
one-minute load of 8.03 against their own ceiling of 8.

**The last run, not quoted here** because it is made on the tree this file is
committed into: `./scripts/check-docs.sh` and then bare `./scripts/gate.sh`
(never inside `nix develop` — it nests `TMPDIR` and panics `confinement.rs`
on the 108-byte `SUN_LEN`). Its output goes to the agent's report and to
`<scratchpad>/pack-store/gate.log`. The branch is pushed only on a green
verdict; a red one is reported red and nothing is pushed.

## Risks and findings

### 1. The **success** path still removes whatever else was in the pack
directory, and that is the same decision over again

`ingest` does not refuse a pack directory that holds more than
`manifest.tsv`, `pack.meta` and `objects/` — it reads the manifest, places
the `kind f` objects, and ignores everything else — and then `leave_packs`
calls `fs::remove_dir_all` on the whole declared path. So a job that leaves
`{out}/packs/build.log` beside its pack has that file **destroyed**, by the
path that was just taught not to destroy anything. The hard-linked objects
survive under `objects/`; nothing else does.

Not fixed here, deliberately: the brief reversed exactly one decision, and
the repair is a second one with two defensible answers — refuse any declared
directory that is not exactly the three parts (which is what `ingest`'s own
doc already claims, so the doc is currently wrong), or remove only the three
parts and let the remainder stay as ordinary output. **Recommendation:** the
first, in `ingest`, because it makes the module's stated rule true and keeps
"the pack directory *is* the pack" (that branch's decision 2) honest.

### 2. `lay_out` can panic on its own refusal path

`packstore.rs:433`: `needed.len() - missing.len()`, two lengths computed by
two calls. They agree in the shipped code — both go through
`objects_needed()` — but during demonstration one they did not, and the
result was not a refusal but
`panic_const_sub_overflow` in `<PackStore>::lay_out`, with two tests dying on
a subtract instead of reading the sentence. A `saturating_sub` costs nothing
and a refusal path is the last place that should panic. One line, not taken
here because the file was reverted to its gated state.

### 3. `place_object`'s "nothing else knows that name" is not quite true

The doc says the staged file is hashed under a name nothing else knows, so
nothing can change it between the hash and the rename. On the hard-link path
the staged file is the **same inode** as the job's own file under `{out}`, so
the job's name for it is a second name. Harmless today — the job's processes
are gone before `collect` runs — but the claim is stronger than the code, and
a later caller that ingests while something can still write would inherit a
sentence saying it was safe.

### 4. `apparent_bytes` walks the pack on the success path too

It is measured before `ingest` so the failure sentence can carry a figure,
which means a 557-file pack is walked once for a figure that is then unused
whenever the ingest succeeds. Sub-millisecond, named only because S12's
measurement should not be surprised by it.

### 5. A kept pack whose modes the job shut stays shut

The record walk takes the runner's access back on directories a job chmodded
(`a_directory_the_job_closed_is_still_collected`), and the reaper restores
modes under `jobs/`. Nothing restores modes under `packs/refused/`, so a
pack set aside with its directories at `0000` is kept exactly as it was —
which is the promise, but its owner may need `sudo` to look at their own
bytes. The rename itself is unaffected: it needs write access on `{out}`,
not on the directory being moved.

### 6. The author's four findings, re-read

Findings 1 (the digest column belongs in `herd-proto`), 2 (hashing on ingest
costs one read), 4 (a size disagreement is refused, not repaired) and 5 (the
staging name's two properties) all read correctly against the code, and 1 and
4 are explicitly left alone as briefed. Finding 3 is the decision this branch
reverses and is now settled the other way.

## Decisions needed

1. **Finding 1: what `ingest` does with a declared directory that holds more
   than a pack.** *Recommendation:* refuse it by name, which makes the
   module's own doc true and sends the whole directory to `refused/` intact.
   *Fallback:* remove only the three parts and let the remainder be collected
   as ordinary output — honest, but it is a rule Herd would be inventing and
   it weakens "gone, not excluded" to "partly gone".
2. **Findings 2, 3, 4, 5:** each is one line or one sentence, none is urgent,
   and all four are in files S9 will open next. *Recommendation:* fold them
   into S9's brief rather than spending a gate on them now.

### Drafted decision entry

**This supersedes the last paragraph of the drafted entry in
`2026-10-30-pack-store.md`** — the one beginning "A declared path leaves
`{out}` whether or not it could be stored" — which recorded the decision this
branch reverses. Everything above that paragraph in the author's draft still
holds. The merger allocates the number; only the final paragraph differs, and
it is given here in full so it is not lost in a report.

```markdown
## D-NEXT — 2026-10-30 — a pack Herd cannot read is set aside and kept, never
removed

**A declared pack leaves `{out}` whether or not it could be stored, and in
neither case is it destroyed.** Why it leaves is the record: a pack Herd could
not ingest, left where the job put it, is ~400 MiB of ordinary output over the
job's output cap, so the whole collection is refused and `results.json` does
not come back either — one failure becoming two, with the second one silent. A
declared pack is outside the record *by declaration*, so it is outside it in
both outcomes.

**Where it goes is `<data root>/packs/refused/<job>-<attempt>/<declared
path>`, and nothing in Herd ever removes it** — not a timer, not the sweep,
not the reaper, not a start. This is not a new shape: the runner's sweep
renames a spool no hub will claim to `<job>-<attempt>.unclaimed` and keeps it,
the hub quarantines a record it cannot read byte for byte, and D221 made
removal depend on the hub's own sentence rather than on a silence. A fourth
path that silently deleted hundreds of megabytes would be the only place in
Herd that does. The reason and the figures are named on the record's `skipped`
list and on the machine's stderr, and the sentence says in words — the sweep's
own words — that removing it is the operator's and that nothing here will.

**Terminal, like a set-aside spool.** Bytes under `refused/` are never
ingested, never laid out, never offered and never looked at again, so the
state does not flip at every boot. Asserted three ways: on the kept bytes, over
the runner's two real startup entry points (the reaper and the sweep, composed
as ORCH-164's regression is), and over the crate's source with the detector
fired on a known-bad line first.

**How long it is kept is not a question this has an answer to: indefinitely.**
The removal delay and the archive path are Alexa's and are parked; "kept, the
operator's to remove" is the whole of the policy, and it is the same answer the
quarantine took.
```

## Resume point

The plan's **S9** (the peer fetch client and D10's three paths) and the
fold-in step that puts the pack payloads on `ToHub`/`FromHub`. S9 reads
`Collected.packs` for the registration frame and calls `PackStore::lay_out`
for `{in}/<key>`; the fold-in is what makes `declared_leaves` return
something. Neither needs a change to this module's interface. Findings 1–5
above are all in files S9 opens, and finding 1 wants a decision before it is
touched.

```sh
cd <scratchpad>/pack-store/wt   # branch pack-store
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-store
./scripts/bench-preflight.sh && nix develop --command ./scripts/test.sh
```

## Processes

One background watcher, started and stopped by this agent: a read-only
`bench-preflight.sh` until-loop polling every twenty seconds
(`<scratchpad>/pack-store/wait-for-room.sh`), plus one `Monitor` doing the
same that was stopped immediately so the host was polled once and not twice.
Both ended before the gate. No `pkill`, no kill by name, no PID of any other
agent touched. `/sys/fs/cgroup` was read (`chatons.slice/memory.stat`,
`memory.high`, `memory.current`) and nothing was written under it; no cache
was dropped; `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set, never
exported, and no phase was run past a refusal.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-store`, outside the worktree,
removed with `rm -rf /mnt/ssd_980/herd/build/pack-store` once the branch is
pushed (gotcha `build-weight-off-root`).
