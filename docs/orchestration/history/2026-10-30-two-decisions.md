# two-decisions — handoff 2026-10-30

- **Item:** ORCH-218 (finding one). Finding two has **no board row**; this
  handoff drafts one, and the `D-NEXT` entry that says what is true instead.
- **Agent:** herd-step
- **Branch / commit:** `two-decisions` / `203ee85` (the delegation) and
  `baf17bf` (the registry note), with this handoff as the tip; based on
  origin/main `67bcd43`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**Commit 1, `203ee85` — the digest rule has one wording and one place
(ORCH-218).**

- `herd-runner/src/packstore.rs`: `object_name` was a full hand-rolled
  check with its own sentence; it is now
  `check_object_digest(sha256)?; Ok(sha256)` — a delegation to
  `herd_proto::pack::check_object_digest`, which is `pub` for exactly
  this. The function stays rather than being deleted because this module
  is the one that performs the join onto `objects/<sha256>`, so a digest
  reaching it with no parse behind it is still refused; what it is no
  longer is a second *statement* of the rule.
- The module doc said the check lives "here and not in `herd-proto`"
  because "`herd_proto::ManifestEntry` does not constrain the column's
  shape today". That sentence was false on this tree — `ManifestEntry` is
  built through `check_object_digest` — and it is corrected.
- The test `an_object_name_that_is_not_a_digest_is_refused_rather_than_
  joined_onto_a_path` asserted the shared substring; it now asserts the
  whole refusal, verbatim, in both halves. Old and new, side by side:

  ```rust
  // before
  why.contains("escaped") && why.contains("lowercase hex")
  // ...and, for the uppercase half, only:
  store.ingest(&dir).is_err()
  ```

  ```rust
  // after
  why.contains(
      "line 2 of this manifest: \"../../../escaped\" is 16 character(s) and a sha256 is \
       64 lowercase hex digits. This is the column Herd builds a path out of — \
       `objects/<sha256>` — so a value that is not a digest is a path outside the \
       pack's own store, and it is refused here rather than joined"
  )
  // ...and the uppercase half now asserts its whole sentence too, built
  // from the fixture's own digest.
  ```

- **A third reader had grown its own copy**: `herd-hub/src/registry.rs`
  `looks_like_an_id`, the filter over `packs/manifests/` directory names
  — the same sixty-four characters, a different *subject* (a pack id, not
  an object digest). Folded onto the same rule
  (`check_object_digest(name).is_ok()`), with the refusal's sentence
  discarded on purpose because that caller filters rather than refuses.
  It had **no test at all**; it has one now
  (`a_directory_under_the_manifests_that_is_not_an_id_is_ignored_and_kept`),
  over the three ways the rule can break — not hex, right shape wrong
  case, one character short — with the real pack beside them as the
  control.
- **The other two readers had not.** `herd-runner/src/endpoint.rs:356-362`
  and `herd-runner/src/fetch.rs:636-642` each carry a comment saying the
  digest was already refused upstream and that a further copy is how two
  come to disagree. Nothing was changed there.

**Commit 2, `baf17bf` — `herd-hub/src/registry.rs` says what is true
about the holdings (finding two).** No behaviour changed, because **no
call could be added**: see *Risks and findings* 1. The module gained a
section, *None of that is reachable from the network in this build*, and
`runner_gone` and `forget_all_holdings` each say they have no caller
outside this module's tests and why.

## Not done, and why

- **`runner_gone` is not wired to `State::release_runner` or to
  `one_watchdog_pass`.** It cannot be, from the paths this task owns: the
  server holds `Arc<dyn JobStore>` and `packs()` is on the concrete
  `JobDirStore`, not on that trait, so neither call site can reach a
  registry. The brief's premise that this is "adding a call" is refuted
  by the tree; the missing piece is a seam on `JobStore`
  (`herd-hub/src/job.rs`) and its implementation
  (`herd-hub/src/store.rs`), neither of which this brief allows. Nothing
  was reordered, and nothing in the watchdog was touched (D225, D228).
- **The re-announce half is not reachable at all**, and not for want of a
  caller in the hub: nothing in the whole repository constructs
  `ToHub::PacksHeld` or `FromHub::ReadPacksAgain` outside `herd-proto`'s
  own round-trip test, and the hub's connection loop has no arm for
  `PacksHeld` — a runner that sent one would be answered
  `FromHub::Refused { "a runner may not send packs_held" }` and its link
  closed. Proving "a runner announces, its link drops, the hub forgets"
  end to end would need the frame dispatch, the seam, and a runner-side
  announcement, two of the three outside this brief's paths.
- **No `docs/` file outside this handoff was touched**, and
  `docs/decisions.md` and `board.md` were not touched at all: the entry
  and the rows are drafted below.
