# Plan

Reworked 2026-10-12 from the chatOnS requirement sheet (issue #1, expectations
E-01..E-55 — 44 must, 9 should, 2 could; mapping in `docs/coverage.md`). M0 is
done; everything after it is replaced. A milestone is one or more branches,
landed in a batch with one `--no-ff` merge commit (D189), each branch ending
with `nix develop --command ./scripts/test.sh` green and `docs/status.md`
replaced. Done means a passing automated test; where acceptance needs real
hardware or a chatOnS deliverable, the automated stand-in is named and the
rest is a manual check or a gated item closed in M18. Vocabulary per CLAUDE.md
(the sheet's "client" is our runner); chatOnS reaches Herd only as declared
commands, labels and figures — strings and numbers, never meanings.

**Corrected against the tree on 2026-10-29** (ORCH-152). Four statements
in this document changed what gets built and were wrong: M14's hub-side
bare repository (D207), M12's three-term placement order, M15d's empty
justification, and M11's pack written under `{out}` (ORCH-153). Each is
now marked where a reader meets it, with the wrong claim quoted rather
than quietly deleted, so nobody carries it forward from a stale copy.
Where this document and the code disagree about what exists, the code is
what is true; `docs/coverage.md` holds each expectation's state and the
counted must figure. This correction is **not** the roadmap to v1.0.0,
which is still owed (ORCH-155).

## Milestones

### M1 — End-to-end skeleton (`m1-end-to-end`)
Nix flake dev shell; workspace, four crates; herd-proto v1 (Hello/Welcome with
protocol version and token field; submit, assign, log chunk, finished); hub
dispatches to its single runner; `herdctl run` streams the log, exits with the
job's code.
**Done when** an in-process hub+runner+requester test runs a trivial command,
asserting streamed output within 2 s and exit-code propagation (0 and
non-zero); proto round-trips pass; a version mismatch is refused naming both
versions. Substrate only; satisfies no E alone.

### M2 — Durable job store (`m2-job-store`)
Hub state on disk from birth (Q1): one directory per job — `job.json`
by atomic rename, `events.jsonl` appended, log, kept files. Record fields;
state machine with timestamped transitions; queue rebuilt at startup; a
runner holds a finished result until acknowledged, redelivering after a hub
restart. Detached submission, `herdctl wait <id>`, log readable as a file
afterwards. The minimum reconnect (D17): a runner that loses its control
connection redials with a simple bounded retry and keeps its running job
alive across the gap, so the held-result machinery above is reachable;
an attempt interrupted by the drop is recorded as interrupted by name,
never left silently `running`.
**Done when** a test kills the hub mid-job against the real binaries and
the job finishes on its runner, redials, redelivers its held result, result
recorded, queue intact (E-44); a detached wait returns the exit code and
the log file equals the stream (E-03); the record is plain files readable
while the job runs (E-15 core, E-53). Metric fields exist, filled in M4.

### M3 — The link survives reality (`m3-link-robustness`)
M2's minimum reconnect (D17) gets a backoff policy; health report every ~15 s;
~60 s of silence = genuinely lost: running jobs requeued once at priority,
the record naming both attempts, a returning runner finishing nothing of
the old attempt. Cancel propagates hub → runner. Drain ≤ 5 s (invariant 2).
**Done when** killing the runner mid-job past the lost threshold requeues
within 90 s with a two-attempt record (E-21); cancel ends the job within
10 s while a sibling job continues (E-06; kill hardened in M4); a draining
busy runner exits ≤ 5 s; heartbeat ages land in hub state (E-40 partial,
surfaced in M13).

### M4 — Runner confinement (`m4-runner-confinement`)
One configured data root: a bad root refused, nothing written outside it
(E-45). Unprivileged, never sudo; root only via a configured fixed-list helper
(E-46). Per-job workspace, process-group/cgroup kill, nice CPU+IO, no inbound
socket, disk budget (E-34). Kernel memory ceiling: cgroup v2 MemoryMax, logged
fallback, WSL per Q6 (E-30). Timeout, log cap. Collects load, memory peak,
bytes written (completes E-15's fields).
**Done when** a job writing outside the root fails naming the path; an
allocation past the ceiling kills the job and only the job (automated in a
small cgroup; the WSL OOM replay is a manual M16 check); metrics appear in the
record; the runner binds no listening socket.

### M5 — Capability model (`m5-capability-model`)
One algebra in herd-proto (Q3): capability = name + kind (exclusive host-wide
/ counted(n) / shared) + exclusion list + optional external busy probe (a
lock-file path or a command whose exit code means busy — strings, never
plugins). A runner offers probed ∪ declared ∪ denied, each with origin (E-24).
`metal-<m>` is counted(1) with a lease probe (E-27's model); `reference` rides
on the kinds (E-28); external busy suspends availability, fails nothing
(E-26).
**Done when** unit tests cover kind semantics, exclusion closure, origin
reporting (`kvm: present, denied`); a held lock file keeps a queued job queued
with the reason recorded, releasing it afterwards.

### M6 — Queue and priorities (`m6-queue-priorities`)
One durable queue for every job (E-20). Four levels set at submission, FIFO
within, aging so the lowest never starves, operator raise (E-18; merger-first
is E-43). Dedup on the command and the labels — the commit joins the key when a
submission names one, in M10 (E-01): the second submission joins the first
while that job is open, both callers one result id, higher priority wins;
work that is over is rerun (E-05, D70).
**Done when** simulated-time tests show a lowest-priority job behind a steady
higher stream running within the aging bound, a merger submitted last claiming
the next free slot, and two identical submissions producing one execution with
two completed waits.

### M7 — Matching, slots, no fallback (`m7-matching-slots`)
Claiming matches labels against the algebra, enforcing kinds under contention
(E-25). Counted build slots; per-slot parallelism derived from memory and
threads, recorded (E-22). Stickiness: a job waits for an eligible runner while
one is available (connected, switch on), moves only when none is, under a
visible safety limit (E-19). One explanatory line per claim; impossible label
sets fail fast (E-36). Matching reads a job's requirements off its own
record — stated at submission or produced by a `labels_from` reading
(D74) — and never anywhere else.
**Done when** two counted jobs serialize and an exclusive one never overlaps
them; a modelled PC never runs two builds, its derived `-j` recorded; with the
eligible runner busy the job stays queued, disconnected it moves within 10 s;
every placement in a simulated day greps to one line; an unsatisfiable job
fails with the reason.

### M8 — Admission and explained waits (`m8-admission`)
Memory admission: declared need vs free minus a per-runner floor minus what
running jobs hold (E-29). tmpfs reservation before start, released at end;
short → wait, never fail (E-31). Host-wide sums; load recorded, never a gate
(E-32). A queued stream reports position, what is ahead and why, at least
every 60 s; refusals carry figures (E-04). Admission reads the same
requirements held on the record (D74), whoever put them there.
**Done when** admission unit tests pass with figures in refusals (`1.2 GiB
free in tmpfs, 2 GiB needed`); a reservation-starved job waits then starts
when room frees; a 10-minute simulated wait leaves periodic reasons.

### M9 — Owner control (`m9-owner-control`)
Owner switch per runner — off-with-reason, drain, on — honoured in 10 s, set
from the runner's host or via herdctl, state visible (E-33). Dry run (E-07).
Deadline (E-09; needed by E-41 — and, since D88, the **only** bound on a job
that no connected runner could ever fit, which otherwise waits for ever).
Dependency: at most one named parent, start after its success, else cancelled
with the reason — one edge, no conditions
(E-10; needed by E-12's flow). Metal drain-first: a `metal-<m>` job drains its
host's build slot before starting, bounded 15 min (completes E-27). A
`metal-*` capability's lease file is read as a busy probe (D48, D51): a job
whose machine is leased waits rather than runs and fails. A command may
declare one requeue exit code (D76) — their 75 — for the race between that
probe and the run; it causes a bounded requeue with the capability
re-probed, separate from E-21's two attempts for a lost runner, and ends
the job naming the code and the runners that answered it once the budget
is spent.
**Done when** `off` stops claims within 10 s and stops serving content too
(D11), reason visible; dry run lists
eligible runners and the queue ahead, runs nothing; a past-deadline job shows
`cancelled (deadline)`; a dependent starts only after its parent; a simulated
metal trial never overlaps a build on its host; a leased `metal-*` job waits
instead of failing, and a declared requeue code is retried within its
budget then ends named.

### M10 — Command declaration contract (`m10-command-contract`)
The consumer declares commands in one file in its own tree (`commands.toml`),
field set now named: `line`, `labels` and/or `labels_from`, `memory_mib`,
`tmpfs_mib`, `timeout_s`, `priority`, `needs` (artefacts of an earlier job of
the same commit), `leaves` (artefacts to keep, with a key), `coalesce`,
`dedup_key`, `schedule`, plus the `[executor]` hooks (`up`, `fetch`, `run`,
`down`) and the `{job} {commit} {args} {in} {out}` placeholders Herd must
provide (`{in}` and `{out}` two directories under the runner's data root). A
submission names a declared command and a commit, both read by `herdctl` in
the requester's own checkout (D106): the declaration file is parsed there, a
ref resolves to a recorded hash, never a machine (E-01), and the resolved
declaration travels with the submission, so the hub and a runner never see
TOML and never hold a repository. `herdctl` refuses a dirty or an unpushed
tree naming the files (E-08). The runner
materializes the commit, runs the command inside `nix develop` with the
consumer's environment, confined per M4, through its four named executor
steps in fixed order — the record gains one timestamped line per step: name,
start, duration, exit code (D77). A command declaring `labels_from` in place
of static labels is resolved by an ordinary job on a runner, confined and
recorded like any other, against the command's declared baseline labels; its
answer is read strictly, refuses the job by name on an unknown, missing or
duplicated key, a non-numeric figure, a non-zero exit or a timeout, never
recurses into a further `labels_from`, and is cached by (commit, command,
args) in the job store (D74). Output framing: first line
`ran on: <runner> (<why>)`, then the tool's verdict and exit code unchanged,
0/1/75 passed through, format versioned (Q7) (E-02). Herd's code and docs name
no consumer command (E-11).
**And the job's `{out}` comes home** (D131): what a job leaves there is
collected before the workspace is reclaimed — on every ending that produced
one, a failure above all — written into the job's `files/` directory on the
hub by D8's discipline, and handed to a requester on the frame that already
carries the verdict, so `herdctl wait <id> --outputs-to <dir>` works for a
requester that was never connected while the job ran. Keyed by job id with
relative paths kept, never by `leaves`. Capped at 32 MiB, which is what one
frame carries and decodes inside the 15 s cadence; over it no file comes back
and the refusal names both figures. Nothing of it touches E-02's stream.
**Done when** a fixture consumer repo declares two commands and adding a third
changes nothing on Herd's side; e2e asserts the `ran on:` line, the verbatim
verdict, exit codes 0/1/75; a submission naming a ref resolves it to the hash
the fixture's git holds; a dirty submission and an unpushed one are each
refused naming the files;
a `labels_from` reading lands as its own recorded job, caches by (commit,
command, args), and a malformed answer refuses the real job by name; the
record shows a timestamped line per executor step.
The real chatOnS declaration closes in M18.

### M11 — Content registry, direct transfer (`m11-content-registry`)
**Two flows wearing one sentence, and this milestone is both** (D148). The
first consumer has made Herd their only path, with no fallback, and jobs are
about to run on a second machine. Their words: *"once a job can run on
another machine, logs, screenshots and scenario files no longer sit on the
server's disk; results.json alone is not enough to diagnose a red test — we
need chunked return, hub-side streaming and a retention policy, sized for a
few hundred MB per job."* That is two needs, separated by **who reads the
bytes next** and not by how many bytes there are, and they get opposite
answers.

**A pack is read by a job, and never touches the hub.** A job's declared
artefacts move **runner to runner**: the hub is the registry and the
authority, never the byte path (D10). It holds manifests — a pack is a list
of content hashes keyed by (commit, key) — and which runners hold which
blobs; it issues a short-lived, scoped fetch capability for a (blob, peer)
pair. Each runner keeps a per-file content-addressed store under its data
root and advertises a fetch endpoint (Q2); a runner needing a pack asks the
hub who holds it and fetches directly, only the hashes it lacks. Three
paths, in order: **same host** — hardlink or reflink inside the local store,
zero bytes on any network; **peer reachable** — direct fetch from a holder;
**outbound only** (a runner with no mesh) — it pushes to and pulls from a
keeper, never serves. A fetch endpoint binds to the one configured mesh
interface and is refused at startup if it would bind a default or public one
(D11). A pack is complete or absent: the manifest is registered only once
every blob is held and fsynced (E-17, E-13, E-12). The real shape, now
known: `<id>/manifest.tsv`, `<id>/pack.meta`, `<id>/objects/<sha256>`, `<id>`
the sha256 of the manifest. Herd keeps a build job's pack by (commit, id)
and hands it to the same commit's test-pack jobs as `{in}/pack`; measured by
them, ~400 MiB in ~560 files, verified in under 2 s.

**A pack must not be written under `{out}`, and this paragraph used to say
it was** (ORCH-153). `herd-runner/src/outputs.rs` collects **everything**
under `{out}` that is a regular file, with four exclusions and none of them
a pack, so a build job writing `{out}/packs/<id>` would stream ~400 MiB
runner → hub → requester as record evidence, against the hub's 200 GB
output budget: the exact byte path D10 removed and D152 then has to sweep.
**The shape is settled: D223** (2026-10-29). The two candidates were not
equal. A fifth exclusion in `outputs.rs` can be undone by a later edit to
that list; a pack that is **gone from `{out}` before the record walk runs**
cannot, and the second is what D10's byte-path argument implies. So that is
the rule, and the pack's path here is **never `{out}/packs/<id>`**.

E-13's content-addressed storage is then simply storing objects by their
sha256 name, which is what their pack already is.

**A record is read by a requester, and travels through the hub.** A job's
log, its `results.json` and its per-scenario screenshots are evidence about
one job, wanted by whoever holds its id — a person or an agent diagnosing a
red test, who never saw the declaration and may be on a machine with no mesh
at all. D131 and D137 put that category on the hub and D147 ran it on the
real deployment; what this milestone changes is only the **carriage**, which
was one base64 frame and therefore capped at 32 MiB. A requester declares
nothing new: the runner measures the collection when the job ends, rides the
verdict frame at or under 32 MiB exactly as today, and **streams** over it,
file by file, each announced with its path, length and sha256 before a byte
moves, in bounded chunks, into the job's `files/` directory under D8's
discipline. The verdict goes first and the files follow, so a side channel
never delays a verdict (D20) and a stop still takes ≤ 5 s (invariant 2). A
transfer that dies resumes: the runner holds the collection in its own data
root until the hub acknowledges all of it, and the hub says on redial which
files it holds complete and how much of a partial one. Hub → requester needs
no resumption state — the bytes are on the hub's disk, so `herdctl wait <id>
--outputs-to <dir>` is simply run again and skips what it already has at the
right hash.

**This is where the record half's protocol break is spent** (D148).
Streaming files needs new variants on both enums, which D124 and D128 make
breaking whatever serde tolerates, so `PROTOCOL_VERSION` moves to **2**,
approved by Alexa on 2026-10-16. **It is not spent once for both halves,
and the sentence that said so was never true (D205):** version 2 went on
the record half alone, the pack frames below are numbered on neither
protocol enum today, and they are part of why `PROTOCOL_VERSION` moves to
**3** before v1.0.0 (D199). Anyone sizing the pack work on the belief that
the protocol cost was already paid is sizing it short. The registry's own
frames above ride the same break, and so does a catch-all arm so an unknown
frame no longer means "does not decode at all" (D128's recorded defect).
The arm reads three ways,
one per direction, and the difference is the point: a **runner** logs and
ignores what it cannot read, because a frame it does not know is work it is
not needed for; a **requester** still refuses the run, because one that
cannot read what the hub is telling it must not report a verdict (D124's
`bail!`, which stays); and the **hub** refuses by name rather than ignoring,
because a request it cannot read is work that will never be done and the
peer has to be told. Nothing from M15 is guessed at here.

**Done when** a two-runner e2e produces a fixture pack on runner A and
consumes it on runner B with the hub seeing no payload byte (asserted by
counting bytes on the hub's sockets); a same-host fetch moves zero network
bytes and no second copy on disk; a second pack sharing files with the first
transfers only the new blobs; killing a transfer mid-flight leaves the pack
unregistered and no partial blob readable; an outbound-only runner completes
both directions without listening; a configuration that would bind a default
or public interface is refused at startup; a fetch without a valid capability
is refused. And for the record's half: a job leaving a few hundred MB under
`{out}` returns all of it through the hub and `herdctl wait --outputs-to`
writes it byte for byte; a requester cannot tell from its own output which
carriage was used; a link killed mid-collection resumes and the files are
whole, with no file ever readable half-written; a hash that does not match is
named on the record rather than kept; **a collection past the output budget
is gone while its record and its manifest read fine, and a requester asking
for it is told so with the figures and the date** (D152); and — the
assertion D138's objection makes the important one — **a runner streaming a
few hundred MB never looks lost**, its health reports staying inside the
15 s cadence throughout, which is the measurement M11 owes and currently
only has an argument for. chatOnS's `0 crates compiled` acceptance closes in
M18.
Certain to overflow one session. The split is **at the record/pack line**,
record first: it is what the consumer is blocked on today, and it is where
the record half's protocol break lands. **The intention was to design and
number both halves' frames together in one move to version 2; that is not
what happened.** The record half shipped at version 2 with no pack frame
beside it, so the pack half pays its own break — `PROTOCOL_VERSION` 2 → 3,
with `RECORD_VERSION` and `LAYOUT_VERSION` 1 → 2 in the same window (D199).
That is a second break paid by the deployment, which this paragraph set out
to avoid; it is affordable only because the consumer resets their volume at
v1.0.0 (D199) and for no other reason.

### M12 — Custody, budgets, GC, retention, locality (`m12-custody-gc`)
**This milestone shrinks twice.** First by the consumer's own words — "you
move packs; our executor owns warm caches": their `up` step pulls its nearest
cache itself and D77 already timestamps that step, so Herd need not carry a
cache, know its contents, or collect it. Gone: the prepare step tied to cache
keys, its duration bookkeeping, a runner's health report of cache keys held,
and cache diffs registered specially as packs — all of it the cache-engine
apparatus D9 sketched, replaced by nothing, because the generic mechanisms
already cover the ground: `needs`/`leaves` (M10) for artefacts, D77's step
timestamps for preparation time, M11's registry for which runner holds which
blob. Second by D148: **custody is about packs and not about records.** A
job's record leaves the runner when the job ends, so a verdict and its
evidence are never unreadable because a machine is asleep, and there is
nothing for a keeper to pin on that side.

Custody of packs (D10): a key family names one or more **keepers** — runners
that pin what the retention policy keeps, so a pack outlives the machine that
built it and survives its owner switching that machine off. The server's
runner is the default keeper; a pack with no reachable holder is a named,
visible state, not a silent failure. Per-runner budgets shown as use against
budget, gc on each runner never removing what a queued or running job
references nor what its keeper role pins (N = 5 most recent per key family,
configurable) (E-16). A runner's own undelivered collections are swept the
same way, and only once the hub has acknowledged them or their record is
gone.

**Retention, answering the question the consumer will actually ask** — *how
long do I have to fetch yesterday's failing screenshots?* Two numbers,
because there are two kinds of thing (D148), and **since 2026-10-29 the
space the numbers guard is what decides, not the calendar**
(`docs/disk-policy.md` §0). The record proper — `job.json`,
`events.jsonl`, the log, the inline files — carries its priority class's
span, 14/90 days, configurable (closes E-15), **and that half is still
here**: what changed is that a span is now the **maximum age** a record may
reach rather than when it is deleted. The trigger is free space on the
filesystem the state root sits on, with the span as the backstop that acts
when nothing is under pressure; the output floor doubles as the guard that
keeps space pressure off a fresh record. **Deduplication ships on by
default** — every file in a job's `files/` a hard link to a
content-addressed blob — because that is where the room is: four fifths of
what the first consumer's records name are second and later copies, so
deduplication divides the arrival rate rather than recovering anything
already written. The other half, a job's **large files** against their own
budget on the hub's volume, **shipped in M11** rather than waiting: a
sweeper with no budget is a disk-full outage with a delay fuse, so
streaming could not be released without it (D152). `--output-budget` is
200 GB with a seven-day `--output-floor`, both printed at startup; the
200 GB is Alexa's, chosen on 2026-10-16 without a measurement and written
down as such (D148), and the budget governs the **collected files only** —
a record's own bytes are the filesystem's business, which is why the free
space guard and not the budget is what can ever remove one. What M12 still
owes on that side is the use-against-budget figure in M13's status
document, which needs M13. The eviction order is not oldest-first: outside
the floor before inside it, then a job that passed before one that did not,
then a collection already fetched before one nobody has opened, with age
only as the tie-break (D152). Evicting inside the floor is a named line and
an event, never a silent trim. And **nothing is kept without a bound**:
what can no longer serve is removed after a stated delay, saying so, or
moved to a configurable archive path on another disk — never destroyed
immediately and never destroyed silently.
And the **manifest outlives the bytes**: each file's path, length and sha256
stay on the record for as long as the record does, so a requester asking for
something already gone is told how many files there were, how big, and when
and why they went — never an empty directory and never a short one.

Placement preference among available eligible runners. **What ships today is
not the three-term order this line used to state.** `herd-hub/src/matcher.rs`
takes the maximum of one operator-declared integer, `preference`, and breaks
ties by runner name so the same fleet makes the same choice twice; there is
**no free-memory term in it at all**, and no `reference` ordering either.
What M12 adds is cache locality (E-23) as a **further term on top of that
integer** and not a second mechanism: keys already held preferred among
runners of equal declared preference, reference last. **Free memory has no
home anywhere** — not in the matcher, and not in M12's own *done when*
below, which asks only that placement prefer the key holder and pick
reference last. So the deleted three-term order disagreed with this very
milestone as well as with the code. Whether free memory ever becomes an
ordering term is undecided and must not be assumed from this document;
memory is an **admission** (M8), which is a different question from an
ordering. E-19 still governs waiting.
Scheduling and running the consumer's `produce` command on a `build` runner,
and moving its pack (M11), stay here.
**Done when** a pack whose only non-keeper holder goes offline is still
fetchable from its keeper; a pack with no holder at all reports that state by
name; gc on a runner with a budget 10 GiB below use comes in under it keeping
every pinned and referenced item; a step result past retention is gone while
a 60-day merger result reads fine; a `produce` job is
scheduled and run on a `build` runner and its pack moves like any other; and
placement prefers the runner already holding the key, picking reference last.

### M13 — Status and events (`m13-status-events`)
**Partly landed 2026-10-29 in batch 8, and E-37's fifth item built on
`detached-end-event`, 2026-10-30.** E-35 is merged and E-37 is partly
built — four of its listed transitions landed in batch 8, the fifth is on
that branch, and the last of the five needs M14 and no event of its own.
The milestone as a whole is not done.

One status document, JSON with a format version (Q7), carrying the queue,
runners, running jobs and leases with the fields E-35 lists, refreshed on
change, plus one text screen (E-35); M3's health ages surfaced (E-40). **As
built (D215):** `status.json` in the state root beside the records, with its
own `STATUS_VERSION`, published on a ring from `State::with_peers` and never
on a clock, and `herd-hub status` as the one text screen over it. Reading it
takes no lock, and a serving hub rewrites it about five times a second, so
it is not state a test may checksum. Events pushed to a configured URL or
command — exactly E-37's list, never per step job by default (E-37). **As
built (D216):** four of the five listed things have a transition in today's
hub and are emitted to one sink, a URL or a command string, with no filter,
template or per-event switch. **The fifth is built on `detached-end-event`,
2026-10-30:** the submitter's asking travels as `announce_end` on
`ToHub::SubmitJob`, through `NewJob`, onto `job.json`, and is read back at
every door a record reaches a terminal state through; `herdctl submit
--announce-end` asks for it and `herdctl run` is offered no such flag, which
is what makes *"detached"* true. Neither `PROTOCOL_VERSION` nor
`RECORD_VERSION` moved: both fields are absent when nobody asked, asserted
on the written bytes. **What is left of E-37 is the nightly's verdict, and
it is this same event rather than a new one**: a scheduled pass is
`herdctl if-moved` submitting on the consumer's own timer, so what it waits
on is M14's nightly existing and the asking being passed through on that
submission — one line in `herdctl`, not hub work.
**Done when** the status document and its screen carry E-35's fields and
drive a fixture renderer from the document and nothing else, and a simulated
day of step jobs emits exactly the expected events and zero per-step ones.
**A separate JSON Schema was the old wording of this and does not exist**:
the typed document is the schema, and a document of another
`STATUS_VERSION` is refused by name with both figures. What is still owed
here is the use-against-budget figure M12 points at this milestone for,
which stays `herd-hub report`'s because it is a walk of the volume. E-37's
own remainder is **not** owed here: the nightly's verdict is M14's, and no
further M13 work closes it.

### M14 — Scheduled jobs and the hook path (`m14-scheduled-jobs`)
**Corrected 2026-10-29 by D207, and partly landed the same day.** This
milestone said, from 2026-10-15 until that correction, that the **hub**
kept a bare repository and read a ref to decide when to submit. That was
wrong on the record and not merely stale: D106 settled that the hub never
holds a repository and never runs git, and a hub that clones a repository
to decide when to build is the generic-CI surface invariant 1 refuses —
it would also put a credential for someone else's repository inside the
hub. **The predicate is the requester's.**

As built (D207, D217): `herdctl if-moved` submits a declared command when
a named ref has moved since the last run it recorded, under a lock so two
overlapping timers cannot both submit, and refuses rather than guess when
its state file is missing, unreadable or from another version. **The timer
is the consumer's own**, as E-42's hook is; `docs/scheduling.md` says what
we ship and what they write. The hub accepts the submission, matches it
against declared capabilities and priority (invariant 6), and nothing
about a scheduler changes that. Exclusivity from label kinds, deadline
from M9. Their nightly is concrete: 03:00 local, daily, only if main
moved, 5 h cap, `nightly` exclusive against everything on that runner, a
05:00 start deadline past which the job is cancelled and reported. **One
deviation, and no hub work closes it (D217):** a pass that submitted
nothing is `herdctl`'s own stderr line and never an event, because the hub
never heard of the job; the skip E-41 *can* have as an event is the other
one, a pass submitted with `--deadline` that nothing started by 05:00,
which the hub cancels as `cancelled (deadline)`, exit 125 (E-09, D40).
**E-37's last item lands here and not in M13**: *the nightly's verdict* is
the `job_ended` event built on `detached-end-event` 2026-10-30, so what it
needs is `if-moved` asking for it on the submission it makes — the hub side
exists and announces every ending, the 05:00 cancellation included. The
push-hook path is a documented detached `herdctl` submission; the hook
itself is the consumer's (E-42), submitting `produce` with a `dedup_key`
of `produce:<commit>` and a `coalesce` key of `produce:main`: a queued
`produce` of an older main is superseded, never a running one (D75).
**Done when** `herdctl if-moved` submits exactly once for a ref that moved
and nothing for one that did not, saying which on its own stderr, and two
concurrent runs racing one state file do not both submit (**landed**: E-41,
`herdctl/tests/if_moved.rs`); the job carries its deadline and its exclusive
label keeps everything else off that runner until it ends, a 03:30 job
waiting or going elsewhere; a job submitted with a 05:00 deadline that
nothing started by then ends `cancelled (deadline)` with the reason naming
what it was waiting for; a scripted hook of the consumer's lands a
lowest-priority job in e2e; a second hook push before the first `produce`
starts supersedes the queued one, exiting 125 and naming the superseding
job, while a `produce` already running is never discarded. **No simulated
hub clock and no bare repository appear in any of these**: there is no hub
clock to simulate and the hub holds no repository to read (D106, D207).
What is left of this milestone is E-42's coalesce path — `coalesce` is
parsed by `herdctl`'s declaration today and nothing in the hub or the
protocol supersedes a queued job on it.

### M15 — Auth and versions (`m15-auth-versions`)
Per-runner token minted by one operator action (`herdctl runner approve`),
stored hashed; an unapproved runner claims nothing; removal revokes (E-48,
Q8). Runner build version in Hello beside the protocol version: warn on minor,
refuse on major, naming both (E-51).
**Done when** e2e shows an unapproved runner getting no job and one approval
command fixing it; mismatch tests assert both versions in warning and refusal.

### M15d — Self-update and rollback (`m15d-self-update`)
A runner updates itself on the hub's word and never needs a hand on its
host (D66, the owner's operating requirement, not a chatOnS expectation).
The hub names a version and the artefact's digest; the runner fetches it
outbound, verifies the digest, and swaps only while it holds no job,
draining first. The host service keeps the previous binary: the new one must
complete a handshake and one health report inside a deadline or the service
restores the previous version and restarts it, locally, with no hub
involved. The restored runner reports what it refused and why, so a failed
rollout is read from the hub instead of from the machine. During a rollout
the hub accepts the previous protocol version, and a runner too old to
understand the instruction is told to update rather than refused.
**Done when** a real runner moves from version A to B on the hub's word with
no shell opened on its host; a B that exits on start, a B that never
handshakes and a B whose digest does not match each come back as A with the
reason readable on the hub; and no swap ever interrupts a running job.
**The justification this milestone used to carry is empty, and it is the
open question about M15d rather than a detail.** It read: the server's
native runner is updated by Nix (E-47), so M15d serves the non-Nix hosts.
There are no non-Nix hosts in the planned fleet. The only other machine is
the owner's PC, and M17's own recipe installs Nix inside WSL2 — "a login
shell for Nix" is the first thing on its list. So M15d serves no host the
plan names. It may still be wanted on its own terms (D66 calls it the
owner's operating requirement and not a chatOnS expectation, and it
carries no E), but **that case has to be made from the operating
requirement, not from a class of hosts that does not exist.** M15d is
outside v1.0.0's must set either way, since it carries no must.

### M16 — The hub deployed, the server's runner native (`m16-deploy-hub`)
Flake builds the hub image; docker compose beside the consumer's dashboard;
`wss://` through the existing reverse proxy; state and artefact volumes with
their budgets (Q9). The server's runner runs natively, unprivileged, with
`/dev/kvm`, `/dev/dri`, `/dev/vfio` and the consumer's namespaces reachable
(E-47), restarting with the host (E-52, server side). The runner's user is
not in the `docker` group and never needs to be (D78, D121): no crate in
the workspace depends on a Docker client, and the only place the workspace
names `docker.sock` is the list `privilege.rs` warns about and the test
that watches it — nothing opens, dials or executes it; a test asserts no
open file descriptor of the runner resolves to the Docker socket while a
job runs; and claim, execute, return the result and
reconnect after a service restart are all exercised with that socket
unreachable to the runner's user. The hub itself may still run in docker
compose; that is the operator's deployment and needs nothing from the
runner's user.
**Done when** a fixture job submitted from a requester outside the compose
network runs on the server's native runner over `wss://` and returns its log
and exit code (automated against the deployed pair); a hub restart under
docker compose re-asserts M2's no-loss test on the real volume; a reboot of
the server brings the runner back with no hand; the no-Docker-socket test
passes with the runner's user removed from `docker`. This milestone set
`PROTOCOL_VERSION` and `RECORD_VERSION` back to 1 and froze them: the first
deployed protocol is version 1 (D73). Frozen means a bump is a compatibility
statement needing Alexa's word, not that neither moves again. M11's record
half spent the first, `PROTOCOL_VERSION` to **2** (D148). **All three then
move before v1.0.0** (D199, Alexa, 2026-10-29): `PROTOCOL_VERSION` 2 → 3,
`RECORD_VERSION` 1 → 2, `LAYOUT_VERSION` 1 → 2. There is **no migration
path anywhere in `JobDirStore::open`**, so the last two destroy an
operator's state root as a working root; that price is accepted only
because the consumer resets their volume at v1.0.0. **One binding ordering
constraint:** the ordinary release before v1.0.0 ships at layout 1 and
record 1, no part of the triple in it — a layout-2 hub refuses to open
their existing root, and `report.rs` pushes a version-mismatched record
onto `census.unread` and continues, so a record-2 build on a version-1
root exits cleanly with every job unread, which looks like an answer
rather than a refusal.

### M17 — The owner's PC joins (`m17-pc-runner`)
The second machine, and the one with the hard conditions. Runner on WSL2: a
login shell for Nix, the documented WSL checklist, a Windows logon task, no
inbound socket, the disk budget (E-50, E-52, E-34 on real hardware). The join
recipe written and walked by a person, under 30 minutes (E-49). The owner's
switch reachable from her side by a desktop shortcut (E-33 on real hardware).
The mesh path verified rather than assumed (D11): either the mesh daemon
inside WSL (needs `/dev/net/tun`; their WSL has kernel 6.18 and systemd) or
WSL mirrored networking sharing the Windows interface — whichever works goes
in the recipe.
**Done when** the PC joins by the recipe alone and runs a fixture job from the
server's hub; the timing of the walk is recorded. Three checks are manual on
real hardware and recorded in `docs/status.md`: the 30-minute join, the
2026-10-12 WSL OOM replayed against M4's ceiling (E-30), and the shortcut
flipping the switch within 10 s (E-33).

### M18 — First consumer acceptance (`m18-onboarding`)
Close every chatOnS-gated acceptance with their deliverables (declaration
file, pack and `test --pack`, results.json, push hook, nightly command,
their executor and the status document): E-02's real verdicts, E-11's
one-entry change, E-12's `0 crates compiled`, E-20's every-QEMU
listing, E-41 live at 03:00, E-42 from their hook. **E-14's prepare
timing left this enumeration on 2026-10-31**, with the row itself,
when the consumer withdrew the must (**D275**): a retired requirement
is not M18's to walk.
**Done when** `docs/coverage.md`'s *Gated on chatOnS* list — the one
list, and the members above are **derived from it** rather than kept
beside it — is walked with chatOnS on the real machines; each item
checked or a numbered follow-up. Paced by their side; blocks nothing of
ours. **E-20 was added 2026-10-30** because its own row already said
*"M13, gated M18"*: the row's claim stood and this paragraph was what
disagreed with it, so a note that gains a `gated M18` marker gains a
place here, and never the other way round (the orchestrator, ORCH-206).

## Critical path

The scheduler spine, **M1 → M2 → M5 → M6 → M7 → M8 → M9**, is merged.

**The path to a deployed hub was M10 → M15 → M16, then M15d → M17**
(reordered 2026-10-15, D109: the owner's build server keeps being
OOM-killed by work Herd is not yet there to bound, so deployment outranks
breadth). **M10, M15 and M16 are merged**, so the hub is deployed and the
server's runner is native. What is left of that path has changed shape:
**M17 is outside v1.0.0 by Alexa's decision (D206)**, with E-49 and E-50
declared unmet by choice, and M15d's own justification is empty (see
M15d). So nothing of the original tail is on the way to the stable
release; what is on the way is M11's pack half, which D205 puts inside
v1.0.0, plus M12, M13's remainder and M14's remainder.

Deferred to after M16, each with its cost written down in D109: M11 and M12
(artefacts — so a build and its test must run on one machine), M13 (status
and events — the consumer keeps its own status files, which E-53 permits),
M14 (the scheduled job and the hook — the consumer's cron keeps them).
**M13 and M14 came off that list in part on 2026-10-29**: E-35 is merged,
E-37 is partly built and E-41's requester-side predicate is merged
(D215, D216, D217). M11's record half had already landed, so what remains
deferred is M11's pack half, M12 whole, and one must each in M13 and M14.
Still genuinely deferrable for good: E-38, E-39, E-55.

- M1 → M2: the store persists the lifecycle M1 creates; anything built before
  it would be rewritten onto it — the walk-back E-44 exists to forbid.
- M2 → M5: durable queue entries embed label requirements; the algebra's types
  must exist before entries persist in final shape.
- M5 → M6: dedup keys and queue entries carry label sets; the queue cannot be
  final on a provisional algebra.
- M6 → M7: matching claims from the priority queue; kind and slot enforcement
  is the claim step itself.
- M7 → M8: admission is a further claim-time filter inside the matcher; E-04's
  explanations annotate its refusals.
- M8 → M9: switch, drain and dry run manipulate claim and admission state that
  must exist first.
- M9, M10, M15 → M16: a first deployment needs a job that can be **declared**
  (M10), **confined and admitted** (M4, M8), **placed** (M5-M7), **switched
  off by its owner** (M9) and **authenticated** (M15). It does not need
  artefacts, a status document or a scheduler's clock — those make Herd
  broader, not deployable.
- M15 → M15d: the update instruction is an authenticated instruction.
  Without auth it is a remote code execution channel into every machine of
  the fleet (D66), because runner names are self-asserted until M15 (D57).
- M15d → M17, **not → M16** (corrected 2026-10-15): the server's native
  runner is updated by Nix (E-47), so self-update buys nothing for the first
  deployment. It serves the non-Nix hosts, which means it is owed before the
  PC joins, not before the hub ships.
- M16 → M17: the PC joins a hub that already works; its switch (E-33) and
  ceiling (E-30, via M4) are the owner's hard conditions and are checked on
  her machine, not simulated.
- M17 → M18: acceptance needs both machines on the real deployment.

**What M16 does and does not buy.** It bounds what runs on the server: a
kernel-enforced memory ceiling (M4), admission against a floor (M8), a
queue with priorities (M6). It does **not** move load off the server — the
PC has no runner until M17. And M17 before M11 gives a PC that can only
take `check` work, since without a pack a build there produces nothing the
server can test from, and the owner's rule denies it `kvm`.

## Parallel tracks

One branch per milestone; parallel milestones meeting in herd-proto or the
hub's module tree conflict, so the spine owns herd-proto and a parallel track
gets its message additions agreed before its branch opens. **The one-track
rule this line used to quote is not in CLAUDE.md and has been withdrawn.**
It read: CLAUDE.md allows one sub-agent at a time, with Alexa's agreement,
so at most one track beside the spine. CLAUDE.md contains no such rule
today; what it has is a shared-host bound — more than two heavy agents may
run while the first consumer is stopped (Alexa, 2026-10-27), bounded by
load, free memory and free disk, with one compile at a time per agent. So
parallelism is limited by the host and by the conflict surface below, not
by a count in CLAUDE.md.

- **Track R — runner** (M4, then M10): herd-runner plus a fixture consumer
  repo. Depends on M1 (M10 also on M4, Q5, Q7); must not touch the hub's
  scheduler modules. Best delegation candidate: long, isolated, few shared
  files.
- **Track S — store** (M11, then M12's engine): a new hub storage module, blob
  endpoint, runner fetch. Depends on M2 and Q2/Q4; integrates with M10's
  declared outputs at the end; M12's placement preference needs M7. Must not
  touch the scheduler. Second delegation candidate.
- **Track O — ops** (M13, M14, M15): M13 reads hub state across many modules,
  stays with the orchestrator; M14/M15 are small, after M9 and M1, before M16.
- M3 is off the formal path but touches the hub's connection and queue code;
  the orchestrator runs it between M2 and M5, not in parallel.

## Decide before coding

| Q | Decision | Recommendation | Blocks |
|---|---|---|---|
| Q1 | Hub persistence model | Directory per job: `job.json` by atomic rename, `events.jsonl` appended; no database. Atomicity is E-17's primitive reused; records stay plain files the consumer can read (E-53). | M2 |
| Q2 | The data path | **Herd is never in the byte path (D10).** The hub registers manifests and who holds what, and issues scoped fetch capabilities; runners move blobs directly. A runner advertises a fetch endpoint and binds it to one configured mesh interface only, never a default or public one (D11). Same host: hardlink/reflink, zero network. A runner with no mesh is outbound-only: it pushes to and pulls from a keeper and never serves. | M11, M12 |
| Q3 | Resource algebra shape | One Capability type {name, kind: exclusive / counted(n) / shared, excludes, busy-probe}; memory and tmpfs are numeric admissions, not labels; a lease is counted(1) with an external probe. | M5 |
| Q4 | Content addressing | Per file, not per archive. It buys three things at once: inter-pack dedup (E-13), GC pinning (E-16), and "fetch the diff" for free — a peer fetches only the hashes it lacks, so no delta protocol and no rsync is ever needed. | M11 |
| Q5 | Warm caches | Herd grows no cache engine of its own, but must actively *carry* one: (a) the artefact store is generic, so the consumer may publish cache diffs through it as ordinary packs — per-file addressing (Q4) means a 57-700 MB diff costs only its missing blobs; (b) ~~the declaration contract has a first-class prepare step with declared cache keys and a recorded duration~~ — **dropped**, see M12: the consumer's own words are "you move packs; our executor owns warm caches", so the cache-specific prepare step is replaced by D77's generic per-executor-step timestamps, which time their `up` step without Herd knowing it is a cache; (c) a runner reports which keys it holds, and placement prefers a holder (E-23), which is M12 and unbuilt. Herd never knows what a cache *is*. | M10, M11, M12 |
| Q6 | WSL memory ceiling | All job processes in one cgroup v2 subtree with MemoryMax (WSL2 ships systemd); documented fallback; verified on the real PC in M17. | M4 |
| Q7 | Public format versioning | `ran on:` line, status JSON and event payloads each carry a format version; v1 freezes at M16. | M10, M13 |
| Q8 | Authentication | Per-runner token minted at approval, hashed at the hub; no mTLS (the proxy terminates TLS); one shared requester token to start. | M15 |
| Q9 | Where state and content live, budgets | Hub volume: job records and manifests only — kilobytes, no payloads. Content lives in each runner's store under its data root, with a per-runner budget (the server's keeper share of their 80 GiB offer; the PC's 900 GiB). | M2, M11, M12 |
| Q10 | Name and licence | Settled: "Herd", Apache-2.0 (D141). `LICENSE` is verbatim; `license` is on the workspace. | M16 |
| Q11 | Runner packaging | A flake output (binary via nix profile or copied); semver carried in Hello (E-51). | M16, M17 |
| Q12 | Local foreground mode | Do not build one: local hub plus runner via `cargo run` is the debug path; chatOnS keeps its own local path. | — |
| Q13 | Schedule predicate | Exactly {local time, ref moved since last run, submission template}; anything richer is refused as workflow. | M14 |

## Conflicts and risks

- **Herd must not become a data mover (Alexa, 2026-10-12).** D7 fixed the
  wrong half of the problem: it moved packs off the JSON frames but kept them
  going through the hub. Orchestrating and relaying bytes are different jobs,
  and routing 1.5 GB through a container and a reverse proxy to reach a disk
  on the same machine is the clearest case against it. D10 supersedes D7: the
  hub registers content and authorises fetches, runners move the bytes.
- **The mesh is the owner's, not ours** (D11, approved 2026-10-12). A tailnet
  is validated as the network layer, and that is a deployment choice: Herd
  must never depend on a VPN product, because a coordination service we do not
  run would breach invariant 1 (no cloud) and a network layer inside Herd
  would breach it again. Herd receives one address to bind and one to
  advertise. With no mesh a runner is outbound-only and nothing breaks.
  Invariant 3 was rewritten for this: runners dial out for control and may
  listen on a private mesh interface, but no port is ever opened on a router.
- **Direct transfer costs something.** Peer liveness, partial holders, and
  "who actually has this blob" are harder to debug than one central store;
  custody (D10, M12) and a visible no-holder state exist to pay that cost
  honestly. Their own E-55 already asks for peer exchange, so this is the
  spec simplified, not stretched.
- **Invariant 1 judgements.** E-10 stays inside the line capped at one parent,
  start-after-success, no conditions — a primitive, not a graph. E-41 is a
  timer plus one predicate (Q13), not a workflow; if the predicate creeps, the
  fallback is a consumer-side cron submitting via herdctl. E-42 is entirely
  the consumer's hook. E-26's busy probes stay configuration strings; the
  moment they grow an API they are plugins — refuse.
- **E-14, the cache (Alexa, 2026-10-12).** The cache is load-bearing for the
  first consumer — few build machines, and a cold build is over 30 minutes —
  so "Herd does not manage it" must not become "Herd ignores it". Herd grows
  no cache engine (that would shape it around one consumer), but the generic
  artefact store, D77's per-step timestamps (**which replaced the declared
  prepare step** — see M12 and Q5) and key-aware placement must be good
  enough to carry theirs. See D9.
- **Timing vocabulary.** Invariant 2's ≤ 5 s stop, E-06's 10 s job kill,
  E-33's 10 s switch and E-27's 15 min metal drain are four different things;
  M9 specifies all four.
- **E-19 vs E-23.** Stickiness decides when a job may move; locality
  preference only orders among available eligible runners (M7 vs M12).
- **Shared host.** Hub and reference runner share the server: nightly
  exclusivity silences the runner's jobs, not the hub container; on a swapless
  31 GiB host E-29's 6 GiB floor is the only guard — enforced, never advisory.
- **E-32's p90 < 12** is an adoption outcome, not a test: tests assert the
  mechanisms; the figure is read from statistics after M18.
- **Gated on chatOnS** (fixture tests stand in until M18): the list is
  `docs/coverage.md`'s *Gated on chatOnS* section, which is derived from
  its own rows and is not restated here. This bullet named nine rows:
  one (E-53) that nothing supports; two (E-38, E-54) gated on them with
  no milestone that closes the wait, which are a named second group in
  that section; and one the rows carry (E-20) that it was missing. That
  section says which and why (ORCH-206, settled 2026-10-30).
- **Size honesty.** The sheet carries 44 musts (recounted; not 38) across
  **16** must-bearing milestones, M2 to M17 — counted from
  `docs/coverage.md`'s own *Musts per milestone* table, which lists
  sixteen rows; this line said 15 — not a handful of sessions, and pretending
  otherwise is how work gets walked back.

## Order rationale

Durability before the scheduler, or every scheduler feature is later rewritten
onto the store. The algebra before the queue, or persisted entries change
shape. Matching before admission, because admission is a filter inside the
claim step. Confinement before the command contract, so consumer commands
never once run unconfined. The contract before the store's integration,
because packs are declared outputs. Formats freeze only at deployment, so M10
and M13 can still correct them cheaply. Auth before exposure. Reordering any
of these pairs means building on a surface the next milestone must break.
