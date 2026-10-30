# pack-store — handoff 2026-10-30

- **Item:** M11's pack half, steps **S2** (the runner's content-addressed
  pack store) and **S3** (ORCH-153: the pack leaves `{out}` before the
  record walk), of the plan of 2026-10-29. Two steps in one branch because
  they are sequential and each would otherwise pay its own full gate.
- **Agent:** herd-step
- **Branch / commits:** `pack-store` / three commits (two of code, one
  this handoff) on top of the merge of
  `pack-proto` and `origin/main`; based on `pack-proto` `adbc204`, merged
  with `origin/main` `1a8c1ae` as `49563fd`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

### The base, and both merges

`git worktree add -b pack-store <wt> pack-proto` then
`git merge origin/main`. **Both clean, no conflict.** The merge brought
batch 9's `herd-runner/src/outputs.rs` (the measured byte count that now
reads exactly, through `herd_runner::size::measured_bytes`),
`herd-runner/src/size.rs`, `sweep.rs`, `confinement.rs` and
`herd-hub/tests/spool_offer_answered.rs`: 13 files, 2 711 insertions.

### Part one — `herd-runner/src/packstore.rs`, new

A content-addressed store under the runner's data root, in the shape the
consumer already uses. **Two layouts, and the difference is the point of
the store**, which is why both are in the module's own doc:

A **pack directory** — what a job leaves at its declared `leaves` path
under `{out}`, and what a later job is given at `{in}/<key>`, read one way
and written the other:

```text
<pack>/manifest.tsv      "# chatons pack 1", "# profile debug", then per row:
                         sha256 TAB size TAB octal mode TAB kind f|d|l TAB role TAB path [TAB link target]
<pack>/pack.meta         commit, profile, scenarios — not part of the id
<pack>/objects/<sha256>  each file's bytes once, under its own sha256
```

The **store's own** layout, which is not that shape, because objects are
shared across packs:

```text
<data root>/packs/objects/<sha256>              every pack's bytes, once per content
<data root>/packs/manifests/<id>/manifest.tsv   the bytes, verbatim
<data root>/packs/manifests/<id>/pack.meta      when the pack had one
```

The root sits at `<data root>/packs`, beside `jobs/`, `held-results/` and
`outputs/`, and is created by `PackStore::open(<data root>)`. **A runner
that has never been given a pack has no `packs/` directory at all** —
asserted, because two empty directories on every runner's disk would be a
feature announcing itself before it exists.

The interface is four operations and nothing else: `ingest(dir)`,
`missing(manifest)` / `holds_whole(manifest)`, `lay_out(manifest, at)`,
and `leave_packs(out_dir, data_root, leaves)` for part two. `manifest_of`
and `meta_of` read a stored pack back. **Nothing binds, dials, fetches or
serves**, and the module doc says so in those words, so that the module's
existence is not read as a working pack feature.

### Part two — the pack is gone from `{out}`, not excluded from it

`outputs::collect` now takes the `leaves` map and its **first act** is
`packstore::leave_packs`, which ingests each declared path and removes it;
the record walk is in `collect_record`, which has no other caller and
receives `{out}` only after that has returned. So the ordering is stated
once, in one place, and the pack is **absent** rather than filtered: an
exclusion list is a thing a later edit silently breaks, and an absence is
not.

`Collected` grew a `packs: Vec<LeftPack>` field — the ingest-and-hand-back
the step that owns the registration frame calls. `job.rs` prints one line
per pack saying it is in the store and **not** registered with the hub.

## Not done, and why

1. **No frame, no socket, no fetch, no registration.** As briefed. The
   runner stores a pack and hands it back; nothing tells the hub.
2. **`declared_leaves(declaration)` in `job.rs` returns an empty map
   today.** `leaves` and `needs` are read by `herdctl`
   (`herdctl::declaration::Resolved::leaves`) and carried on no frame:
   `herd-proto` is not this step's to touch, and the fields and variants
   that carry them are frozen in `herd_proto::pack` awaiting the fold-in
   step. The seam is a named function with a doc comment rather than an
   inline `BTreeMap::new()`, and the extraction it feeds is tested
   directly, so what is missing is one field's arrival and not the
   behaviour.
3. **No process was killed in the middle of an ingest.** A kill at a
   chosen instant cannot be staged deterministically from a unit test and
   a timing-based one is a flake, which erodes invariant 5 rather than
   proving anything. What is proved instead is in *Done* above and in the
   test's own doc comment: bytes that are not the bytes a name promises
   never appear under that name, a failed ingest files no manifest, and a
   staging file left behind by a kill is counted by nothing.
4. **`docs/plan.md`'s stale sentence** — the pack path "unspecified, to be
   settled before any pack code is briefed" — is **not** amended here:
   `plan.md` is on this brief's do-not-touch list and a separate task owns
   it.

## Gates

