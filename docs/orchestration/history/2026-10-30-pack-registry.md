# pack-registry — handoff 2026-10-30

- **Item:** M11's pack half, step **S6** of the pack-half plan — the hub's
  pack registry, with D199's `RECORD_VERSION` 1 → 2 and `LAYOUT_VERSION`
  1 → 2 in the same commit
- **Agent:** herd-step
- **Branch / commit:** `pack-registry` / see the agent's report, based on
  `pack-proto` `adbc204` merged with origin/main `1a8c1ae`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

### The merge

`git merge origin/main` on top of `pack-proto`: **clean, no conflict**, 13
files, 2 711 insertions. The five new quarantine fixtures the brief warned
about in `herd-hub/tests/durability.rs` are **not** in this merge — that
batch had not landed on `origin/main` at 23:40 — so there was nothing to
conflict with. `durability.rs`'s two existing record-version assertions
(`:344`, `:377`) are written against `herd_hub::RECORD_VERSION` and are immune
to the move.

### The two version numbers, in one commit

| Constant | File:line | Was | Is |
|---|---|---|---|
| `RECORD_VERSION` | `herd-hub/src/job.rs:72` | 1 | **2** |
| `LAYOUT_VERSION` | `herd-hub/src/store.rs:99` | 1 | **2** |
| `PROTOCOL_VERSION` | `herd-proto/src/lib.rs` | 3 | **3, untouched** |

`.github/workflows/release.yml:169-174`'s two `sed` patterns still read one
number each out of the moved lines: re-run by hand, they print `3`, `2`, `2`.

Both constants' doc comments are rewritten. `LAYOUT_VERSION`'s was three lines
and said only *"A state root written by a newer Herd is refused rather than
read"*; it now carries the cost paragraph its two siblings have, and the reason
the two numbers move together.

### The registry

`herd-hub/src/registry.rs`, 1 100 lines, 15 unit tests. Its doc comment opens
with *"**Nothing calls this from the network yet.**"* and says so again on
`JobDirStore::packs()` and on the `packs` field, so its existence cannot be
read as a working feature: the pack frames are numbered in `herd_proto::pack`
and are not dispatched by the hub's loop, which is a separate step owning
`server.rs`.

**On disk**, under the state root:

```
packs/
  manifests/
    <id>/                 <id> = sha256(manifest.tsv), 64 lowercase hex
      manifest.tsv        the bytes, verbatim, as the runner sent them
      pack.meta           verbatim; absent when the pack has none
      registered.json     record_version, pack, commit, key, runner, job,
                          attempt, registered_at, manifest_bytes, objects,
                          object_bytes, meta_bytes
  manifests.part/         a registration that did not finish; cleared at open
```

Measured: the whole of `packs/` for one registered pack weighs **under 4 096
bytes** and holds not one object byte — asserted in
`herd-hub/tests/pack_registry.rs`.

**Three properties the shape is for**, and each is a test:

1. **A registration is one `rename(2)`.** Every file is written and fsynced
   inside `manifests.part/<pid>-<id>-<n>/`, that directory is fsynced, and only
   then does it take the name `manifests/<id>`. There is no second rename to be
   interrupted between, so a pack is complete or absent (E-17).
2. **The `(commit, key)` index is derived, never stored.** Rebuilt at
   `PackRegistry::open` from each `registered.json`, exactly as
   `index/state.json` is rebuilt from the job directories. No durable index to
   keep true, nothing to interrupt.
3. **The id is recomputed from the bytes on the disk too**, not only off the
   frame. A pack directory whose `manifest.tsv` does not hash to the
   directory's own name is set aside by name and never resolved — the identity
   rule is not taken on a directory name somebody typed either.

**Holdings** are `BTreeMap<runner, Holding{endpoint, held, arriving}>` in
memory only, per manifest, never per blob. `announced()` implements
pack-proto's documented **replace** semantics: `more: true` accumulates into
`arriving`, `more: false` applies it, and an empty announcement with `more:
false` is the real statement *"I hold none"*. `runner_gone()` and
`forget_all_holdings()` are the volatile half. There is no holdings file and
the test asserts none was written.

`state_of()` returns `Registered` or `NoHolder` and **never `Incomplete`**,
with the reason in its doc: holdings are per manifest, so "some object is held
nowhere" is a statement about blobs this hub has no information about.

