# 04b — Integrity and containment (experiment 6.3)

**Date:** 2026-10-24
**Base:** `origin/main` at `0a2cf73`.
**Scope:** the remediation dossier's experiment 6.3 — two measurements and
one proposal. (a) what a requester's exit code says when a verdict's inline
bytes contradict their own digest; (b) how many jobs one runner takes in
hand when the thing handing them out is not the hub; (c) how a verdict could
be tied to the commit that was really executed. Alexa approved this
experiment and only this experiment.
**What this is not:** no product code was changed. The two measurements are
**bench files** that assert nothing about the figures they print, and (c) is
a **proposal on paper**. No protocol field was added and none may be until
Alexa decides.

**Evidence standard.** Every claim carries **F** (measured here, or read at a
named `file:line` on `0a2cf73`), **H** (hypothesis, with what would settle
it) or **I** (unknown). An `I` is an answer; a badly supported `F` is not.

**The bench files.**

* `herdctl/tests/inline_digest_mismatch.rs` — (a). A hand-written runner, a
  real hub in process, the **real `herdctl` binary** as a child, because an
  exit code is a property of a process.
* `herd-hub/tests/fake_hub_flood.rs` — (b). A hand-written **hub**, the real
  `herd_runner::serve_once`, jobs that are `sleep` and nothing else. It sits
  in `herd-hub` and not beside the runner because it binds a listening
  socket, and `no_module_of_the_runner_can_bind_a_socket`
  (`herd-runner/tests/confinement.rs:1638-1668`) forbids the string
  `TcpListener` anywhere under the `herd-runner` crate, tests included.
  Invariant 3 is the reason and the rule is right; the file moved rather
  than the rule. **F**, found by the gate, not by reading.

Both are named `measure_*` and both only print. Pinning a number would
commit Herd to behaviour nobody has decided on.

---

## 0. The five things that matter

1. **(a) is confirmed. `herdctl` exits 0.** F. A verdict carrying two inline
   files, one of which contradicts its own sha256, produces exit code **0**,
   a manifest with **one** entry of two, one file under `--outputs-to`, and
   a sentence on stderr that neither the exit code nor the verdict document
   carries. §1.
2. **The one bad path and the one good path differ in one line of code and
   in nothing a caller can see.** F. The refused-path case is intentional and
   written down (`herd-hub/src/job.rs:1823-1826`); the digest case has no
   such sentence and contradicts the rule stated a few lines further down its
   own function (`herd-hub/src/store.rs:1717`). Both exit 0. §1.4.
3. **(b) is confirmed, at both figures and with no refusal at either.** F.
   50 assignments → 50 jobs in hand; 250 → 250. The runner's own health
   report says 50 and 250. It never said no. §2.
4. **And it stayed responsive the whole time.** F. The control link answered
   a probe in **360 µs** at 50 jobs and **237 µs** at 250, against a 2 000 ms
   budget. Invariant 2's *"stops cleanly"* half held; its *"never
   destabilizes its host"* half was not tested, because the jobs were
   `sleep`. §2.3 says what that does and does not license.
5. **(c) needs no protocol break.** F. Herd has a written additive rule — a
   new `#[serde(default)]` field whose absence means exactly its default
   moves neither number (`docs/decisions.md:7293`, `herd-hub/src/job.rs:44-51`)
   — and both frame enums carry a `#[serde(other)] Unknown` catch-all with no
   `deny_unknown_fields` anywhere on the wire. So the commit can come home on
   `StepRun` with `PROTOCOL_VERSION` at 2 and `RECORD_VERSION` at 1, and
   **no migration**. §3.

---

## 1. (a) A verdict whose inline bytes lie about themselves

### 1.1 What was done

A hand-written runner joins a real hub, takes a real job submitted by the
real `herdctl`, and answers with a `JobFinished` carrying a collection of two
inline files:

* `files/results.json`, 11 bytes, with the sha256 of `{"ok":true}` and the
  bytes of `{"ok":fals}` — same length, so the length check is not what
  trips, and only the digest is wrong;