**NOT RUN. No Rust gate, no test, no `gate.sh`, and the branch is NOT
pushed.** `./scripts/bench-preflight.sh` refused on every reading taken
across roughly an hour and it is a gate and not advice, so no suite was
started. Readings, verbatim in the agent's report: load 4.23–17.23 on
twelve cores against a 9.00 ceiling, slice room 123–4 018 MiB against the
8 192 MiB floor, oscillating by a factor of thirty inside a minute.
`/sys/fs/cgroup/chatons.slice/memory.stat` was read directly, as the brief
allows: `anon` 871–3 630 MiB against `file` at 13.7–15.6 GiB, so part of
each refusal was reclaimable page cache and part was a sibling agent's
real memory — and the **load** reading was independently over the ceiling
for nearly all of it, which no reading of `memory.stat` excuses. Nothing
was written under `/sys/fs/cgroup` and no cache was dropped.

What **was** run, and all of it is cheap and single-core:

```
$ nix develop --command cargo fmt --all
(no output)   exit 0

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit 0
```

**One compile was run past a refusal, and that was wrong.** Disclosed in
full: `nix develop --command cargo check -p herd-runner --lib`, once, with
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB=0` set **inline on that one command**
after reading `memory.stat` and finding the room refusal cache-dominated
(`anon` 1 193 MiB against 14.8 GiB of `file`). By the time it ran the load
reading had risen to 9.81 against the 9.00 ceiling, so the preflight
refused on a **second** ground that the cgroup reading does not speak to,
and it should have stopped there. It found exactly one error — a missing
`packs` field in `collect`'s spool branch — which is fixed in this tree.
No override is set in any environment now, and none was ever exported.

**So every acceptance case in this branch is unproven.** The tests are
written and they have never been executed. The next agent's first act is
the gate, not a change.

**Skips:** none observed, because nothing that reports a skip was run.

## Risks and findings

### 1. `ManifestEntry.sha256` is unvalidated and becomes a path

`entry_of_row` (`herd-proto/src/pack.rs:444`) parses the size, reads the
kind and refuses an unknown letter — and carries column 1 **as written**.
A manifest row whose first column read `../../../escaped` would have named
a path outside the store. `herd-proto` is not this step's to change, so
the check is here: `packstore::object_name` requires exactly sixty-four
lowercase hex characters before anything is joined onto `objects/`, and
refuses by name otherwise. Uppercase is refused too — one spelling, or the
store holds the same bytes twice under two names.

**Recommendation:** the constraint belongs on `ManifestEntry`, in
`herd-proto`, where every reader of a manifest gets it. Until then the
runner's store is the only code that joins that column onto a path, so
nothing is unguarded today; the hub's registry (S6) and the fetch endpoint
(S5) will each need the same check, and three copies of it is how two of
them come to disagree.

### 2. Objects are hashed on ingest, which costs one read of the pack

A content-addressed store whose `objects/<sha>` does not hash to `<sha>`
is a lie the next machine cannot detect, because it asks for the name it
trusts. So every object is hashed as it is placed. The consumer's own
figure for verifying 401.6 MiB is 1.5–2.1 s, and the bytes have just been
written so they are in page cache. The cost is paid once per content, not
once per pack. Named because it is a decision and not an implementation
detail: a later step may want it configurable, and S12's measurement
should carry it.

### 3. A declared `leaves` path leaves `{out}` even when the ingest fails

Decided here because the plan does not say. See *Decisions needed* 1.

### 4. A size disagreement is refused rather than repaired

An `objects/<sha>` present at a size the manifest does not declare means
one of the two figures is wrong, and overwriting would let a bad manifest
destroy a good object. The refusal names the file and says that removing
it lets the next ingest place it again — actionable, but it is a state a
runner does not clear by itself, and M12's eviction work should know about
it.

### 5. The staging name has two load-bearing properties, one untested

`.ingest-<pid>-<counter>` can never be read as a 64-hex object name (the
leading dot, the hyphen), which is what makes a stray staging file
uncountable; and it is unique per process and call, which is what keeps
two ingests of one pack from sharing a half-written file. The first is
asserted. The second is argued from the counter, because two concurrent
ingests in one process is not a shape anything reaches today.

## Decisions needed

1. **A declared pack path that cannot be ingested: removed, or left under
   `{out}`?** Decided here as **removed, with the reason and the figures
   named** on the record's `skipped` list and on the machine's stderr.
   Reasoning: a declared pack is outside the record *by declaration*, so
   it is outside it in both outcomes; and left where it is, one pack Herd
   cannot read is ~400 MiB of ordinary output that puts the whole
   collection over the cap, so `results.json` does not come back either —
   one failure becoming two, the second silent. *Recommendation:* keep.
   *Fallback:* keep the bytes under a new `packs/refused/` area, which
   needs a retention policy nobody has decided and which the plan already
   parks for Alexa (the removal delay, the archive path).
2. **`{out}/<leaves path>` is the pack directory itself, not a directory
   holding one.** The declared path holds `manifest.tsv`, optionally
   `pack.meta`, and `objects/`; anything else is refused by name. The plan
   gives the three-part shape and does not say which level the declared
   path points at. Decided for symmetry with `{in}/<key>`, which the plan
   *does* fix as the three-part shape. *Fallback:* accept a single
   subdirectory that is itself a pack — which is a rule Herd would have to
   invent and a guess the brief told me not to dress as a decision.
3. **Whether the hub's registry (S6) and the fetch endpoint (S5) get
   finding 1's check, or whether `herd-proto` gets it once.**
   *Recommendation:* `herd-proto`, in a one-line follow-up owned by
   whoever next holds `pack.rs`.

### Drafted decision entry

```markdown
## D-NEXT — 2026-10-30 — the runner's pack store, and the pack is gone from
`{out}` before the record is collected