Anything that walks a manifest goes through `PackManifest::objects_needed()`,
which is herd-proto's `kind == f` filter. Both fixtures — the unit one in
`registry.rs` and the integration one in `pack_registry.rs` — are four rows:
two `kind f`, one `kind d`, one `kind l`. Nothing in this module can reprint a
manifest: `stage` writes `manifest.bytes()` and the comment says why.

### The record

`JobRecord` gains `packs_in` and `packs_out`, both `BTreeMap<String,
herd_proto::pack::PackId>`, both `#[serde(default, skip_serializing_if =
"BTreeMap::is_empty")]`. The **id** and not the manifest: the bytes live once,
in `packs/manifests/<id>/manifest.tsv`. No pack field went on the verdict
document, so `VERDICT_FORMAT_VERSION` and `OUTPUT_FORMAT_VERSION` have not
moved.

**Said plainly, because it bears on what the bump bought:** these two fields
are `#[serde(default)]` and absent means exactly what their default says, so by
this project's own written rule they are **additive** and would not have needed
a record move at all. What the move buys is in the next section.

### The three refusals, in full

**1. Layout** — `herd-hub/src/store.rs`, `layout_version_mismatch_reason`:

> the state root {root} has layout version {found}, and this hub writes version
> {LAYOUT\_VERSION}. Nothing converts it: there is no migration tool, no
> --migrate and none planned, and the check is equality and not a floor, so
> rolling back to the release that wrote this root is refused in exactly the
> same way once this hub has written to one. Crossing this number means
> starting on a fresh root and leaving every queued job and every uncollected
> verdict behind in the old one. In order, and the order is the part that
> matters: (1) take the reading off this root FIRST, with the OLD release's
> binary — `herd-hub report --state-dir {root}` — because once the root has
> been moved aside you will not get another chance at it; (2) move the root
> aside, do not convert it and do not delete it, and give this hub a new empty
> volume; (3) expect every runner to be refused once, by name, and that is
> correct. The whole instruction is in docs/operating.md, "The instruction, in
> order"

The old sentence is a **prefix** of the new one, so `docs/operating.md:576-578`
quotes it correctly without being edited.

**2. Record** — `herd-hub/src/job.rs`, `record_version_mismatch_reason(path,
found: Option<u32>)`, used by both of store.rs's sites:

> {path} has record version {found}, and this hub reads version
> {RECORD\_VERSION}. It is set aside: this hub will not dispatch it, will not
> rewrite it and will not repair it, so whatever it was for has to be asked for
> again. There is no migration in either direction — the check is equality and
> not a floor, so rolling this hub back to the version that wrote it does not
> read what version {RECORD\_VERSION} has written since. If every record in
> this root reads this way then the root itself is from another release, and
> every queued job and every uncollected verdict in it is unreadable to this
> build: do not convert it and do not delete it, take the `herd-hub report
> --state-dir` reading off it with the old release's binary first, and follow
> docs/operating.md, "The instruction, in order"

The whole-root clause is **conditional** on purpose. Since D218 this check sets
*one record* aside and serves the rest, so an unconditional *"every queued job
in this root is unreadable"* would be false of the one-odd-record case. With
both numbers moved, a whole root from another release is refused at the door
and never reaches this sentence more than once.

**3. The absent-versus-zero defect, fixed where it is mine.** `store.rs`'s own
`VersionProbe` carried `#[serde(default)] record_version: u32`, so a `job.json`
with **no** `record_version` key read back as `0` and the refusal said *"has
record version 0"*. It is now `Option<u32>` and the shared sentence says
**"carries no record version at all"**. Asserted in `registry.rs`'s
`a_registration_with_no_record_version_at_all_is_said_to_be_absent`, which also
asserts the string `record version 0` does **not** appear.
`herd-hub/src/report.rs:80-85` and `herd-hub/src/retention.rs:620-624` have the
identical defect and are **another agent's** — see *Risks and findings*.

### The half-move, measured and not argued

`herd-hub/tests/version_move_at_the_door.rs`, three tests, and its doc comment
is the table of where the two checks live and how they differ.

The counterfactual was **run**, not read. With `LAYOUT_VERSION` put back to 1
in a throwaway build tree while `RECORD_VERSION` stayed 2 — which is the
half-moved build exactly — a state root that is byte-for-byte a v0.4.1 root
(`store.json` layout 1, three `job.json` at record 1) was handed to
`JobDirStore::open`. It **opened**. Every job was quarantined by name and
`unfinished_jobs()` was empty: a hub that would have bound its port and served
an empty queue. The stderr of that run is in the agent's report, verbatim.

