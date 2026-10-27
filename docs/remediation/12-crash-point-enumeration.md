# 12 — Crash-point enumeration, by construction

**Date:** 2026-10-27.
**Base:** `origin/main` at `cd59ef5`. Every `file:line` below was read there.
**Author:** `herd-step`, ORCH-080.
**Branch:** `orch-080-crash-point-enumeration`.
**Harness:** `herd-hub/tests/crash_points.rs`, ten `#[ignore]`d cases.

**What this piece is.** A **mesure** in this dossier's sense, of an unusual
kind: nothing is loaded and nothing is timed. Every on-disk state a power cut
can leave in one job directory is **written byte for byte**, the hub's store
is opened on it, and what the store does is asserted. The harness starts no
hub process, sends no signal, kills nothing, and writes nothing outside
`tempfile::tempdir()`.

**Why it exists.** `10-ordering-on-ext4.md`'s amendment splits the durability
tier into three steps. **A** (ORCH-083, the event `fdatasync`) is authorised
and needs no crash evidence. **B** (ORCH-084, the directory `fsync`) is held
behind this enumeration, because dropping that sync is exactly what makes some
of the states below reachable. **C** (ORCH-085, the record `fsync`) is
deferred and may never be taken. This document is the evidence **B** is to be
judged on.

**Nothing here changes the product.** No `fsync` was dropped, reordered or
added on this branch; nothing under `herd-hub/src` is touched. It describes
today's tree.

**Marks.** **F** — read at a named `file:line` on `cd59ef5`, or asserted by a
case of the harness and named. **H** — a hypothesis. **I** — unknown.

---

## 0. The three answers, in one page

**1. A `job.json` the hub cannot parse does not quarantine one job: it stops
the hub.** `JobDirStore::open_with` → `store.recover()?` (`store.rs:436`) →
`self.read_stored(&id)?` (`store.rs:899`) → the `?` on
`serde_json::from_str::<VersionProbe>` (`store.rs:1141-1142`). The error
leaves `main.rs:224` and the process exits before it binds a port. D52's
quarantine is three lines further down (`store.rs:1150-1157`) and covers
**labels only** — a record that *parses* and asks for something the rules now
reject. One byte of damage in one record is a different class of event, and it
takes the whole state root with it (F, cases
`a_torn_record_refuses_the_whole_state_root` and
`one_torn_record_takes_every_other_job_with_it`). **This is a behaviour we
have, not one we want**; §5 proposes the change and §6 says why it is not
made here.

**2. The ordering hazard is `B`'s, and it is real but narrow.** Of the 308
states built, **4** leave `events.jsonl` *ahead* of `job.json` — the one
ordering D103 forbids. All four are the same state up to the stale temporary:
the record still holds the older reason while the event file already carries
the newer one. The hub opens on it, queues the job under the record's reason,
serves the history that contradicts it, and says nothing (F, case
`an_event_ahead_of_the_record_is_neither_seen_nor_repaired`). Today that state
is **unreachable**: `write_file_with`'s `sync_dir` (`durable.rs:56`) returns
before `append_line` is issued, so the rename is on the platter before the
event exists. Dropping the directory `fsync` is what makes it reachable.

**3. Nothing removes a stale temporary at `JobDirStore::open`** — ORCH-086 is
right (F, case `nothing_removes_a_stale_temporary_at_open`). And one of them
is not merely untidy: a `.results.json.tmp-…` left in `files/` by the
inline-output write (`store.rs:1752`) is **counted against the output budget**
by `directory_bytes` (`store.rs:2730-2746`, which has no dot-filter) and is
never swept, because the sweeper ranks whole collections and this job holds
none (F, case `a_temporary_in_files_is_held_output_for_ever`). A hub that is
killed often enough loses disk it will never report as free.

---

## 1. The write under study, and what a cut can land in

`JobDirStore::record_waiting` (`store.rs:2020-2036`, F) is the write the three
steps are about. It performs, in this order:

1. `write_record` → `durable::write_file_with(job.json)` (`durable.rs:35-57`):
   `File::create(.job.json.tmp-<pid>-<n>)`, `write_all`, **R** `sync_all()`,
   `rename` onto `job.json`, **D** `sync_dir(parent)`.
2. `append_event` → `durable::append_line(events.jsonl)` (`durable.rs:64-72`):
   one `write_all` of the line and its terminator, **E** `sync_data()`.

R, D and E are `11-sync-prices.md`'s three codes, unchanged. The comment at
`store.rs:2030-2032` states the intent: *"The record first, so that the moment
the line is readable in `events.jsonl` the record agrees with it"* — which is
D103, written in the code.

A cut at an arbitrary instant leaves exactly three things to vary.

---

## 2. The enumeration, and why it is complete

### 2.1 The three axes

**`job.json`** — seven shapes:

| shape | what it is | reachable today? |
|---|---|---|
| `Older` | the rename never became durable | only with **B** taken |
| `Newer` | the rename did | yes, the ordinary outcome |
| `Torn` | a prefix of the newer bytes under the right name | only with **C** taken, or a filesystem that publishes a rename ahead of the bytes |
| `Empty` | zero bytes under the right name | as above; this is ORCH-074 §2's case |
| `Nulled` | right length, NUL bytes — a journal replay over an allocated, unwritten block | as above |
| `Absent` | the directory exists and no record ever landed: an interrupted mint | yes, today |
| `WrongVersion` | parses, `record_version` this hub does not read | not a crash state; included because it is the one *other* way `read_stored` returns `Err`, and it shares the refusal |

**`events.jsonl`** — eleven shapes. A cut can leave any byte **prefix** of the
intended content, so the prefixes are enumerated by equivalence class, plus
the two shapes a journalling filesystem can hand back that a plain truncation
cannot:

- `Boundary(0..=3)` — a prefix ending exactly at the end of line *n*;
  `Boundary(0)` is an empty file, `Boundary(2)` is the pre-write state and
  `Boundary(3)` the post-write one.
- `MidLine(1..=3)` — a prefix ending strictly inside line *n*.
- `NulWholeLine` — two whole lines, then a third line of NULs **with** its
  newline: a line that is whole and is not JSON.
- `NulTornTail` — two whole lines, then NULs and no newline.
- `NotUtf8` — two whole lines, then two bytes that are not UTF-8.
- `Absent` — no file at all.

**the stale temporary** — four shapes: `None`; `HalfWritten` and `Whole`
beside `job.json`, named as `durable::temporary_beside` names them
(`durable.rs:235-242`); and `InKeptFiles`, the same leftover inside `files/`
from the other atomic writer (`store.rs:1752`).

7 × 11 × 4 = **308 states**, every one built and opened.

### 2.2 Why that list is complete

The claim is not that 308 is every byte sequence; it is that every byte
sequence falls into one of these classes, because the reader's decision
function has very little in it.

1. **`recover` reads `job.json` and nothing else.** `events.jsonl` is read
   only by `job_events` (`store.rs:2307`), on a query — `grep -n EVENTS_FILE
   herd-hub/src/store.rs` gives `:1285` (the append) and `:2312` (that read)
   and no third site (F). So the startup verdict is a function of the record
   alone, and the table in §3 has to be read down the `job.json` column.
2. **`job_events`' per-line decision is one test**, `line.ends_with('\n')`
   (`store.rs:2318`). Two prefixes ending strictly inside the *same* line are
   therefore indistinguishable to it, and one representative per line covers
   the class. A prefix ending on a boundary is its own class, and with three
   lines there are four of them.
3. **Nothing in the tree reads a temporary.** Its contents are one class; its
   *existence* is the only variable — and §4.3 shows where even that is
   observable.
4. **`job.json` cannot be torn by this write while R is kept.** The bytes are
   `sync_all`ed before the `rename` (`durable.rs:46-48`), so a name that is
   durable names durable bytes. That is why the four refusing shapes are
   listed above as **C**'s hazard and the filesystem's, not **B**'s — a point
   `10 §2` makes and this enumeration confirms by construction rather than by
   argument.