* `files/honest.txt`, 17 bytes, honest.

The requester is `herdctl run --outputs-to <dir> -- /bin/sh -c 'exit 0'`, run
as a child process, and what is read is the status the shell would read.

A real runner cannot produce that frame: it computes the length and the
digest from the same buffer it sends (`herd-proto/src/outputs.rs:213-220`).
That is why the runner here is hand-written, exactly as `herd-hub/tests/link.rs`
does for the same reason. **H:** the realistic triggers are a runner of a
different build, a corrupted spool read, or memory corruption — low
frequency, and the consequence is the one measured below.

### 1.2 What was measured — F

```
herdctl exit code: 0
manifest entries on the record: 1
record manifest: [KeptFile { path: "files/honest.txt", bytes: 17,
                             sha256: "7f04d2cd…7137" }]
record skipped: ["a frame this hub will not believe: files/results.json:
                  the frame says sha256 4062edaf… and carries 802f7e1d…"]
top level of --outputs-to: ["files"]
herdctl stderr:
  herdctl: this job's output: a frame this hub will not believe: …
  herdctl: wrote <dir>/files/honest.txt (17 bytes)
```

**The audit's prediction was exactly right.** A requester asked for a job's
output files, was handed one of the two the runner sent, and read **0** — the
one code that means "everything you asked for happened"
(`herdctl/src/main.rs:578-579`, the comment's own words).

### 1.3 The mechanism, confirmed line by line — F

1. `record_outputs` pushes the manifest entry **after** the digest check, so
   a `continue` at `herd-hub/src/store.rs:1715` drops the entry entirely;
   the entry would have been pushed at `:1726`.
2. Two lines under that `continue` — six under the `if` at `:1711`, which
   is the line the brief names — at `herd-hub/src/store.rs:1717`, the
   function states the opposite rule in bold: *"**The manifest first, the
   bytes if they are here.** … What a reader of `job.json` must never find is
   a path that is not there *and* no way to tell why."* The same rule is
   written three more times — `store.rs:1773-1778`,
   `herdctl/src/lib.rs:856-859` — and every other absence path honours it.
3. `herdctl` counts the shortfall **off the manifest**:
   `missing = outputs.files.iter().filter(|f| !f.is_held()).count()`
   (`herdctl/src/main.rs:1283`). An entry that was never written is not
   missing; it does not exist. `missing` is 0.
4. `code_for` (`herdctl/src/main.rs:591-596`) turns a 0 into
   `NOT_SETTLED_CODE` **only** when `files_short`, and `files_short` comes
   from that count. So the one mechanism D167 built to stop a short
   collection reading as a whole one is bypassed by the one path that
   removes the evidence of the shortfall.
5. The fact is not invisible, but it is less visible than the audit
   assumed. **F:** `outputs.skipped` reaches stderr as prose
   (`herdctl/src/main.rs:1261-1263`) and is on the hub's `job.json`. It does
   **not** reach the verdict document: `said` is assembled at
   `herdctl/src/main.rs:713-723` from the no-verdict reasons and
   `short_of_its_files`, and the skipped sentences are `eprintln!`ed in
   `keep_the_outputs` without ever entering it. So D166's machine-readable
   verdict — the file written precisely so a wrapper need not parse prose —
   says nothing about the dropped file either. The fact reaches a human
   reading stderr, and the hub's record, and nothing else.

### 1.4 The control: a refused path — F

The same bench sends a verdict whose single file is `../escape.json`:

```
herdctl exit code: 0
manifest entries on the record: 0
record skipped: ["a path this hub will not write: \"../escape.json\" climbs
                  out of the job's output directory with `..`"]
```

Same code, same shape, same silence in the exit status. **The two cases are
not equal and the difference is not in the code — it is in whether anybody
decided.** The path case has a contract sentence on the trait method it
implements: *"A path this store will not join onto its own directory is
**dropped and named on the record**, not an error"*
(`herd-hub/src/job.rs:1823-1826`), and D131's reasoning behind it. The digest
case has no sentence anywhere, in any file, and contradicts the rule four
lines below it.

**I.** Whether the path case's exit code was itself ever decided, as opposed
to inherited from the drop. The contract sentence settles the *record*; I
found nothing that settles the *exit code*, and it is the same 0.

### 1.5 What is still unknown

* **I.** Whether the consumer has ever hit this in production. Nothing on a
  record distinguishes it after the fact except a `skipped` sentence, and I
  did not read the consumer's records (D161: nothing on their instance).
* **I.** Whether any wrapper of theirs reads `skipped` from the verdict
  document. The field is there (`herdctl/src/main.rs:534`); whether it is
  read is theirs to say.
* **F, and worth saying plainly:** no test anywhere sends a verdict whose
  inline bytes disagree with their manifest. The output-integrity tests
  (`herd-hub/tests/outputs_whole.rs`, `outputs_*.rs`) are all on the streamed
  path. This bench is the first.

---

## 2. (b) A runner facing a hub that is not one

### 2.1 What was done

A hand-written WebSocket server welcomes a real `herd_runner::serve_once`
and then hands it 50, and in a second run 250, `AssignJob` frames as fast as
the socket takes them. Every job is `/bin/sh -c 'sleep 20'` with
`timeout_s = 25`, under a 256 MiB memory ceiling and the rest of M4's
defaults on a throwaway data root — **what is measured is the number
admitted, not work**. Everything is on loopback and in a temporary
directory. The host's one-minute load is read before the flood and
throughout; the bench stops and reports if it crosses 8.

### 2.2 What was measured — F

Two runs, the second a reproduction at a much higher host load (another
agent compiling beside it), which is why both are given:

| | 50 | 250 |
|---|---|---|
| assignments sent | 478 µs / 516 µs | 2.30 ms / 2.33 ms |
| job workspaces open (peak) | 50 / 50 | 250 / 250 |
| jobs on the runner's own health report | 50 / 50 | 250 / 250 |
| control probe answered in | 360 µs / 372 µs | 237 µs / 192 µs |
| host 1-min load, before → during | 2.61 → 2.40 / 7.47 → 6.95 | 2.03 → 2.02 / 6.95 → 6.56 |
| the load guard bit | no | no |
| workspaces left after cancelling all | 0 / 0 | 0 / 0 |

The probe is `FromHub::SetSwitch { switch: On }`, the one frame a runner
acknowledges (`herd-runner/src/lib.rs:2810-2816`), asking for the position it
is already in, so the probe changes nothing. Both answers are three to four
orders of magnitude inside the 2 000 ms budget.

**`herd-job` cgroups: not a measurement on this host. I.** The session's
cgroup is root-owned and delegates nothing (the bench asks by trying to
create one and removing it again), so a runner here can make none and the
count would have read 0 for the wrong reason. The workspace count stands in
for it: the same tag names a job's cgroup, its workspace and its marker
(`herd-runner/src/reclaim.rs:84`). This is the same host condition behind the
suite's ten cgroup-v2 delegation skips.

### 2.3 What this does and does not establish

**F — the audit's hypothesis holds exactly.** `serve_hub` spawns a task for
every `AssignJob` with no condition at all
(`herd-runner/src/lib.rs:2729-2757`, the spawn at `:2745`), and `take_job`
(`herd-runner/src/lib.rs:1151-1165`) refuses for `draining` and for the
owner's switch and **for nothing else** — there is no count, no capacity, no
admission. 250 of 250 were taken. Invariant 2's bound on how much work lands
on a machine is today a property of the **hub's arithmetic**, not of the
runner.

**F — the runner has no way to know the hub is wrong.** It authenticates
*itself* to the hub (the roster token, `herd-hub/src/roster.rs:234-256`) and
nothing authenticates the hub to it beyond the URL it was configured with
and `wss://`. The state-root identity on `Welcome` makes it set aside results
it is holding (`herd-runner/src/lib.rs:1847-1920`); it does not make it
refuse work.

**H, and this is the part the experiment did not test.** That the host would
be destabilized. The jobs were `sleep`, so the load did not move — by design.
With 250 jobs that each compile, the host sees 250 × the per-job cost with
no ceiling on the product, and the memory cap is **per job**, so 250 × 256 MiB
is 62 GiB of permitted ceiling on a machine with 11 GiB free. What would
settle it: the same bench with jobs that allocate, run once, deliberately,
on a machine that is not shared. **It was not run here and must not be run on
this host** — the brief's load guard and D161 both forbid it, and the number
it would produce is already derivable.

**I.** What the hub's own arithmetic actually bounds a runner to today. That
is audit B's territory (concurrency and progression), and the containment
question only becomes "how much is too much" once somebody has stated the
bound the runner should enforce.

### 2.4 What held

* **The runner stayed responsive.** F, at both figures, by three orders of
  magnitude.
* **Cancellation scaled.** F. 250 `CancelJob` frames ended 250 jobs and left
  **0** workspaces behind, with no process killed by name from outside.
* **Nothing leaked on the happy path.** F, for the cancelled path.
  §5.4 of audit 03a's leak (`outputs/` on a `SIGKILL`ed runner) is a
  different path and was not exercised here.

---

## 3. (c) Tying a verdict to the commit that was really executed

### 3.1 The gap, restated as facts

* **F.** `StepRun` (`herd-proto/src/declaration.rs:250-274`) carries the
  step, the start, the duration, the exit code and `ended`. **No frame from a
  runner carries a revision.** The commit exists on the way out only:
  `Declaration::commit` (`:225-230`), the text of the `fetch` and `up` lines
  `herdctl` filled, and the dedup key.
* **F.** So "this verdict is about commit X" rests entirely on the
  assumption that a `fetch` step exiting 0 left the tree at X. A `fetch` with
  a fallback, a stale mirror, a `switch` whose failure is swallowed by `&&`
  ordering, or a shared cache all produce a verdict attributed to a commit
  the tree was never at, and **nothing on the record can contradict it
  afterwards**.
* **F.** There is therefore **no oracle for I01**. This is not "the check is
  weak"; it is "there is nothing to check against".
* **F.** What *does* protect: a preparation step that fails means the job
  never ran (`herd-runner/src/step.rs:19-22`), and the workspace is emptied
  before every attempt (`herd-runner/src/root.rs:282-290`). Both are about a
  step that *failed*. Neither is about one that succeeded wrongly.

### 3.2 The version constraint, measured rather than assumed

The brief states the constraint as "`PROTOCOL_VERSION` 2 and
`RECORD_VERSION` 1 are frozen since D73, and the record check is a `!=` that
blocks going up **and** going back". All of that is true, and one part of it
is less binding than it looks.

* **F.** `PROTOCOL_VERSION = 2` (`herd-proto/src/lib.rs:113`),
  `RECORD_VERSION = 1` (`herd-hub/src/job.rs:52`).
* **F.** The handshake gate is `protocol_version != PROTOCOL_VERSION` →
  refuse (`herd-hub/src/server.rs:2141`). The record gate is
  `probe.record_version != RECORD_VERSION` → hard read error
  (`herd-hub/src/store.rs:1143-1149`). Both block in both directions.
* **F.** There is no record migration code anywhere. `grep -rn migrat`
  across the workspace finds cgroup *process* migration
  (`herd-runner/src/cgroup.rs:744`) and comments. I12's position —
  compatibility enforced by refusal, never exercised — is unchanged.
* **F, and this is the lever.** Herd has a written rule for the additive
  case, and it has been applied: *"a new field on an existing variant may
  leave [`PROTOCOL_VERSION`] alone when it is `#[serde(default)]` and means,
  absent, exactly what its default says"* (`docs/decisions.md:7293`). The
  record side says the same in `herd-hub/src/job.rs:44-51`.
* **F.** The wire tolerates it. No `deny_unknown_fields` appears on any
  protocol type (it appears only on `herd-runner/src/capability.rs` probe
  files and `herdctl/src/declaration.rs`, which parse files, not frames), and
  both frame enums carry `#[serde(other)] Unknown`
  (`herd-proto/src/lib.rs:1601`, `:2139`). An older peer decoding a frame
  with one extra field ignores the field.
* **H.** That no deployed component round-trips a frame or a record in a way
  that would drop the new field silently. What would settle it: a round-trip
  test across the three crates, which is cheap and belongs in the
  implementation.

**So the freeze is not what stands between Herd and an oracle for I01.** The
protocol can carry the commit home without moving either number. What stands
between them is a decision about *who produces the revision*, and that is
Alexa's.

### 3.3 The options

**Option 1 — Do nothing, and write the gap down.**
*Cost:* zero. *Protocol:* none. *Migration:* none.
*Effect:* I01 stays unfalsifiable; a wrong attribution remains undetectable
after the fact. This is the status quo, and naming it as a known limit in
`docs/coverage.md` is strictly better than leaving it implicit.

**Option 2 — The consumer's `fetch` hook asserts the commit itself.**
Their `fetch` becomes `git fetch -q origin && git switch -q --detach <hash>
&& [ "$(git rev-parse HEAD)" = "<hash>" ]`.
*Cost:* zero for Herd; one line in their declaration file.
*Protocol:* none. *Migration:* none.
*Effect:* a divergence becomes a **setup failure** (exit 125, the job never
runs, `herd-runner/src/step.rs:19-22`) instead of a wrong verdict. That is a
real and immediate reduction in blast radius.
*Limit:* the guarantee moves to the consumer. Herd's record still says
nothing, so Herd still cannot contradict a verdict, and a consumer who
changes that line loses the protection without anyone noticing. **It is
containment, not a fix.**

**Option 3 — the runner reports the revision, read from a file the `fetch`
hook writes. ← recommended as the fix.**
A new placeholder, filled by the runner like `{job}`, `{in}` and `{out}` are
(`herd-proto/src/declaration.rs:24`), naming a path the `fetch` hook writes
the resolved revision into. After `fetch`, the runner reads it, trims it,
bounds its length, and puts it on `StepRun` as
`ran_commit: Option<String>` with
`#[serde(default, skip_serializing_if = "Option::is_none")]`. The hub
compares it with `Declaration::commit` and **records a divergence on
`job.json`**.
*Protocol:* additive under the rule at `docs/decisions.md:7293`.
`PROTOCOL_VERSION` stays 2.
*Record:* additive under `herd-hub/src/job.rs:44-51`. `RECORD_VERSION` stays
1. **No migration, and I12 stays untouched.**
*Cost:* one field in `herd-proto`, one read in `herd-runner`, one comparison
and one recorded fact in `herd-hub`, one line in the consumer's declaration,
and three tests (agreement, divergence, silence).
*Invariant 1:* Herd never runs `git` and learns no VCS. It compares two
strings, one of which the consumer's own hook produced. That is the same
shape as `labels_from`, where the consumer's command answers and Herd reads
the answer strictly (`herd-proto/src/declaration.rs:440-500`).
*The absence must mean "did not say".* `None` is "this runner reported no
revision", never "it matched" — the exact reasoning D-for-`suspended` gives
at `docs/decisions.md:7284-7291`, where a plain `Vec` with a default would
have decoded an old peer's silence as a clean bill of health.

**Option 4 — a fifth executor step, `verify`.**
*Rejected.* `herd-runner/src/step.rs:12-18` says it: *"the day a step gains a
condition, Herd has grown the workflow language invariant 1 refuses to be"*.
Four steps, fixed order, no conditions.

**Option 5 — the hub re-resolves the commit itself.**
*Rejected.* The hub would need the repository, the network and VCS
knowledge. Invariant 1 (stay specialized) and invariant 3 (Herd itself
builds and requires no network).

**Option 6 — the runner runs `git rev-parse` after `fetch`.**
*Rejected.* Herd would learn one VCS and stop being project-agnostic
(E-11: Herd's code names no consumer command). Option 3 gets the same fact
without Herd knowing what produced it.

### 3.4 What the hub should do with a divergence

**Record it, do not refuse it.** Three reasons, and the third settles it.

1. A refusal turns a reporting gap into a new failure mode on the day the
   field ships, on a fleet where some runners report and some do not.
2. `None` means "did not say", and a hub that refused on `None` would refuse
   every older runner; a hub that refused only on a *mismatch* is already
   the recording hub plus one `if`, which can be added later on evidence.
3. The thing missing today is not enforcement, it is **a fact on the
   record**. Once `job.json` carries "declared X, ran Y", I01 is falsifiable
   for the first time, and whether that fact should also stop a job is a
   second decision that can be taken with data instead of before it.

`herdctl`'s exit code stays untouched in the first increment: the verdict is
the job's and a side channel never becomes one (D20). A later
`--require-commit` that exits non-zero on a divergence, or on silence, is
the natural follow-up and is D167's shape, not D20's.

### 3.5 Migration path

**There is none, and that is the point of choosing Option 3.** Spelled out
as a matrix, because "additive" is not "an older peer survives it" and the
project has said so before (`docs/decisions.md:7336`):

| | old hub | new hub |
|---|---|---|
| **old runner** | today | field absent → `None` → "did not say"; recorded as such, nothing refused |
| **new runner** | extra field on `JobFinished`/`StepRun`, ignored by serde; old hub behaves as today | the case the change is for |

| | old `herdctl` | new `herdctl` |
|---|---|---|
| **old record** | today | field absent → "did not say" |
| **new record** | extra field in `job.json`, ignored; `RECORD_VERSION` still 1, so the record still reads | the case the change is for |

Neither number moves, so **a rollback works too** — which is the half of
I12 that a bump would break and that nothing in this repository has ever
exercised.

---

## 4. Non-regression and containment criteria

### 4.1 For (a) — how we will know it is repaired

A fix is the two-line inversion of `herd-hub/src/store.rs:1711-1730`: push
the manifest entry **first**, then refuse the bytes. The entry is then
present and not held, which is what every other absence path already
produces.

It is repaired when all five hold:

1. **F-able:** a verdict with one lying inline file of two leaves **two**
   entries on `job.json`, one of them not held, and the `skipped` sentence
   beside them. (Today: one entry.)
2. `herdctl run --outputs-to` over that verdict exits **2**
   (`NOT_SETTLED_CODE`), not 0, because `missing` is 1. (Today: 0.)
3. The honest file still arrives, byte for byte. A fix that loses the good
   file to protect against the bad one is worse than the defect.
4. The **refused-path** case is decided *explicitly* and separately — either
   it keeps its entry too, or `herd-hub/src/job.rs:1823-1826` gains the
   sentence saying why it does not. Not left as "the same code, for a
   different reason".
5. `herdctl/tests/inline_digest_mismatch.rs` stops being a bench and becomes
   an expectation, with the figures asserted.

**What bounds the damage until then — and it is thinner than it looks.**
* **F:** the record is not wrong, it is incomplete. `job.json` carries the
  `skipped` sentence naming the path and both digests, so an incident is
  diagnosable **after the fact** by reading the hub's record.
* **F, and the uncomfortable part:** the only thing a requester's own
  process sees is a prose line on stderr. The exit code does not carry it
  (§1.2) and neither does the verdict document (§1.3.5). A wrapper that
  wanted to protect itself today would have to grep stderr for
  `this job's output:`, which is exactly the "parse a sentence" that D166
  exists to make unnecessary. **Putting the skipped sentences into
  `Verdict::said` is a one-line change with no protocol or record cost**,
  and it would give a consumer something to act on before the real fix is
  taken. It is not proposed as the fix; it is proposed as the stopgap.
* **F, available today with no change to Herd at all:** a consumer whose
  wrapper compares the file count it expected with the file count under
  `--outputs-to` catches this, because the bytes really are absent. That is
  the workaround D167 was supposed to retire, and it is back for this one
  path.
* **H:** frequency is low — reaching it needs a manifest entry disagreeing
  with its own bytes, which a same-build runner cannot produce. The
  consequence is high: a green build with a dropped output file.

### 4.2 For (b) — how we will know it is bounded

There is nothing to repair until somebody states the bound, so these are
criteria for *a* fix rather than for the fix:

1. **A runner refuses work it has no room for, on its own.** The refusal is
   in `take_job` (`herd-runner/src/lib.rs:1151-1165`), beside `draining` and
   the switch, and under the same lock — because a count checked outside
   that lock is a count two assignments can cross.
2. **A refusal is a named abandoned attempt**, not a dropped frame: the hub
   must learn it and give the job another go, which is E-21's existing path
   and is what `draining` already does.
3. **`measure_*_assignments_from_a_hub_that_is_not_one` shows the bound**:
   250 sent, N taken, 250−N refused by name, where N is whatever was
   decided. The bench becomes an expectation at that point.
4. **The responsiveness figures do not regress**: a probe answered inside
   2 000 ms at the flood figure. Today it is 237 µs and there is room.
5. **Nothing is left behind.** 0 workspaces after the refusals, as there are
   0 after the cancellations today.

**What bounds the damage until then.**
* **F:** the runner is reached only by the hub it dialled. Invariant 3 means
  nothing inbound; the flood in §2 required a hub at the configured URL.
  The realistic threat is therefore a **buggy or misconfigured hub**, not an
  arbitrary peer — which is why this is a containment gap and not a
  vulnerability.
* **F:** every job is still individually capped — memory, time, pids, log,
  disk. What is uncapped is the *product*, which is the one figure no cap
  names.
* **F:** the owner's switch and `draining` still work at any count; a runner
  flooded with 250 jobs still obeys `switch off`, and that is invariant 2's
  five-second promise. §2.2's probe is the evidence.
* **H:** a per-runner concurrency ceiling derived from `HostSize` — the
  figure the runner already reports and the hub already uses — would need no
  new configuration and no protocol field. Not proposed here; it belongs
  with audit B's arithmetic.

### 4.3 For (c) — how we will know a verdict names the commit it ran

1. **F-able, and the first time it is:** a job whose `fetch` leaves the tree
   at a different revision produces a `job.json` that says both — declared X,
   ran Y — and a test asserts it.
2. A runner that reports nothing produces `None` on the record, and the
   record says *"this runner did not report a revision"* in those words,
   never a blank that reads as agreement.
3. An older runner and a newer hub, and the reverse, both work with no
   migration: `PROTOCOL_VERSION` is still 2 and `RECORD_VERSION` is still 1
   after the change, asserted by the same `grep` that D168's tag check uses
   (`docs/decisions.md:7775`).
4. `herdctl`'s exit code is **unchanged** in the first increment, and the
   decision to change it is taken separately on recorded divergences.

**What bounds the damage until then:** Option 2. The consumer's `fetch` can
assert its own commit today, in one line, with no change to Herd, turning a
wrong verdict into a setup failure. That should be asked for whatever is
decided about Option 3, because it protects the next build and Option 3
protects the one after the decision.

---

## 5. What I could not establish

* **I.** Whether (a) has ever happened in production. Nothing was read on
  the consumer's instance (D161).
* **I.** The `herd-job` cgroup count in §2: this host delegates no cgroup v2
  subtree, so the figure is not a measurement. Workspaces and the runner's
  own health report stand in for it.
* **I.** What a flood of *real* jobs does to this host. Deliberately not
  measured: the host is shared with a production system.
* **I.** What bound the hub's arithmetic currently puts on one runner. Audit
  B's.
* **I.** Whether the refused-path case's exit code of 0 was ever decided, as
  opposed to inherited.
* **H.** That no component drops an added `#[serde(default)]` field on a
  round trip. Cheap to settle with a test, and that test belongs in the
  implementation of Option 3, not here.