**The store.** A runner keeps packs under `<data root>/packs`:
`objects/<sha256>` once per content for every pack it holds, and
`manifests/<id>/manifest.tsv` — the bytes, verbatim — per pack. That is not
the shape of a *pack directory*, which is the consumer's three parts
(`manifest.tsv`, `pack.meta`, `objects/<sha256>`, measured by them on
2026-10-13 at 401.6 MiB in 557 files); the store shares objects across packs,
which is the whole reason it exists, and a pack is laid out for a job as hard
links into `objects/`. Handing a 400 MiB pack to a job therefore costs the
directory entries and not the bytes, and the figure that says so is reported
rather than assumed.

**Complete or absent (E-17).** An object is linked or copied under a staging
name, hashed there, fsynced and renamed, so a partial object is never visible
under its final name: a later completeness check would count it and a pack
missing those bytes would read as whole. The manifest is written **last**, so
an ingest killed anywhere leaves objects — each whole, or not there at all —
and no pack.

**Every object is hashed as it is placed.** A content-addressed store whose
`objects/<sha>` does not hash to `<sha>` is a lie the next machine cannot
detect, because it asks for the name it trusts. The cost is one read per
content, against the consumer's own 1.5–2.1 s to verify 401.6 MiB.

**The sha256 column is checked before it is joined onto a path.**
`herd_proto::ManifestEntry` carries column 1 as the consumer wrote it, so a
row reading `../../something` would otherwise name a path outside the store.
Sixty-four lowercase hex characters and nothing else, refused by name.

**ORCH-153: the pack is gone from `{out}`, not excluded from it.** Each
declared `leaves` path is ingested and removed before the collection walk
reads the directory. Not a fifth entry in the record half's exclusion list:
an exclusion is a thing a later edit silently breaks, and what it would break
is a ~400 MiB pack collected as a job's output evidence and streamed runner →
hub → requester as if it were a result — the record half doing exactly its
job, with no error anywhere and very likely the whole collection refused over
the output cap so that `results.json` does not come back either. An absence
cannot be broken that way. `docs/plan.md`'s "unspecified, to be settled
before any pack code is briefed" is thereby settled.

**A declared path leaves `{out}` whether or not it could be stored**, and the
failure is named with its figures on the record and on the machine's stderr. A
declared pack is outside the record by declaration; left where it is, one pack
Herd cannot read would refuse the whole collection for it.
```

## Resume point

**The gate, first, before any change.** Nothing in this branch has ever
been executed.

```sh
cd <scratchpad>/pack-store/wt   # branch pack-store, 3 commits, NOT pushed
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-store
./scripts/bench-preflight.sh && \
  nix develop --command cargo test -p herd-runner --lib packstore outputs
```

Then the four Rust gates, `./scripts/check-docs.sh`, and bare
`./scripts/gate.sh` — never inside `nix develop`, which nests `TMPDIR` and
panics `confinement.rs` on the 108-byte `SUN_LEN`. Push only on a green
gate.

The next steps on this branch's shoulders are the plan's **S9** (the peer
fetch client and D10's three paths) and the fold-in step that puts the
thirteen payloads on `ToHub`/`FromHub`: S9 reads `Collected.packs` for the
registration frame and calls `PackStore::lay_out` for `{in}/<key>`, and
the fold-in is what makes `declared_leaves` return something. Neither
needs a change to this module's interface.

## Processes

Two background watchers, both started by this agent and both read-only
polls of `/proc` and `/sys` every twenty seconds: the
`bench-preflight.sh` until-loop waiting for two consecutive clears
(`<scratchpad>/pack-store/wait-for-room.sh`, stopped by this agent so the
host was polled once rather than twice), and its replacement, stopped at
hand-off. No `pkill`, no kill by name, no PID of any other agent touched.
Nothing of `/etc/nixos`, `/srv`, the consumer's data or the chatOnS
project; nothing written under `/sys/fs/cgroup`; no cache dropped.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-store`, outside the
worktree, deleted with `rm -rf /mnt/ssd_980/herd/build/pack-store` once
the branch is pushed (gotcha `build-weight-off-root`). **Not deleted
yet**: it holds the dependency check of this tree, which the next gate run
would otherwise pay for again, and the branch is not pushed.