## Gates

Two runs. The first is quoted below; it was made **before this file
existed**, over the tree of the two code commits minus one
reformatting. The second is `./scripts/gate.sh`, named at the end and
not quoted, because it runs on the tree this file is committed into.

**The four Rust gates and `check-docs.sh`**, 2026-10-30 04:00-04:12,
each phase `&&`-joined to a `bench-preflight.sh` clear, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/two-decisions`:

```
### fmt
Diff in …/two-decisions/herd-runner/src/packstore.rs:1623:
         let upper = herd_proto::sha256_hex(b"first").to_uppercase();
         let text = format!(
             "{}\n{}\t5\t0644\tf\tartefact\tbin/tool\n",
-            herd_proto::CHATONS_PACK_FORMAT, upper
+            herd_proto::CHATONS_PACK_FORMAT,
+            upper
         );
FMT EXIT 1
### clippy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 17.44s
CLIPPY EXIT 0
### build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.12s
BUILD EXIT 0
### suite
 38 assertion(s) DID NOT RUN AT ALL: they carry #[ignore].
 This is not 'this host could not' — it is 'nobody is checking this
 until someone acts'. A green run does NOT mean these were checked.
[38 HERD-IGNORED lines, none of them this task's]
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
SUITE EXIT 0
### check-docs
docs ok
DOCS EXIT 0
```

**`cargo fmt --all --check` was RED, on a line of mine**, in the test
this task rewrote: rustfmt wants the two arguments of one `format!` on
their own lines. It is reported red because it was red. Fixed with
`cargo fmt --all` and folded into commit 1 with `--amend`, so the only
difference between the gated tree and the committed one is that
whitespace. The fmt gate is re-run by `gate.sh` on the committed tree,
which is the named run below — the authoritative one for this branch.

**Skips: no `HERD-SKIPPED` line at all** — zero runtime skips on this
host for this run. **38 `#[ignore]`d assertions were printed as
ignored** by the census and are not counted as passes (D27); none is
mine, none is new, and every one carries a reason.

**The two lib figures the brief asked for, taken before anything was
written**, on `67bcd43` with a clean tree (04:52 lines in the log say
`clean = 0 changes`): `cargo test -p herd-hub --lib` **257 passed; 0
failed; 0 ignored**, `cargo test -p herd-runner --lib` **261 passed; 0
failed; 0 ignored**. After the two commits: hub **258 passed; 0
failed** (the one new test), runner **261 passed; 0 failed**.

**The last run, named and not quoted:** bare `./scripts/gate.sh`, never
inside `nix develop`, on the committed tree with this file in it. Its
verbatim output is in the agent's report; its receipt is
`target/herd-gate/<sha>.receipt` in this worktree
(`HERD_GATE_RECEIPTS` default), and only `gate: GREEN` would authorise
anything, which is `herd-merger`'s business and not this task's.
## Risks and findings

1. **The brief's premise for finding two is refuted: this is not a call
   that can be added.** `herd-hub/src/server.rs` never mentions the pack
   registry, and cannot: `State.store` is `Arc<dyn JobStore>`, `packs()`
   is an inherent method on `JobDirStore`, and `JobStore` has no
   pack-shaped method (36 methods, checked one by one). The tree says so
   itself, beside `packs()` in `herd-hub/src/store.rs:603-610`: *"Nothing
   calls this from the network yet. The pack frames … are not dispatched
   by the hub's connection loop; wiring them in is a separate step, which
   owns `server.rs`."* That step owns `job.rs` too, which nobody had
   noticed.
2. **It is wider than `runner_gone`: the whole registry is wired to
   nothing.** `.packs()` has **no production caller anywhere** — every
   one of its 20 call sites is in `herd-hub/tests/pack_registry.rs`. So
   `announced` is never called either, the holdings map is empty in every
   running hub, and **no runner has ever been named as a holder**. The
   merger's reading — *"a runner that dies stays a pack holder for ever,
   and the hub would name it as a holder to a runner that then cannot
   dial it"* — does not follow on this tree: there is no live defect,
   there is an unimplemented mechanism. That is the correction I owe the
   finding, and it does not make the finding wrong: `runner_gone` is
   still a promise, and D231 decision 5 still describes a design rather
   than this build.
3. **D231 decision 5 therefore needs two mechanisms and not one**, which
   is what the brief said to say clearly if nothing called for the
   announcement. Drafted as a `D-NEXT` entry below.
4. **The fold of `looks_like_an_id` crosses a subject boundary, and that
   is a judgement, not a measurement.** A pack id and an object digest
   are both "a sha256 joined onto a path", which is why one rule is
   right; but `check_object_digest`'s *name* and its sentences are about
   an object's digest column, and the registry now depends on them for a
   directory name. The cleaner shape is a rename in `herd-proto`
   (`check_sha256_name`, say, with `check_object_digest` kept as a
   delegating alias) — `herd-proto` is off limits in this brief, so it is
   named here and not done. Acceptance is unchanged either way: both
   hand-rolled checks accepted exactly 64 lowercase hex characters, and
   so does the shared one.
5. **Both new assertions were proven to fire before they were trusted.**
   Against a known-bad tree — `looks_like_an_id` returning `true`, and
   one word of the expected sentence changed to the drifted spelling —
   each test went red alone: `test result: FAILED. 0 passed; 1 failed`
   for `a_directory_under_the_manifests_that_is_not_an_id_is_ignored_and_
   kept`, and the same for the packstore test. Both reverted from a
   byte-for-byte copy before the gates.
6. **One stale sentence is left behind on purpose.** `store.rs`'s note
   beside `packs()` says the wiring step "owns `server.rs`"; it should
   say "and one seam on `JobStore`". `herd-hub/src/store.rs` is outside
   this brief's paths, so it is a finding rather than an edit.

## Drafted for `herd-merger`

### `docs/decisions.md` — a new entry, headed `D-NEXT`

```markdown
## 2026-10-30 — D-NEXT D231's volatile holdings need two mechanisms

**Context.** D231 decision 5 says a runner's pack holdings are "per
manifest, volatile, in memory only", "re-announced after `Welcome`", with
"no stale holder after a restart". An audit called the forget half *"a
promise"*; `herd-merger` measured at batch 12's landing that
`PackRegistry::runner_gone` and `PackRegistry::forget_all_holdings` have
no caller in `herd-hub/src/` outside their definitions and one unit test,
and read the consequence as a runner that dies staying a pack holder for
ever.

**What the tree says.** Measured 2026-10-30 over `herd-hub/src/`,
`herd-runner/src/`, `herdctl/src/` and the tests, with the search proven
on a frame that *is* dispatched. The consequence does not follow, because
the other half is missing too and the two absences hide each other:

1. **Nothing announces.** `ToHub::PacksHeld` is constructed nowhere
   outside `herd-proto`'s own round-trip test, and the hub's connection
   loop has no arm for it: the frame falls to `server.rs`'s `other =>`,
   which answers `FromHub::Refused { "a runner may not send packs_held" }`
   and closes the link (D13).
2. **Nothing asks for a re-announcement.** `FromHub::ReadPacksAgain` is
   constructed nowhere either.
3. **Nothing forgets, and not for want of a call site.** The two paths
   that know a runner is gone — `State::release_runner` and
   `one_watchdog_pass`'s link check — cannot reach the registry at all:
   `State` holds `Arc<dyn JobStore>` and `packs()` is on the concrete
   `JobDirStore`, not on that trait.
4. **It is the whole registry, not one method.** `.packs()` has no
   production caller anywhere; all 20 call sites are in
   `herd-hub/tests/pack_registry.rs`.

So the holdings map is empty in every running hub, no holder is ever
named, and no stale holder has been observed because none was ever
recorded. **There is no live defect here**; there is an unimplemented
mechanism, and D231 decision 5 describes its design rather than this
build's behaviour.

**Decision.** D231 decision 5 stands as the design and is **not** a
behaviour of this build. Making it true is one step with three parts, and
no part of it is a call added to an existing path:

* the pack frame dispatch in `herd-hub/src/server.rs`, `PacksHeld`
  included — the step `store.rs` already reserves;
* **one seam for the server to reach a registry through**, because
  without it `runner_gone` has no reachable call site at all: a
  `packs()` on `JobStore` returning `Option<&PackRegistry>` with a
  default body leaves every implementor, the two test doubles included,
  untouched;
* a runner that announces its holdings after `Welcome`
  (`herd-runner/src/lib.rs`), since a table emptied on a dropped link is
  only correct if a rejoining runner says again what it holds.

`runner_gone` and `forget_all_holdings` are therefore left as they are
and **documented as promises, with the measurement beside them**, rather
than wired to nothing or deleted. **Nothing was reordered**: D225 and
D228 own the watchdog's ordering, and D228 withdrew the obvious reorder
after measuring it, so a task that cannot add its call does not touch
that pass.

**Evidence.** `herd-hub/src/registry.rs` — the module section *None of
that is reachable from the network in this build*, and the notes on
`runner_gone` and `forget_all_holdings`; `herd-hub/src/store.rs:603-610`;
`herd-hub/src/server.rs` `release_runner`, `one_watchdog_pass`, and
`relay_from_runner`'s `other =>` arm at `:5032-5037`;
`herd-hub/src/job.rs:1961` (the trait's 36 methods, none pack-shaped);
`herd-proto/src/lib.rs` `ToHub::PacksHeld`,
`FromHub::ReadPacksAgain`; **D231** decision 5, **D225**, **D228**,
**D13**; `history/2026-10-30-two-decisions.md`.
```

### `docs/orchestration/board.md` — one row to close, one row to open

ORCH-218 moves `owed` → **`done`**, with its *Next action* cell replaced
by:

| ORCH-218 | *(item text unchanged)* | *(origin unchanged)* | herd-step | done | Done 2026-10-30 on `two-decisions`: `packstore::object_name` is now `check_object_digest(sha256)?; Ok(sha256)`, the test asserts the whole refusal verbatim in both halves instead of the shared substring `"lowercase hex"`, and **a third copy was found and folded** — `herd-hub/src/registry.rs` `looks_like_an_id`, the `packs/manifests/` name filter, which had no test and now has one proven red against `true`. The two readers that were already delegating (`endpoint.rs`, `fetch.rs`) were left alone. **Left as a finding:** `check_object_digest`'s name and sentences are about an object's digest while the registry now leans on them for a directory name; a `herd-proto` rename would read better and `herd-proto` was off limits | batch 12 landed | **D229**, **D230**, **D232**; invariant 4; `herd-proto/src/pack.rs:539`; `herd-runner/src/packstore.rs`; `herd-hub/src/registry.rs`; `history/2026-10-30-pack-fetch.md`, `history/2026-10-30-two-decisions.md` |

A new row in *Owed*:

| ORCH-NNN | **D231 decision 5 is a design and not a behaviour: the hub's pack holdings have neither half.** `.packs()` has **no production caller anywhere** (all 20 call sites are in `herd-hub/tests/pack_registry.rs`), so `announced` is never called and the holdings map is empty in every running hub; `ToHub::PacksHeld` and `FromHub::ReadPacksAgain` are constructed nowhere outside `herd-proto`'s round-trip test, and a runner that sent `PacksHeld` would be answered `FromHub::Refused` and dropped; and `runner_gone`/`forget_all_holdings` have no reachable call site because `State` holds `Arc<dyn JobStore>` while `packs()` is on the concrete `JobDirStore`. There is **no live defect** — no holder is ever recorded, so none goes stale — and no promise kept either | `two-decisions`, 2026-10-30, findings 1-3, which refute the premise that this is a call to be added; the duplication half of the same brief is ORCH-218 | herd-planner, then herd-step | owed | **Size it as one step with three parts**, because no two of them can be proven apart: the pack frame dispatch in `server.rs` (`PacksHeld` included), a `packs()` on the `JobStore` trait returning `Option<&PackRegistry>` with a default body (so the two test doubles stay untouched), and a runner that announces after `Welcome`. Then the acceptance the brief asked for becomes reachable: a runner announces, its link drops, the hub no longer names it as a holder, and a rejoining runner is asked for or sends its holdings again. **Add no call to the watchdog's ordering** — D228 withdrew that reorder after measuring it and ORCH-022 is Alexa's | batch 12 landed; D-NEXT above | **D231** decision 5, **D225**, **D228**, **D13**; `herd-hub/src/registry.rs`, `herd-hub/src/store.rs:603-610`, `herd-hub/src/server.rs`, `herd-hub/src/job.rs:1961`; `history/2026-10-30-two-decisions.md` |

## Decisions needed

**Does the `JobStore` trait gain a `packs()`, or does the hub carry its
pack registry beside the store?** — Recommendation: the trait method,
`fn packs(&self) -> Option<&PackRegistry> { None }`, with the default
body, because it changes no implementor and keeps one owner of the
registry's lifetime. — Fallback: `State` takes an
`Option<Arc<PackRegistry>>` at `bind`, which `main.rs` fills; cheaper to
write, but it puts the registry's lifetime in two places and `bind`'s
signature then carries a pack argument for every caller including the
tests. Either way it is one line of product and it is **not** this
brief's to write.

## Resume point

The branch is pushed and the two commits stand alone. Next, in order:

1. `herd-merger` lands the branch with the batch, allocates the `D-NEXT`
   number, closes ORCH-218 and opens the new row above.
2. The new row is a task for `herd-planner`: three parts, one step, as
   sized above. Nothing in it is a reorder.

## Processes

PIDs started: none that outlived a command. Every heavy phase ran in the
foreground of one `bash` invocation (`cargo`, `nix develop`,
`scripts/test.sh`, `scripts/gate.sh`); none was backgrounded as a
service, and nothing was killed. The first consumer's hub and runner
(3006543, 3021002) were not touched, and no `pkill` was used.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/two-decisions`, off the
root filesystem; it is removed by literal path once the branch is on the
origin, per `gotchas/build-weight-off-root.md`.