The one thing the enumeration does **not** settle is which of these states a
real power cut on a real ext4 volume actually produces, and with what
frequency. That needs a device under the hub and a mount whose options we are
allowed to read; invariant 9 forbids reading the deployment's, and
`10-ordering-on-ext4.md` marks the cross-file ordering question **I** for the
same reason. It stays **I** here. What this piece settles is the *consequence*
of each state, which is what a decision needs.

---

## 3. What the hub does with each state

`JobDirStore::open_with`, then `unfinished_jobs()`, `quarantined()`,
`job_events()` and `sweep_outputs()`. The temporary axis changes none of the
first four, so the grid below is 7 × 11; the `held` column is the one place
the temporary shows, and §4.3 has it.

| `job.json` ↓ / `events.jsonl` → | `B(0)` | `B(1)` | `B(2)` | `B(3)` | `M(1)` | `M(2)` | `M(3)` | `NulWhole` | `NulTorn` | `NotUtf8` | `Absent` |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `Older` | 0 | 1 | 2 | **3 ⚠** | 0 | 1 | 2 | ERR | 2 | ERR | 0 |
| `Newer` | 0 | 1 | 2 | 3 | 0 | 1 | 2 | ERR | 2 | ERR | 0 |
| `Torn` | **REFUSED** | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED |
| `Empty` | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED |
| `Nulled` | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED |
| `Absent` | 0 † | 1 † | 2 † | 3 † | 0 † | 1 † | 2 † | ERR † | 2 † | ERR † | 0 † |
| `WrongVersion` | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED | REFUSED |

A number is how many events `job_events` reads back; `ERR` is a failed query
on that one job with the hub running; **REFUSED** is `JobDirStore::open`
returning `Err` and the hub not starting.

- **⚠** — `events.jsonl` is *ahead* of `job.json`. §4.1.
- **†** — the hub opens and **the job is gone**: not in the queue, not
  quarantined, one line on stderr (`store.rs:925-928`). Its `events.jsonl` is
  still there and still readable, naming a job the hub no longer has.

Totals, printed by the harness:

```
308 states built
  176 refuse the whole state root at `JobDirStore::open`
  44 open with the job silently gone from the queue
  4 leave `events.jsonl` AHEAD of `job.json` (D103's one rule)
  99 leave a stale temporary that nothing removes
  0 are quarantined (D52 covers labels only, store.rs:1150-1157)
```

---

## 4. The behaviours, one at a time: have, or want

The brief asks these two apart, and they are different answers.

### 4.1 `events.jsonl` ahead of `job.json` — **B**'s hazard

**What we have.** `Older` × `Boundary(3)`. The record says the older reason;
the third event line says the newer one. The hub opens, queues the job under
the record's reason, and a requester reading the history is told something the
record denies. Nothing detects it, nothing repairs it, nothing says it (F).

**Is it what we want?** For a `waiting` write — **yes, and the cost is
acceptable.** What disagrees is a *reason a job is not starting*, both halves
of which are rewritten within one pass of the next start (D171,
`07 §0`), and `recover` never reads `events.jsonl`, so no recovery decision
turns on it. The reader who sees it sees one stale sentence in a history,
beside a record that is authoritative and correct.

For a **decisive** write it would not be acceptable, and this is the line that
matters for ORCH-084: an `events.jsonl` claiming `finished` over a `job.json`
that says `running` is a lie about a verdict, not about an explanation. The
board already carries that constraint — *"for a decisive write the directory
sync stays unconditionally"* — and ORCH-079 is the guard. **This enumeration
supports B only under that split**, and the harness is the regression that
catches it if the split is ever lost: the `⚠` cell is asserted, by name.

**H:** the same four states are reachable for a *decisive* write the moment
`sync_dir` is dropped unconditionally, and the harness would then show
`Older` × `Boundary(3)` with a terminal event over a non-terminal record. The
harness does not build that state today, because today it cannot occur; a
branch that drops the sync unconditionally should add it.