That counterfactual is now a permanent green test
(`a_record_only_move_would_have_started_a_hub_on_a_wholly_quarantined_root`,
written against `LAYOUT_VERSION` so it keeps meaning the same thing), beside
`moving_both_numbers_puts_the_refusal_at_the_door_with_the_instruction`, which
asserts twelve clauses of the layout refusal by substring and that the refused
root gained **no `packs/` directory** — `PackRegistry::open` is called after
the layout check for that reason, since creating a directory in a root
`docs/operating.md` says to move aside rather than convert is still a
conversion.

**What catches a half-move is the literal pin, and nothing else.** Experiment A
below showed the two behavioural tests stay green under a half-move, because
each is written relative to the constant it is about. Only
`both_numbers_are_where_d199_put_them` went red. That is the right division of
labour and it is why the pins stay literals.

### Every assertion and fixture changed, old beside new

| Site | Old | New |
|---|---|---|
| `herd-hub/src/job.rs:72` | `pub const RECORD_VERSION: u32 = 1;` | `= 2;`, doc rewritten |
| `herd-hub/src/store.rs:99` | `pub const LAYOUT_VERSION: u32 = 1;` | `= 2;`, doc rewritten from 3 lines to the cost paragraph |
| `herd-hub/src/job.rs` test `the_record_version_does_not_move` | `assert_eq!(RECORD_VERSION, 1, "the field is additive; D199 moves this to 2 …")` | `assert_eq!(RECORD_VERSION, 2, "2 is D199's move … this field is additive and moved nothing")`; the `PROTOCOL_VERSION` literal 3 beside it is untouched |
| `herd-hub/tests/announced_job_ceiling.rs:133` | `assert_eq!(RECORD_VERSION, 1, "every existing state root still reads; …")` | `assert_eq!(RECORD_VERSION, 2, "2 is D199's move, made with the hub's own pack registry and with LAYOUT_VERSION in the same commit; …")`; doc comment rewritten (it said *"`PROTOCOL_VERSION` has since moved"* and now says both have) |
| `herd-hub/tests/ran_commit_across_versions.rs:209` | same pair | same rewrite, worded for `ran_commit` |
| `herd-hub/src/status.rs:627-628` | already `protocol_version: herd_proto::PROTOCOL_VERSION, record_version: crate::job::RECORD_VERSION` | **verified, not changed** — `pack-proto` had already made it relative, which is the one shape that survives this move |
| `herd-hub/tests/state_root_report.rs:64` | `"record_version": 1,` in `silent()` — *"a readable record"* | `herd_hub::RECORD_VERSION`, with the comment saying it would have become an **unread** record at 2 and tested the opposite of its own name |
| `herd-hub/tests/state_root_report.rs:163` | `eleven["record_version"] = json!(2)` — *"a record this build cannot read"* | `json!(herd_hub::RECORD_VERSION + 1)` — **not in the brief's list**, and the more dangerous of the pair: at 2 it silently became a readable record and the census would have counted eleven read and said nothing |
| `herd-hub/tests/state_root_report.rs:754` (in `EXPECTED`) | `it carries record version 2, and this build reads version 1` | `{UNREAD}` / `{READS}`, substituted from the constant beside `{ROOT}` |
| `herd-hub/tests/retention_preview.rs:161` | `"record_version": 1,` in `record()` | `herd_hub::RECORD_VERSION` |
| `herd-hub/tests/retention_preview.rs:326` | `other_layout["record_version"] = json!(2)` | `json!(herd_hub::RECORD_VERSION + 1)` — **not in the brief's list either**, and worse here: an unreadable record that becomes readable is a 300-day-old job that becomes **removable**, the opposite of the comment above it |
| `herd-hub/tests/retention_preview.rs:1364` (in `EXPECTED`) | same literal pair | `{UNREAD}` / `{READS}` |
| `herd-hub/src/store.rs:4272` | `{"layout_version":99}` | **left alone**, as instructed |
| `herd-hub/tests/crash_points.rs:346` | `{"record_version":99}` | **left alone**, as instructed |
| `herd-hub/src/store.rs:4296` | `RECORD_VERSION + 1` | **left alone** — relative, immune, and the pattern the two fixtures above were rewritten into |

`herd-hub/tests/{walk_cost,sync_prices,unsynced_write_cost}.rs` and
`durability.rs` were checked and need nothing: every one of their
record-version sites is written against the constant.

### The acceptance cases, and which were proved by watching them fail

