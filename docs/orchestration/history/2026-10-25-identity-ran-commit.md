# identity-ran-commit — handoff 2026-10-25

- **Item:** C3 — `ran_commit` (Alexa, D-4, 2026-10-25)
- **Agent:** herd-step
- **Branch / commit:** `identity-ran-commit`, based on `origin/main` f507ab6
- **PR:** none — `herd-merger` opens it

## Done

A verdict can now be contradicted by the record it sits on. There was no
oracle for I01 before this; there is one now.

**The protocol** (`herd-proto`).

- `RAN_TOKEN = "{ran}"`, an eighth entry in `TOKENS`, filled by the runner
  like `{job}`, `{in}` and `{out}`. It names a **path**, not a value.
- `StepRun::ran_commit: Option<String>`, `#[serde(default,
  skip_serializing_if = "Option::is_none")]`.
- `CommitRun { declared: String, ran: Option<String> }`, with `diverged()`
  and `line()`. `ran` is serialised **even when `None`**, as an explicit
  `null`, so silence cannot be missed by reading only the keys that are
  there.
- `FromHub::JobFinished` gains `commit: Option<CommitRun>`.

**The runner** (`herd-runner`).

- `Workspace::ran_path()` → `<job base>/ran`. Named, never created: a file
  the runner created would be an empty answer where there was no answer.
  Beside `work` and not in it, because a file Herd puts in the middle of a
  checkout is an untracked file in somebody's repository.