### 4.2 A torn record refuses the whole state root — **C**'s hazard, and an
operational fact today

**What we have.** Four of the seven record shapes refuse. The error names the
file and the fault:

```
Torn:         reading …/jobs/j0000000001/job.json failed: EOF while parsing a string at line 1 column 350
Empty:        reading …/jobs/j0000000001/job.json failed: EOF while parsing a value at line 1 column 0
Nulled:       reading …/jobs/j0000000001/job.json failed: expected value at line 1 column 1
WrongVersion: …/jobs/j0000000001/job.json has record version 99, and this hub reads version 1
```

and with three jobs on the root and one of them torn, the other two go down
with it:

```
three jobs, one torn record: reading …/jobs/j0000000002/job.json failed: EOF while parsing a value at line 1 column 231
```

The index does not save it either. `usable_index` reads every record the index
leaves out, and an unreadable one there sets the index aside
(`store.rs:1052-1054`, *"A record this hub cannot read is not a record it may
call terminal"*) so `recover` reads it again and refuses. The refusal is
unconditional (F).

**Is it what we want?** **No.** Three things make it worse than it reads:

1. **The blast radius is wrong.** One job's record is one job's. Taking down
   a hub that holds a thousand other jobs — queued work, finished verdicts,
   collections nobody has fetched — because of one of them is not
   proportionate, and no other part of the store behaves this way: a
   `readings` cache that will not parse is started without, loudly
   (`store.rs:2666-2672`); an index that will not parse is rebuilt
   (`store.rs:990-993`); a `log.seq` that holds no number is reported and
   worked around (`store.rs:1231-1241`); a torn *tail* of `events.jsonl` is
   dropped (`store.rs:2317-2328`). The record is the one file with no
   degraded mode, and D52 — the decision that exists precisely to stop one
   record stopping a hub — stops three lines short of it.
2. **Nobody can fix it.** Herd is shipped, not operated (invariant 9, D161).
   The consumer runs the hub in docker compose. `JobDirStore::open_with` is
   `main.rs:224` and its error leaves `main`, so the container exits
   non-zero, restarts, and exits again. The recovery is "find the job
   directory inside the volume and delete a file", which is exactly the
   operation nobody on either side of this boundary is positioned to perform
   quickly: we may not log in, and the consumer has no reason to know which
   file or that deleting it is safe.
3. **It is the failure mode the restart loop hides.** The message is on
   stderr of a container that is dying; what the operator sees is a hub that
   will not come up.

**What it costs to leave it.** Today, very little: the record `fsync` (R) is
still paid, and `10 §2` could not establish that this state is reachable at
all on a correct filesystem. It is a latent fault, not a live one. **ORCH-085
(step C) is what makes it live**, and the board already says C must not be
asked until this is green. This enumeration's answer is that C should not be
asked until it is *fixed*, not merely measured.

### 4.3 Stale temporaries

**What we have.** 99 of the 308 states leave one, and all 99 still have it
after the hub has opened, recovered, written its index and swept (F). ORCH-086
is confirmed: nothing removes one.

They are *invisible* rather than absent: `durable::names_in` filters names
beginning with a dot (`durable.rs:216`), which is what keeps a reader from
tripping over one mid-write and is also what keeps a dead one off every
listing the hub makes.

With one exception, and it is the one that matters. `directory_bytes`
(`store.rs:2730-2746`) has no such filter, and `sweep_now` calls it on
`jobs/<id>/files` (`store.rs:661`). So a temporary left there by the
inline-output write (`store.rs:1752`) is **held output**:

```
HalfWritten: still on the disk after open, held = 0 bytes
Whole:       still on the disk after open, held = 0 bytes
InKeptFiles: still on the disk after open, held = 4096 bytes
```

It counts against the budget D152 governs, and the sweeper cannot take it
back: the sweeper evicts *collections*, ranked off records
(`store.rs:698-701`), and a job whose only bytes are a dead temporary has no
collection to rank. The bytes are charged for ever.

**Is it what we want?** **No, and it is the cheap one.** `07 §5` item 5
already proposes the sweep at `JobDirStore::open`, which already scans; this
piece adds that the sweep must cover `jobs/*/files` and not only the job
directory, and that a temporary in `files/` is a budget leak and not only
clutter. A sweep at open is safe because a temporary is, by construction,
nobody's: `write_file_with` removes its own on failure (`durable.rs:50-53`),
and a live one belongs to the single hub that holds the state root's lock
(D21).

**One leftover *is* removed at open** — `files.gone`, by `sweep_now`
(`store.rs:654-670`). So the absence is a gap in a mechanism that exists,
which is the easiest kind to close.

### 4.4 The interrupted mint

**What we have.** A job directory with no `job.json` is `Stored::Missing`: the
hub opens, the job is not in the queue, is not quarantined, and one line goes
to stderr (`store.rs:925-928`). Its `events.jsonl`, its `log` and its
`log.seq` stay on the disk for ever, naming a job that no longer exists (F).

**Is it what we want?** **Mostly yes.** `mkdir` is what claims an id
(`store.rs:1397-1411`) and the id staying spent is correct. A requester that
was told this id never got one — the mint had not returned — so no promise is
broken. What is slightly wrong is that the directory is never reported again
after that one line and never cleaned, so a state root accumulates them. That
is cosmetic beside §4.2 and §4.3 and is not proposed here.

### 4.5 `job_events`' two refusals

**What we have.** A torn *tail* is dropped and is not an error; a whole line
that is not JSON, and a file that is not UTF-8, are errors:

```
NulWholeLine: open succeeds, `job_events` says — reading …/events.jsonl:3 failed: expected value at line 1 column 1
NotUtf8:      open succeeds, `job_events` says — reading …/events.jsonl failed: stream did not contain valid UTF-8
```

Neither reaches `JobDirStore::open`, which never reads the file. So both are a
failed query on one job with the hub running — the right blast radius, and the
contrast with §4.2 is the whole argument of §5.

**Is it what we want?** The torn tail: yes, and `store.rs:2319-2321` says why
in one sentence. The two errors: **arguable**. A history is a history; a
reader asking for one and being handed an error learns less than a reader
handed the lines that are whole plus a line saying the rest is unreadable. But
the error is contained, it is named, and it is not on a path that stops
anything. Not proposed.

---

## 5. The proposal, which is not made here

**Quarantine a record the hub cannot read, instead of refusing the state
root.** Concretely: at `store.rs:1141-1142` and `store.rs:1160-1161`, a
`serde_json` failure becomes `Ok(Stored::Quarantined(…))` rather than `Err`,
the way a rejected label set already does three lines below; `recover` then
puts it on the quarantine list, names it at every start (`store.rs:933-940`),
and the hub comes up with every other job intact.

Why it fits what is already there:

- **D52 already decided the principle**, and its own comment states it: *"the
  alternative is a hub that will not start because of one record it minted
  itself"* (`store.rs:1129-1135`). The decision's reasoning covers the torn
  record exactly; only its implementation stops short.
- **The machinery exists.** `Stored::Quarantined`, the list, the per-start
  message, `quarantined()` on the store and the count in `main.rs:234` are all
  built. The change is which arm a parse failure takes.
- **The quarantined job is already excluded from everything that acts.**
  `read_record` refuses it by name (`store.rs:1172-1178`), `unfinished_jobs`
  skips it (`store.rs:2354-2359`), `collections_owed` skips it
  (`store.rs:2416-2420`). Nothing would dispatch a torn record.
- **It keeps the record untouched.** D52's phrasing — *"Its record is
  untouched; the hub is running"* — is what makes this safe to propose: the
  bytes stay for whoever wants to look, and Herd does not repair what it
  cannot read.

What it changes that needs a decision:

1. **The version mismatch arm should probably stay an error**, or at least is
   a separate question. `RECORD_VERSION` is a format contract, and a hub
   quietly running past a state root a newer Herd wrote is the downgrade
   hazard `00-baseline.md §4` names. A parse failure is damage; a version
   mismatch is the wrong binary.
2. **"The hub starts and is missing jobs" is a different failure than "the hub
   does not start"**, and it is quieter. It must not be quiet: the count at
   `main.rs:234` and the per-start line are the mitigation, and whether they
   are loud enough for a container's log is a judgement, not a fact.
3. **It is a behaviour change on a crash path**, so it is its own task, its own
   test and its own decision entry.

**It is not made on this branch.** ORCH-080 is an enumeration; the brief says
propose, do not make. The proposal belongs on the board as a task of its own,
and it should be ranked **before** ORCH-085, because C is what turns this from
latent into live.

---

## 6. What this settles for ORCH-084, and what it does not

**Settles.** Dropping the directory `fsync` on the `waiting` write makes
exactly one new state reachable — `Older` × `Boundary(3)`, four of the 308
counting the temporary axis — and that state is an `events.jsonl` one line
ahead of a `job.json`, over a *reason*, which the next dispatch pass rewrites.
It makes no record unreadable, loses no verdict, and changes nothing recovery
reads. The harness asserts it, so a branch that widens it past `waiting` goes
red.

It also settles that **B must not be taken for a decisive write**, which the
board already says and which this enumeration now shows rather than argues.

**Does not settle.**

- **I1** — whether a real ext4 power cut on the deployment's device produces
  `Older`, `Newer`, or something the eleven event shapes do not cover. That
  needs the device and the mount options, and invariant 9 forbids both.
  `10 §2`'s **I** stands.
- **I2** — how often. Nothing here is a frequency.
- **H1** — that the same four states, for a decisive write, would be seen by
  nothing either. The hub's recovery does not read `events.jsonl`, so the
  mechanism is the same; nobody has built that state, because today it cannot
  exist.
- Whether §5's quarantine is the right repair, which is Alexa's.

---

## 7. Running it

```sh
nix develop --command cargo test -p herd-hub --test crash_points \
  -- --ignored --nocapture --test-threads=1
```

Ten cases, all `#[ignore]`d, about 12 s on this workstation. The enumeration
asserts the table above rather than only printing it, so it cannot drift from
the code without going red. The other nine are the findings of §4 and §5, one
each, so a reader can run one.

---

## 8. F, H and I

**F** — observed at `cd59ef5` or asserted by a named case.

1. `recover` reads `job.json` only; `events.jsonl` has two sites in
   `store.rs`, the append at `:1285` and `job_events` at `:2312`.
2. The refusal chain is `store.rs:1141-1142` → `:899` → `:436` →
   `main.rs:224`, and it leaves `main`.
3. D52's quarantine is `store.rs:1150-1157` and covers labels only.
4. `usable_index` cannot route around a torn record (`store.rs:1052-1054`).
5. Four of the seven record shapes refuse; 176 of the 308 states do.
6. 44 states open with the job gone and no quarantine entry.
7. 4 states leave `events.jsonl` ahead of `job.json`; none is reachable today.
8. 99 states leave a stale temporary; all 99 still have it after open.
9. `directory_bytes` has no dot-filter, so a temporary in `files/` is held
   output — 4096 bytes measured — and the sweeper cannot take it.
10. `files.gone` *is* removed at open (`store.rs:654-670`); temporaries are
    not.
11. `job_events` drops a torn tail, errors on a whole line that is not JSON,
    and errors on a file that is not UTF-8 — none of which stops the hub.

**H**

1. A decisive write with the directory `fsync` dropped would reach the same
   four states and be seen by nothing; not built, because it cannot occur
   today.
2. §5's change is small in lines and contained, because the quarantine
   machinery is already built and already excludes the job from every acting
   path.

**I**

1. Which of these states a real cut on the deployment's ext4 volume produces.
2. With what frequency.
3. Whether the deployment's mount options foreclose the refusing shapes —
   `10 §2` and board row ORCH-085 both carry this, and invariant 9 keeps it
   unanswerable from here.