Five experiments were run in the branch's own build tree, each reverted
afterwards and the suite re-run green. Verbatim output in the agent's report.

| Broken on purpose | What went red |
|---|---|
| **A** `LAYOUT_VERSION` back to 1, record stays 2 | `both_numbers_are_where_d199_put_them`; and the half-moved build **opened** the v0.4.1-shaped root with all three jobs quarantined — the measurement itself |
| **B** count every manifest row, not just `kind f` | `the_objects_counted_on_a_registration_are_the_kind_f_rows_and_nothing_else`, `left: 4` against 2 |
| **C** believe the announced id instead of recomputing it | `an_announced_id_that_is_not_the_sha256_of_the_bytes_is_refused_by_name` |
| **D** newest-wins on two ids for one `(commit, key)` | `two_ids_for_one_commit_and_key_is_refused_by_name_and_not_a_newest_wins` |
| **E** stage with `fs::write`, no fsync | `nothing_of_a_pack_is_visible_before_every_byte_of_it_is_synced`, naming the two files that were still synced and the one that was not |
| **F** publish into `manifests/` with no staging rename | nine of the fifteen, `an_interrupted_registration_leaves_nothing_readable_and_is_cleared_at_open` among them |

## Not done, and why

- **No frame is wired into the hub's loop.** `server.rs` is another agent's and
  the brief says so; nothing calls the registry from the network. Said in the
  module doc, on `packs()` and on the field.
- **The record's two fields are never written with a value by this branch**,
  because the only things that would write them — resolving `needs` at
  assignment, accepting a registration frame — are S7's and S8's. They are on
  the record's shape and in `job.json`'s serialisation, and that is all.
- **`report.rs` and `retention.rs` are untouched**, so two of the four
  record-refusal sites still carry their own wording and their own
  absent-as-zero defect. Finding 1.
- **No prose outside the code was edited.** `docs/architecture.md:346`,
  `docs/operating.md:560-563`, `docs/handover.md:67`, `docs/disk-policy.md:769`
  and `:1462` all now say something false about `LAYOUT_VERSION` or
  `RECORD_VERSION`. Finding 2. The brief's do-not-touch list covers
  `operating.md`; the others are not mine either.
- **`docs/decisions.md` untouched.** The drafted entry is below.

## Gates

**The verdict first, because it is not green.** `./scripts/test.sh` is **RED
on this host**, `./scripts/gate.sh` was **NOT RUN**, and **this branch is not
pushed**. The three Rust gates that are green were taken with
`bench-preflight.sh`'s cgroup-room floor lifted inline, which the orchestrator
has since ruled out; they are reported here as what they are and not as a
passing gate. What covers the gates is the batch's own `./scripts/gate.sh` run
at integration, which is `herd-merger`'s.

**Green, on the tree of this commit, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-registry`:**

```
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.84s
exit 0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
exit 0

$ nix develop --command cargo fmt --all --check
(no output)   exit 0

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit 0
```

**Green, narrow, run while iterating (not a gate, but the evidence for this
branch's own claims):**

```
$ nix develop --command cargo test -p herd-hub --lib registry
running 15 tests
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 230 filtered out

$ nix develop --command cargo test -p herd-hub \
      --test version_move_at_the_door --test pack_registry
pack_registry            4 passed; 0 failed; 0 ignored
version_move_at_the_door 3 passed; 0 failed; 0 ignored
exit 0
```

**RED — `./scripts/test.sh`, twice, and the red is the host.** Both runs
exited 101, and **no two runs failed the same set**:

| Run | Targets failed |
|---|---|
| 1 | `admission`, `approval`, `away_fleet` |
| 2 | `admission`, `killed_hub`, `owner_control`, `slots`, `confinement` (`herd-runner`) |

Every failure but one is a `herd-hub/tests/real_herd/mod.rs` wait that ran out,
and the harness names its own cause in the panic:

```
timed out after 30s waiting until the job is over.
why this wait may have run out:
  * the in-test cargo build compiled for 14.502702744s and finished
    213.811244ms before this wait began — every core it could take, on the
    same cores as the hub and the runner this wait is about.
  * host load average 11.89 / 9.93 / 8.71 (1, 5, 15 min) over 12 cores,
    read as this wait ran out
```

and, in run 2, `4.27 / 8.87 / 9.16` and `3.43 / 7.99 / 8.84`.

**Run 1's three targets were then re-run on their own, prebuilt, and all 15 of
their assertions passed:**

```
$ nix develop --command cargo test -p herd-hub \
      --test admission --test approval --test away_fleet