- `step::planned` fills `{ran}` in every hook, in the environment block and
  in the declared line. Its three path arguments became one `Places` struct
  (clippy's `too_many_arguments` at 8).
- `step::ran_commit(path)` reads the answer back: at most 4 KiB read, first
  line, trimmed; empty or absent → `Nothing`; longer than 128 characters,
  not printable ASCII, or not text → `Unusable(why)`. Nothing is parsed,
  nothing is lowercased, no shape is assumed — a `svn:r48217` comes home
  untouched (E-11).
- Read **after the `fetch` step and no other**, and attached to that step.

**The hub** (`herd-hub`).

- `JobDirStore::record_step` compares, on every `fetch` of a declared
  command, and writes `Attempt::commit = Some(CommitRun { declared, ran })`
  — including when `ran` is `None`, so the record says "did not report"
  rather than leaving a blank.
- A divergence is **recorded, never refused**: state, outcome, exit code and
  requeue budget are untouched.
- `the_commit_it_ran(state, job_id)` reads it back off the record for the
  verdict frame, at all six places one is built — the same reason
  `read_the_outputs` does: a `herdctl wait` an hour later gets the same
  answer as the requester who followed the job.

**The requester** (`herdctl`).

- `Finished::commit`, and three always-present fields on the verdict
  document: `declared_commit`, `ran_commit`, `commit_diverged`
  (`null`/`false`/`true`, where `null` is "the comparison could not be
  made"). `VERDICT_FORMAT_VERSION` does not move — that module's own rule
  is that adding a field does not.
- One stderr sentence on a divergence, kept in `said` like the others. The
  exit code is untouched (D20).

**`docs/decisions.md`** — one entry headed `D-NEXT`, dated 2026-10-25, with
the rollback cost, the token-versus-environment-variable argument and six
rejected alternatives written down.

## Not done, and why

- **`docs/coverage.md` has no I01 line.** It is the right place for "the
  oracle now exists", but the brief's write paths did not include it and a
  second agent may be in that file. A finding, not an edit.
- **No `herdctl --require-commit`.** Deliberately a second decision, to be
  taken on divergences this field makes visible rather than before there are
  any.
- **No new `events.jsonl` variant.** See *Risks*: it is the one addition to
  the record an older hub could not read.
- **`docs/remediation/04b` §3.5 is not corrected in place** (out of scope by
  the brief). Its second matrix is headed "old `herdctl` / new record" and
  `herdctl` never reads a record. The cell's answer is right and its
  artefact does not exist; the real case is a *frame*, and it is asserted as
  one in `an_older_herdctl_decodes_a_verdict_carrying_the_new_key`.

## Gates

`nix flake check --no-build` → `all checks passed!` (exit 0).

`cargo fmt --all --check` → clean.

`cargo clippy --workspace --all-targets -- -D warnings` → clean
(`Finished dev profile`, no diagnostic).

`./scripts/test.sh` → `test suite: PASSED`, every binary `0 failed`. Run
twice: once at a one-minute load of 18.8 (a backup, and above the brief's
ceiling of 8 — noticed after it had started) and once again at 3.9. Both
passed with the same figures, so nothing here is load-sensitive; the second
run is the one to believe.

Skips: **12**, the reference count — 10 for cgroup v2 delegation being
unavailable to this user, 2 for a ZFS data root where no cgroup IO knob
bites. Unchanged by this work; none of them is in the paths touched here.

`./scripts/check-docs.sh` → **fails, on one line and only this one**:

```
docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates
the number at integration:
8367:## 2026-10-25 — D-NEXT A verdict says which revision it really ran
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost)
docs check failed
```

Verified to be the only cause: a copy of this tree with `D-NEXT` replaced by
`D174` prints `docs ok`. The `D170` line is a `note:`, not a failure.
`./scripts/check-orchestration.sh` → 86 passed, 1 failed, and the one
failure is `unmodified copy passes`, which runs `check-docs.sh` and fails
for the same D-NEXT.

New tests, all green:

- `herd-hub/tests/ran_commit_across_versions.rs` — 12 tests, the cross-
  version matrix in both directions and the loss.
- `herd-runner/tests/declared_command.rs` — 4 new, real hub + real runner +
  real shells: agreement, divergence, silence, an unusable answer.
- `herd-runner/src/step.rs` — 2 unit tests (filling, and the reader's seven
  cases).
- `herd-proto` — 4 (the three states, silence as `null`, an older step, the
  revision crossing the wire on both frames).
- `herdctl/src/verdict.rs` — 2 (the three states in the document, and the
  three keys always present).

## Risks and findings

- **F — a rollback reads the field and erases it.** A hub of an earlier
  version reads a record carrying `ran_commit`/`commit` perfectly well, and
  drops both the first time it writes that record back; serde has nothing to
  re-emit for a field it never decoded. Reachable through `record_state` on
  any job in flight and through `note_outputs_fetched`
  (`herd-hub/src/store.rs`), which rewrites a **finished** job's whole
  record at the first fetch of its outputs. Asserted in
  `an_older_hub_reads_the_field_and_erases_it_on_its_next_write`, with the
  mirror assertion that a hub that *knows* the field keeps it. This is a
  cost, not a defect, and the decision entry states it.
- **F — the record's `events.jsonl` is not additively safe, unlike its
  fields.** `JobEventKind` (`herd-hub/src/job.rs`) is `#[serde(tag =
  "event")]` with **no `#[serde(other)]`**, so a new variant would make
  `events.jsonl` unreadable to an older hub — a strictly worse break than
  the field loss above. That is why a divergence is a field on the attempt
  and not an event. Worth a line in the project's additive rule, which today
  talks about fields only.
- **F — an older runner given a declaration naming `{ran}` is harmless.** It
  leaves the brace in the hook, `sh` treats `{ran}` as the filename it is,
  the hook exits 0, and the runner reports nothing. Asserted with a real
  `/bin/sh`. The same test asserts the counter-case that decided the design:
  `> "$HERD_RAN_COMMIT_FILE"` with the variable unset **fails the step**,
  which would have made every job of that consumer a setup failure on every
  machine not yet restarted.
- **H — one extra record read per verdict.** `the_commit_it_ran` reads the
  record where the verdict is built. It is once per *finished job*, beside a
  `read_the_outputs` that already reads, and not on the dispatch pass 04a
  and 05 identify as the hot path. What would settle it: `cost_surface.rs`
  case 4's counters before and after. Not measured here.
- **I — whether a divergence should eventually refuse.** Not answerable
  before there is a population of recorded divergences, which is what this
  increment creates.
- **Finding against the brief:** none of its premises was refuted. Every
  checked claim held — no `deny_unknown_fields` in `herd-proto` or
  `herd-hub`, `#[serde(other)]` on both frame enums, `Vec<StepRun>` on the
  record, both version numbers unmoved. Two things the brief did not say are
  in *Risks* above (the events enum; the environment-variable asymmetry).

## Decisions needed

1. **Does `herdctl` eventually gain `--require-commit`?** Recommendation:
   not yet — decide it on the divergences this increment records. Fallback:
   the consumer's own wrapper branches on `commit_diverged`, which it can do
   the day this ships.
2. **Should `docs/coverage.md` gain an I01 line now?** Recommendation: yes,
   by whoever owns that file, saying the oracle exists and that it records
   rather than enforces. Fallback: leave it; this handoff and the decision
   entry carry the fact.

## Resume point

`herd-merger`: allocate the number for the `D-NEXT` entry at
`docs/decisions.md:8367`, re-run `./scripts/check-docs.sh` (it then prints
`docs ok`), and open the PR. Nothing else is pending on this branch.

## Processes

None started that outlived a command. Every `cargo`/`nix` invocation ran in
the foreground and exited; no `pkill`, no kill by name, no background
runner or hub. Build tree: `/mnt/ssd_980/herd/build/c3-ran-commit`, removed
at the end.

---

## Annexe A — the containment line, written for chatOnS

Available **today**, with nothing shipped and nothing changed on their
machine. One line in their own declaration file, in the `fetch` hook:

```toml
fetch = "git fetch -q origin && git switch -q --detach {commit} \
         && [ \"$(git rev-parse HEAD)\" = \"{commit}\" ]"
```

What it buys: a `fetch` that ends anywhere other than the declared commit
now **fails**, so the job never starts and comes back as a preparation
failure at exit 125 — instead of running the tests on the wrong tree and
coming back as a green verdict about a commit that was never checked out.

What it does not buy: the guarantee lives in their file, not in Herd. Herd's
record still says nothing, so Herd still cannot contradict a verdict after
the fact, and a day when somebody edits that line is a day the protection
goes away with nobody noticing. It is containment, not a fix — worth doing
whatever else happens, because it protects the next build and `ran_commit`
protects the one after the deployment.

## Annexe B — impact statement for chatOnS

**What changes for you.** Nothing, unless you want it to. Jobs behave
exactly as they do today: same steps, same exit codes, same verdicts, same
records. No field you read disappears or changes meaning.

**What is new.** Herd can now record *which revision your `fetch` really
reached*, beside the commit the job was submitted for, and tell you when the
two differ.

**What you have to do to get it.** One token in your `fetch` hook. Herd
replaces `{ran}` with a path inside the job's workspace; write the revision
there, one line:

```
fetch = "... && printf '%s\n' \"$(git rev-parse HEAD)\" > {ran}"
```

Then every verdict document (`herdctl run|wait --verdict-to FILE`) carries
three more keys:

```json
"declared_commit": "cb5a…",
"ran_commit":      "9f41…",
"commit_diverged": true
```

and the job's record carries the same thing on its attempt. `ran_commit` is
`null` when nothing was reported, and `commit_diverged` is `null` — never
`false` — when the comparison could not be made. **`null` is not
agreement.** Branch on `commit_diverged === true`.

**What happens if you do nothing.** Nothing. The hook writes no file, the
runner reports no revision, the record says "this runner did not report a
revision" in those words, and every verdict you read keeps the shape it has
today with three `null`s added. No job fails for this, now or later, without
a decision taken with you.

**What a divergence does, in this first increment.** It is **recorded, not
refused.** A job whose tree was at another revision still passes if its
tests passed, still wears its own exit code, and still returns its outputs.
What changes is that the record can now contradict it — which is exactly
what was missing when a green verdict was attributed to a commit nobody had
checked out. Whether a divergence should also *stop* a job is a separate
decision, to be taken on real divergences rather than before there are any.

**One constraint on the string.** Herd does not know what produced it and
never parses it: one line, printable, at most 128 characters, compared with
your declared commit byte for byte. Anything else is treated as if you had
written nothing, and the job's log says why.

**Order of operations.** Updating your declaration before every machine in
the fleet has the new runner is safe: an older runner leaves `{ran}`
unfilled, the shell writes a file of that name into a workspace that is
about to be deleted, your `fetch` succeeds, and nothing is reported. Nothing
breaks and nothing fails.

**And the thing you can do before any of this.** Annexe A: assert the commit
in your own `fetch`. It is one line, it works today, and it turns a wrong
verdict into a preparation failure.