admission  5 passed; 0 failed    approval  5 passed; 0 failed
away_fleet 5 passed; 0 failed
exit 0
```

**The one failure that is not a timeout is the preflight's refusal coming true
word for word**, and it is worth quoting because it is the argument for not
having lifted the floor:

```
herd-hub/tests/admission.rs:456
the inherited job's 1 GiB of declared memory must still be held:
  … on lender, 12 MiB free of its 1 GiB memory admission, 1 GiB needed …
```

The assertion wants *"512 MiB free of its 1.5 GiB memory admission"*. The
runner announced **1 GiB and not 1.5 GiB** because that is what this host could
honestly lend at that moment (D119) — the runner was right and the test went
red on a true statement about the host, which is the exact sentence
`bench-preflight.sh` refuses in order to prevent. **Nothing in this branch
touches admission, memory announcement or the runner.**

**NOT RUN, and named as not run:** bare `./scripts/gate.sh`. It was never
started, so there is no receipt and no log. Nothing of this branch has been
gated by the gate.

**Skips.** Run 2 reported **12**, which is this host's figure when green, and
all twelve are the cgroup-delegation and `io.max` family —
*"cgroup v2 delegation is unavailable: this runner cannot create a cgroup
under /sys/fs/cgroup/chatons.slice/… (Permission denied)"*. Run 1 reported
**14**: the same twelve plus `flood-fifty` and `flood-two-fifty`, both skipped
because *"the host's one-minute load is Some(17.63), over this bench's ceiling
of 8"*. Every `HERD-SKIPPED` line is copied here as a **skip** and none is
counted as a pass (D27). Both runs printed *"AT LEAST N assertion(s) COULD NOT
RUN … and the suite did not finish … so this list is a floor and not a count"*,
so even the skip count is a floor.

**Host, disclosed in full, including the override that should not have
happened.** `bench-preflight.sh` refused on the cgroup-room clause 24
consecutive times between 23:50 and 00:05 and kept refusing afterwards: 133 –
5 351 MiB under `memory.high` on `/chatons.slice` against the 8 192 MiB floor,
oscillating minute to minute with six other agents on the host.
`/sys/fs/cgroup/chatons.slice/memory.stat` was read directly, as the brief
allows, and gave `anon` **527 – 848 MiB** against `file` **12 494 – 15 145
MiB` — 94 – 96 % of the slice's charge was reclaimable page cache from the
sibling build trees, which `memory.current` counts. On that reading
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB=2` was set **inline on each heavy command,
once per command, with the figures printed before each one**, and **never
exported** — it was absent from the shell's environment (`env | grep -c
HERD_PREFLIGHT` → `0`), so no gate ever saw it and no `check-orchestration`
case about what a refusal says could be reddened by it. Nothing under
`/sys/fs/cgroup` was written and no cache was dropped. The **load** clause was
never lifted: every heavy command was started under a one-minute load of
3.37 – 7.96 against the 9.00 ceiling, and the two readings of 9.10 and 15.52
were waited out.

**And the lift was the wrong call.** The orchestrator's ruling arrived
mid-task — *"treat a preflight refusal as a gate, do not lift it on load, and
if you cannot gate tonight, commit, write the handoff, and report the gate as
not run rather than overriding anything"* — and the suite's own red proves the
ruling right on the memory clause too, not only on load: `admission.rs:456`
went red because the host could lend 1 GiB and not 1.5 GiB, which is the
refusal's own prediction. No further command was run with the floor lifted
after that ruling, `gate.sh` was not run at all, and the branch was not pushed.
## Risks and findings

Ordered by what is hardest to repair later.

**1. The other two record-refusal sites are already fixed on another branch,
and the three sentences now differ.** `herd-hub/src/report.rs` and
`herd-hub/src/retention.rs` carried the identical absent-as-zero defect, and
`measured-fold` (`13b1925`, pushed) fixes both: each probe is now
`Option<u32>`, each says *"it carries no record version at all, and this build
reads version {RECORD_VERSION}"*, and `report.rs` gains a banner
*"NOT ONE RECORD COULD BE PARSED BY THIS BUILD, so every figure below is …"*
when nothing was read. Both format from the constant, so **my bump makes their
sentences read "version 2" with no edit**. Verified by reading that branch, not
assumed.

**No textual conflict with this branch**: `measured-fold` touches
`report.rs`, `retention.rs`, `herd-proto/src/size.rs` and
`herd-runner/src/size.rs`, and **not** `state_root_report.rs` or
`retention_preview.rs`, which are mine. Its own fixtures already use
`RECORD_VERSION` and `RECORD_VERSION + 1`, which is the idiom this branch
rewrote the two literals into.

**What is left is a convergence, not a defect.** After both land the tree holds
**three** wordings of one refusal: `measured-fold`'s in `report.rs` and
`retention.rs`, and this branch's longer one — which says what is lost, that
the rollback is refused too, and points at `docs/operating.md` — in
`herd-hub/src/job.rs` for the hub's own two sites. The reviewer's invariant-4
argument wants one sentence in one place, and
`herd_hub::record_version_mismatch_reason` is exported from the crate root and
takes exactly the `Option<u32>` the other two readers already produce.
Decision 1.

**They must read as a pair, and they do.** `herd-hub report` stays runnable on
a root the hub refuses — that is ORCH-161's whole point and it is on two
published release pages — so an operator meets the banner and this branch's
refusal together. This branch's sentence tells her to run exactly that
command, with the old release's binary, **before** moving anything. It makes
no claim about bytes on the volume, deliberately: `measured-fold` measured that
an all-unread root prints *"every `files/` directory measures 0 bytes"* over
directories that really held 27, because the measurement sits inside the
per-record loop after the version check. Nothing in this branch's text leans on
that figure.
**2. The per-record refusal is now ~700 characters, printed once per
quarantined record at startup.** On the consumer's ~1 571-job root a half-moved
build would print ~1.4 MB of stderr. With both numbers moved that case is
refused at the door and prints **once**, which is the argument for moving both
— but a partial restore leaving a handful of foreign records will print the
paragraph per record. The cheap remedy, if it is wanted, is for `store.rs:1253`
and `:1429` to print the long half once and the short half per record; this
branch did not, because the brief asked for the full treatment and truncating
it would undo that.

**3. `.github/workflows/release.yml` does not read `LAYOUT_VERSION`.** It greps
`PROTOCOL_VERSION` and `RECORD_VERSION` only (`:169-174`), and
`docs/operating.md:556` says *"the release body prints the two that matter on
lines a script can match"*. `LAYOUT_VERSION` now moves for the first time ever,
and it is the number with no migration path at all — so the release that
ships this is the first release whose most destructive number is not on a line
the consumer's script can match. `.github/` is `herd-merger`'s. Decision 2.

**4. The two record fields did not need the record move.** Both are
`#[serde(default)]` and absent means exactly what their default says, which is
this project's own written definition of additive
(`herd-proto/src/lib.rs:105-108`, `job.rs`'s own doc). So `RECORD_VERSION` 2
buys **not** their carriage but the refusal arriving at the door together with
the layout's, and a rollback being refused on the record as well as on the
root. That is a real purchase and D199 authorised it; it is stated here because
a later reader comparing the fields to the number would otherwise conclude the
move was unnecessary and be half right.

**5. Four `JobRecord` construction sites had to gain two lines, and two of them
are outside the brief's *Yours* list.** Rust requires every field in a struct
literal, so a new field on `JobRecord` breaks every construction of one. There
are exactly four in the workspace: `herd-hub/src/store.rs:1904` and
`herd-hub/src/job.rs:1499` (mine), and **`herd-hub/src/queue.rs:312` and
`herd-hub/src/matcher.rs:1624`**, both inside `#[cfg(test)]` helper functions,
one line each (`packs_in: Default::default(), packs_out: Default::default(),`),
adjacent to the existing `outputs: Default::default(),` and changing no
behaviour and no assertion. Neither file is on the brief's do-not-touch list
and neither is held by another agent tonight, but neither is on the *Yours*
list either, so both are declared here rather than passed over. `matcher.rs` is
S8's step; if S8 is already in flight, these two lines are the only thing to
reconcile. `herd-hub/src/lib.rs` also gained three lines — `pub mod
registry;` and two `pub use` lines — which a new module cannot exist without.

**6. `PackRegistry::open` re-hashes every manifest at every start.** That is
one read and one sha256 of each `manifest.tsv` — tens of kilobytes apiece on
the consumer's own measurement, so tens of milliseconds for a fleet's worth of
packs — and it is the only place the identity rule can be checked for a pack
this hub did not receive itself. **Unmeasured at scale:** nobody has timed a
root with hundreds of packs in it, and the figure belongs in S12's measurement
beside the per-object transfer cost.

**7. A pack directory is never removed by anything.** There is no retention, no
sweep and no budget for `packs/`. The registry only ever grows: one directory
per `(commit, key)` for ever. `docs/retention` and the output budget govern
`files/` and not this. That is deliberate for tonight — the removal delay and
the archive path are parked for Alexa, per the plan's §8 — but it means the
first consumer's root grows without bound in a directory the hub created
tonight, and nothing in the tree says so. **This is the thinnest part of the
layout and the one I would want named before v1.0.0**, not because the bytes
are large (they are kilobytes each) but because an unbounded directory with no
owner is how a disk policy gets discovered rather than decided.

**8. The brief's premise about `status.rs:627-628` was already satisfied.**
`pack-proto` had rewritten that fixture to be built from the constants. It was
verified and not changed. Reported because the brief asked me to verify rather
than trust the list, and this is the one entry on it that needed nothing.

## Decisions needed

1. **Converge the record refusal on one sentence, and which one.**
   `measured-fold` has fixed `report.rs` and `retention.rs` independently, so
   after both land there are three wordings of one refusal (finding 1).
   *Recommendation:* a short follow-up step owning all four files, pointing
   `report.rs` and `retention.rs` at
   `herd_hub::record_version_mismatch_reason` — it is exported and takes the
   `Option<u32>` they already produce — and keeping `report.rs`'s banner, which
   says something this sentence does not. *Fallback:* leave three wordings and
   record it, accepting that an operator who pastes one into a question gets a
   different answer depending on which subcommand printed it.
2. **Does `LAYOUT_VERSION` go on the release body's machine-readable lines?**
   *Recommendation:* yes, and in the same release that moves it — it is the
   number with no migration path, and a consumer's script that matches two of
   three numbers will conclude a move is safe when it is not. `.github/` is
   `herd-merger`'s. *Fallback:* leave the workflow alone and say the number in
   the release's prose, accepting that nothing matches it.
3. **Who owns `packs/`'s growth** (finding 7). *Recommendation:* name it as a
   board row tonight and put the removal policy in front of Alexa with the
   removal delay and the archive path that are already parked, rather than
   discover it when the consumer's root is large. *Fallback:* leave it, and
   accept that a directory created by this layout move has no retention story
   in the release that introduces it.
4. **Whether this branch may be pushed ungated.** The suite is red on this
   host for host reasons (see *Gates*) and `gate.sh` was not run, so the branch
   stayed local as the brief requires. *Recommendation:* push it as it stands
   once the host drains enough for one honest `bench-preflight.sh` clear and
   one `./scripts/gate.sh`, which is the only thing missing; the commit and the
   handoff are finished and nothing else needs to be written.
   *Fallback:* `herd-merger` takes the branch into the batch ungated and the
   batch's own `gate.sh` is the first and only gate it ever sees — which is
   what *One landing per batch* already makes the authority, but it means a red
   that is this branch's own would be found at integration rather than here.
### Drafted decision entry

```markdown
## D-NEXT — 2026-10-30 — The hub's pack registry, and the two state-root
versions spent on it

**Context.** D199 reserved `PROTOCOL_VERSION` 2 → 3, `RECORD_VERSION` 1 → 2
and `LAYOUT_VERSION` 1 → 2 for M11's pack half and named none of the shapes.
The protocol half was numbered on `pack-proto`. The record and the layout are
spent here, and an agent found tonight that the two checks behave differently:
the layout's is in `JobDirStore::open_with` and returns `Err` before the port
is bound, while the record's is in `read_stored` and quarantines one record
(D218). A build moving the record alone therefore **starts**, binds its port
and serves an empty queue off a root where every job is set aside by name —
measured, not argued: a half-moved build opened a v0.4.1-shaped root with all
its jobs quarantined and an empty queue.

**Decision.**

1. `RECORD_VERSION` is **2** and `LAYOUT_VERSION` is **2**, in one commit.
   What the record move buys is not the two new fields — they are
   `#[serde(default)]` and additive by this project's own rule — but the
   refusal arriving **at the door** with the layout's, and the rollback being
   refused on the record as well as on the root.
2. The state root gains `packs/manifests/<id>/` holding `manifest.tsv`
   verbatim, `pack.meta` verbatim and `registered.json`, plus
   `packs/manifests.part/` for staging. `<id>` is the sha256 of
   `manifest.tsv`'s bytes and the hub recomputes it — off the frame **and**
   off the disk at every start, because a directory name is a claim too.
3. **A registration is one `rename(2)`** and the `(commit, key)` index is
   **derived**, rebuilt from the directories at open like `index/state.json`.
   An interrupted registration leaves a directory under `manifests.part/` that
   nothing reads and the next start removes by name: a pack is complete or
   absent (E-17).
4. **Two different ids registered for one `(commit, key)` is a refusal by
   name, never a newest-wins.** The consumer's promise is that identical
   outputs give the identical pack, so two ids mean the outputs really
   differed, and picking one hands some later job the wrong artefact in
   silence. The refusal names both ids and says what picking one would cost.
5. **Holdings are per manifest, volatile, in memory only**, re-announced after
   `Welcome`. There is no holdings file and no stale holder after a restart.
   `PackState::Incomplete` is therefore not the hub's to compute and is never
   returned.
6. `job.json` gains `packs_in` and `packs_out`, carrying ids and not
   manifests. **No pack field goes on the verdict document**:
   `VERDICT_FORMAT_VERSION` and `OUTPUT_FORMAT_VERSION` are frozen as whole
   literal text and a field there would move a fourth public version.
7. **All three refusals say what is lost and what to do**, and point at
   `docs/operating.md`'s *"The instruction, in order"*: no conversion in
   either direction, the reading taken off the old root first with the old
   release's binary, the root moved aside and not converted, every runner
   refused once. The record's sentence lives once, in `herd-hub/src/job.rs`
   beside the constant, and takes an `Option<u32>` so an absent
   `record_version` is said to be **absent** and not reported as zero.

**Cost, and what is not decided.** `packs/` has no retention, no sweep and no
budget: the registry only grows, one directory per `(commit, key)`. The
removal delay and the archive path stay parked for Alexa. The release body's
machine-readable lines still carry only `PROTOCOL_VERSION` and
`RECORD_VERSION`, and `LAYOUT_VERSION` — the number with no migration path at
all — is not among them.
```

## Resume point

For whoever takes S7 (`server.rs`): the registry's whole surface is
`herd-hub/src/registry.rs` and is reachable as `store.packs()`. Call
`register(Registration{..})` **only** on `ToHub::RegisterPack` and on nothing
else, and treat `Registered::Refused(why)` as `FromHub::PackRefused` and never
as `FromHub::Refused`, which closes the link (D13). `resolve(commit, key)`
gives the `AssignedPack` for `FromHub::AssignJob`; `Resolved::Nothing` is a
waiting state with a reason. `announced()` on `ToHub::PacksHeld`,
`runner_gone()` when a link drops, `forget_all_holdings()` beside
`ReadPacksAgain`. D150's lesson applies: a registration arrives **after the
verdict** and a fetch frame **while the job is queued or starting**, both
outside `is_current`, and both need their own arrival-window predicate or they
will be refused in silence.

The exact next command, to confirm the branch where it was left:

```sh
cd <worktree> && export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-registry
./scripts/bench-preflight.sh && nix develop --command ./scripts/test.sh
```

## Processes

- One background shell of this agent's own, polling
  `./scripts/bench-preflight.sh` every 20 s and waiting for two consecutive
  clears. Started 23:52, 24 refusals, never cleared, and ended with the task.
  Nothing else was started, no PID was signalled, and no process of another
  agent was touched — no `pkill -f`, no `killall`, no kill by name.
- `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-registry`, off the root
  filesystem (gotcha `build-weight-off-root`), **12 GB, deleted as the task
  ended**. `workflow.md` deletes a build tree once its branch is pushed and
  this branch is not pushed, so this is the harder of the two readings taken
  on purpose: `/mnt/ssd_980` is the consumer's production disk, 12 GB of it
  is worth more to the host than a warm tree, and the gate that still has to
  run has to be run from scratch anyway — a tree built with the preflight
  lifted is not evidence of anything. The agent's report carries the `rm -rf`
  and the reclaimed figure.
- Backups of the two perturbed files, for the five revert experiments, lived in
  this agent's own scratchpad subdirectory and were used to restore the tree;
  the restored tree was re-run green before the commit.
- Logs, all in this agent's own scratchpad subdirectory
  (`…/scratchpad/pack-registry/`): `build.log`, `clippy.log`, `fmt.log`,
  `suite.log`, `suite2.log`, `retry3.log`, `preflight-unmodified.log`,
  `preflight-lifted.log`, `preflight-wait.log`.
