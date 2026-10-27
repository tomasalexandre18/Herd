# 10 — Holding D103's order on ext4, and what adopting `07` takes

**Date:** 2026-10-27.
**Base:** `origin/main` at `96fc10b`, verified in this worktree
(`git rev-parse origin/main`), plus `06`, `07` and `08` read from that tree.
**Author:** `herd-planner`, board row ORCH-074.

**Which kind of piece this is: a constat**, in the sense
[`README.md`](README.md) gives the word — produced by reading the repository
at the base above. It ran no build, no test, no load and no experiment on
Herd, and it changes no product file.

**One exception, declared because it is not a reading of the repository.**
Four facts in §3 come from reading this workstation's own kernel interfaces
— `/proc/mounts`, `/proc/fs/ext4/nvme0n1p1/options`, `zpool status`, `lsblk`,
and `man 5 ext4` from `e2fsprogs-1.47.4`. They are a measurement of **this
host's configuration**, not of Herd, and not of anything inside the first
consumer's files: the one `ls` that reached toward their volume returned
`Permission denied` and was not retried. Read them as host configuration,
quoted with the host, as `README.md` requires of any figure.

**Marks.** **F** — read at a named `file:line` on the base, or read from a
named host interface. **H** — a hypothesis, with the measurement that would
settle it. **I** — could not be decided here.

**Numbering.** This piece is `10` and not `09`, deliberately. `09` is taken:
`docs/remediation/09-runner-self-limits.md` exists on the pushed branch
`origin/c5-announced-job-ceiling` and will arrive on `main` with C5; `11` is
taken by `11-sync-prices.md` (ORCH-077, PR #90), which this piece's amendment
answers to. See §0.1.

---

## Amendment, 2026-10-27 — H-1 is refuted, and the recommendation changes

**Written after the rest of this piece, against
[`11-sync-prices.md`](11-sync-prices.md) (ORCH-077, PR #90), which was
measured to test H-1 and refutes it.** The body below is left as it was
signed, as `README.md` requires — *"where a later piece overturns an earlier
one, the later one wins, and the earlier one is not edited"* — except that
every passage H-1 governs now carries a pointer here. **Read this section
before §0.2, §4 and §5.1.**

### What `11` measured

1. **One `fsync` on ext4/NVMe is 989 µs** (iqr/med 7.3 %), not the
   tens-to-low-hundreds of microseconds H-1 assumed: 3.3x the top of that
   range, and only **1.2x** better than the 1.19 ms derived from `08`.
   **H-1 is refuted.**
2. **The three syncs are additive, not redundant.** Three marginals summing
   to 3.05 ms against a measured `RDE` of 3.00 ms, the two marginal columns
   agreeing within 11 % (`11 §6`). The mechanism that would have made a kept
   sync nearly free is not operating.
3. **`08`'s 9.970 s junction at `N` = 1 000 is 3.128 s on ext4**: 3.2x
   smaller, **not** an order of magnitude. §8 item 5 said to stop quoting
   9.97 s for the deployment and was right about the direction and wrong
   about the size. **The problem does not go away on ext4:** three seconds of
   durable writing in one pass is still the dominant cost of that pass.
4. **`06`'s `sync_dir` at 6.6 µs is an artefact** and is corrected by about
   two hundred times. `06`'s loop synced the file and then the directory with
   **no `rename` between them**, so the directory's only change was already
   committed; `durable::write_file_with` renames between the two, so the real
   directory sync always has dirty metadata and always pays — **991 µs on
   ext4, 1.32 ms on ZFS** (`11 §5`). This is also the gap `08` recorded and
   did not diagnose.

### What that does to the numbers in this piece

`11`'s measured pass at `N` = 1 000, ext4/NVMe, replaces every derived figure
in §0.2 and §4:

| | syncs kept | pass at `N` = 1 000, ext4 | prize collected |
|---|---|---|---|
| today | R + D + E | **3.128 s** | — |
| **M1** drop E only | R + D | ~2.1 s (from `11 §6`'s `RD-`) | **~31 %** |
| **M3a** drop D and E | R | **1.085 s** | **64.5–66.4 %** |
| **M3b** drop all three | none | **52.85 ms** | **~98 %** |

Two corrections of wording, since both my brief and `11`'s own prose understate
this: M3a is **two thirds off, not a third off** — `11 §8`'s own table says
64.5 % per write and 66.4 % per pass. What is "a third" is the **kept** sync:
it still costs 1.10 ms, 21x the 51 µs unsynced floor, and alone it is 35 % of
what a waiting write costs today.

### What survives, and what I reverse

**§8 item 5 survives and is sharpened**, §2's hazard survives untouched (it is
a reading of the code, not a figure), §1's five facts survive untouched (same),
and §3's documentation finding survives untouched. **`07`'s shape survives**:
`11 §8` says three seconds in one pass is still dominant, so the keeper's
argument is intact with a 3x smaller baseline.

**What I reverse: M3a is no longer the right *first* increment.** I recommended
it on the strength of H-1 — that the kept sync would be nearly free, so the
integrity guarantee of Leg 1 would come almost for nothing. It is not nearly
free: it costs **1.03 s per pass at `N` = 1 000**, and the gap between M3a and
M3b is now a factor of **21**, not the factor of 3 H-1 implied. An integrity
guarantee that is contingent anyway — foreclosed on this mount by
`auto_da_alloc` and `data=ordered` (§3.2) — is a different proposition at
1.03 s per pass than at 50 µs.

**But the answer is not "therefore M3b".** It is that `11`'s additivity result
changes the *shape* of the decision: three syncs, three independent thirds, so
**each sync is its own increment with its own justification**, and they should
be taken one at a time rather than as one jump. That is a smaller first step
than I proposed, not a larger one.

### The revised recommendation: three steps, not one

| step | drop | prize, cumulative | what it needs | crash evidence |
|---|---|---|---|---|
| **A** | **E**, the event `fdatasync` | ~31 % | ORCH-052 only, **already granted** | **none, of any kind** |
| **B** | **D**, the directory `fsync` | ~65 % | §1's D103 scope clarification | the enumeration (§0.3 tier 2) |
| **C** | **R**, the record `fsync` | ~98 % | §2's integrity hazard answered | tier 2, and arguably tier 4 |

**Step A is the one to take first, and `11` is what makes it obvious.** At
these figures **§4's M1 row becomes the interesting one**, because a third is
now the going rate for *every* sync and M1 is the only third that costs
nothing anywhere else. Dropping E alone:

- **cannot violate D103 in any direction.** The record keeps both its syncs
  and stays fully durable; the event line merely becomes less durable. D103
  forbids `events.jsonl` being *ahead*, and step A can only leave it
  **behind**, which D103 explicitly permits — *"may be one transition behind
  the record"*.
- **assumes nothing about the filesystem**, keeps §2's integrity guarantee
  whole, and needs no crash test of any kind.
- **needs only ORCH-052**, which Alexa granted on 2026-10-26.
- is reversible by the same flag, and touches one line of `durable.rs`.

**Step B completes M3a** and is where §1's scope argument and the crash-point
enumeration are actually spent. Note what `11 §5`'s correction adds here that
I had wrong: §0.2 dismissed the directory sync as "the 6.6 µs directory sync",
and **D is not a rounding error — it is a full third, 991 µs.** The safety
reasoning in §0.2 is unchanged (losing a rename yields the whole old record),
but the economics are: step B is worth as much as step A, not nothing.

**And one hazard that correction exposes, which I had under-rated.** For a
**decisive** write, D is load-bearing for D103: `sync_all` makes the record's
bytes durable but only `sync_dir` makes its *rename* durable, so a transition
written without D could be lost while its `events.jsonl` line, still carrying
`sync_data`, survives — `events.jsonl` genuinely **ahead** of `job.json`.
`durable.rs`'s own comment says it: *"The name, too, has to survive: without
this the rename itself can be lost while the file's contents are safe."*
**So D stays, unconditionally, on every decisive write**, and the task that
catches anyone dropping it is the ordering oracle, which is now more valuable
than when I proposed it, not less.

**Step C stays a separate, later decision** and may never be taken. It is the
only step that touches §2's record integrity.

### What this does to §6's task ordering

**It survives, with one task split in three and one already done.** Task 1 is
`11`; it did its job and refuted the hypothesis it was set to test, which is
what a first task is for. Tasks 2 and 3 — the ordering oracle and the
crash-point enumeration — survive and the oracle **gains** importance, because
`11 §5`'s correction shows a real way to break D103 that only an ordering
oracle catches. Tasks 4 and 5 (the stop path, the lock) are untouched: neither
depended on a figure. Task 6, the tier, **splits into steps A, B and C above**,
of which A needs no decision beyond ORCH-052. Tasks 7 to 10 are untouched.

**The order changes in exactly one way:** step A no longer waits on the
crash-point enumeration, so the first *product* change in this programme can
now be the stop path and then step A, with B behind the enumeration. Nothing
else moves.

---

## 0. The answer, in one page

### 0.1 Two premises of the brief are wrong, and one is wrong in my favour

**First, `09` is not free.** The brief asked for
`docs/remediation/09-ordering-on-ext4.md`. `git ls-tree origin/c5-announced-job-ceiling
-- docs/remediation/` lists `09-runner-self-limits.md` (**F**). `README.md`
records that one dossier number has already been reused deliberately and
says so at the point of reuse; a second reuse, this time by accident and
across two in-flight branches, is not worth the confusion. This is `10`.

**Second, and this is the substantive one: the hazard ORCH-074 names is not
the hazard that matters.** ORCH-074 asks for the mechanism that holds D103's
ordering on ext4. Read against the code, **the writes `07` proposes to
de-sync are not the writes D103 binds** (§1), so there is no ordering
mechanism to invent for them — and meanwhile the same de-sync puts something
much larger at risk that no piece of this dossier has named:

> A `job.json` that comes back empty or torn does not cost one stale wait
> explanation. It makes the hub **refuse to start on the whole state root** —
> every queued job and every uncollected verdict unreachable — because
> `read_stored` parses the version probe with `?` (`store.rs:1136-1142`),
> `recover` reads every non-terminal record with `?` (`store.rs:899`), and
> `JobDirStore::open` calls `store.recover()?` (`store.rs:436`). D52's
> quarantine, which would have contained this to one job, covers **labels
> only** (`store.rs:1150-1157`). **F.**

So the question to answer is not "what holds the order" but **"which of the
three syncs buys what"** — and the dossier has never separated them.

**Third, in my favour: the deployment's filesystem is not the one any figure
in this dossier was measured on, and the one it is on is faster.** `08`'s host
table is accurate — `rpool` is a mirror of two SATA Toshiba SSDs with no SLOG
(`zpool status rpool`, **F**) — and that is where its 8.1–10.8 ms per write
comes from. The state volume the first consumer reported, and the partition
this workstation's `/mnt/ssd_980` is on, is **ext4 on a Samsung 980 NVMe**
(`/proc/mounts`, `lsblk -o TRAN,MODEL`, **F**). `08`'s own `I`-3 says every
figure in it is "one workstation, one filesystem, one device" and that the
deployment's is unmeasured. It is still unmeasured — and it is reachable from
any agent's scratchpad without touching anything, because `TMPDIR` on this
host is already on it.

### 0.2 The mechanism, named

> **Amended.** The destination below is unchanged and still right; what the
> amendment changes is that it is reached in **three steps and not one**,
> because `11` measured the three syncs as three independent thirds. The
> first step drops only the `fdatasync` on the event line.

**"The record's bytes, and nothing else."** One `fsync` survives, on the
temporary file, and the other two go:

| | today | recommended |
|---|---|---|
| `fsync` the temporary before the rename | yes | **yes, kept** |
| `fsync` the parent directory after the rename | yes | **no** |
| `fdatasync` the appended `events.jsonl` line | yes | **no** |

Two legs hold it up, and they are independent of each other.

**Leg 1 — integrity, with no filesystem assumption at all.** Keeping
`sync_all()` on the temporary means the new record's bytes are durable before
the rename is even issued. A power cut then leaves either the whole old
`job.json` or the whole new one, on **any** filesystem — no `auto_da_alloc`,
no `data=ordered`, no transaction groups, nothing to read a man page about.
The hub-wide start refusal of §0.1 is foreclosed by construction. The
directory sync is what buys durability of the *rename*, and losing a rename
costs the whole old record, which is a loss of one explanation and not of
integrity. **Corrected by the amendment:** this paragraph priced that sync at
`06`'s 6.6 µs. `11 §5` shows that figure is an artefact of a loop with no
`rename` in it; the real cost is **991 µs on ext4**, a full third of a waiting
write. The safety reasoning here is unchanged; the economics are not, and for
a *decisive* write that sync is load-bearing for D103.

**Leg 2 — ordering, by scope and not by mechanism.** D103 binds state
transitions. The writes being de-synced are not transitions, and the record is
**already** permitted not to reflect them. Five facts, §1.

**What it costs the prize, and the honest gap.** `08` measured three syncs
against none. Nobody measured one against three. Decomposing `08` phase S0a
against `06` phase 0 gives three roughly equal thirds of the 3.57 ms synced
floor — temporary `fsync` ~1.19 ms, rename plus directory `fsync` ~1.17 ms,
event `fdatasync` ~1.21 ms — so on **`08`'s host** this mechanism lands
between **1.2 and 3.4 ms per job** — two thirds of the way to the floor at
best, and above the 1 ms threshold `08` set for itself at either bound. The
two bounds, and why there are two, are in §4. That is not good enough to recommend on
its own. What makes it good enough is the device: one `fsync` on NVMe is tens
to low hundreds of microseconds, not 1.19 ms, and the deployment is NVMe.
**H-1, with the measurement that settles it in §5.1 and the plan's first
task.** If H-1 falls, §4 M1 is the fallback and the prize shrinks to a third.

> **H-1 fell. `11-sync-prices.md` measured one `fsync` on ext4/NVMe at
> 989 µs and the three syncs as additive, so this whole paragraph's
> arithmetic is superseded: M3a collects 64.5–66.4 %, not nearly all, and
> M1 — here called the fallback — is the step the amendment recommends
> taking first. See the amendment above.**

### 0.3 How a power cut gets tested

Four tiers, in the order I would spend effort on them. The headline: **the
test that actually proves D103 needs no power cut at all**, and the test that
covers the real risk needs no hardware either.

1. **A syscall-*ordering* oracle.** `06` and `08` both *counted* syncs;
   neither ordered them. For every state transition, assert under `strace`
   that the record's `fsync` has returned before the first byte of the event
   line is written. That is D103's guarantee, expressed as an observable, with
   no crash and no filesystem in it. Deterministic, CI-able, green on `main`
   today, and a regression test every later step must keep green.
2. **Crash-point enumeration, by construction.** Do not crash — *build* every
   state a cut could leave: the previous `job.json` beside an `events.jsonl`
   truncated at every line boundary and at a mid-line byte, with and without a
   stale `.job.json.tmp-<pid>-<n>`, and the pathological empty `job.json` of
   §0.1. Start a real hub on each and assert what it does. This enumerates the
   reachable post-cut states instead of sampling them, needs no root, no VM and
   no device, and it is where the actual risk lives — recovery, not writing.
3. **`kill -9`, which the tree already does.** `killed_hub.rs` kills the hub
   twelve times in one job and asserts no temporary is left behind
   (`killed_hub.rs:423-434`, **F**). It proves nothing about a power cut — the
   page cache is the kernel's and survives the process, as `08` says — and it
   is still the oracle for `07`'s V8.
4. **A real cut, in a VM.** `dm-log-writes` is present on this host as a module
   (`modinfo dm-log-writes` → `/run/booted-system/kernel-modules/.../dm-log-writes.ko.xz`,
   **F**) and `libvirt` is installed (`/var/lib/libvirt/images`, **F**), so it
   is feasible — in a guest, never on this host's block devices. It needs root
   and a block device, so it needs Alexa's word, and I would not spend it until
   tiers 1 and 2 are green and have left a gap.

**What cannot be tested with what we have, stated plainly.** Nothing we can
run proves the **device** honours a cache flush. `dm-log-writes` models the
cut at the block layer and trusts FLUSH/FUA; a drive that lies about its cache
defeats every tier above. The weaker evidence that stands in for it: the
mount's `barrier` setting, read from the host (§3), and the device's own
specification. Say that, and do not claim more.

---

## 1. What D103 binds — read from the decision and from the code

D103 (`docs/decisions.md:2801`, quoted in `docs/coverage.md:70`):

> `job.json` is the authority on where a job stands **now**. `events.jsonl`
> is its history, and may be one transition behind the record. It is **never
> ahead of it**: a line in `events.jsonl` means the record already agrees
> with that line, or has moved past it.

Five facts put `record_waiting`'s pair outside that rule.

**F-1 — a `waiting` line is not a transition, by the code's own classifier.**
`JobEventKind` has seven variants (`job.rs:1518-1600`), and
`JobEvent::state()` returns `Some(state)` for `Transition` and **`None`** for
`Waiting`, `LogGap`, `AttemptStarted`, `AttemptEnded`, `Joined`,
`PriorityRaised` and `OutputsRemoved` (`job.rs:1495-1506`). The type's own
doc comment says "A job's history is not only its transitions … a reader that
wants the transitions reads the `transition` lines" (`job.rs:1509-1517`).
D103's sentence is about transitions; `Waiting` is not one.

**F-2 — the record is already permitted not to reflect a `waiting` line, and
the code says so twice, on purpose.**

- `move_to`, `store.rs:1322-1325`: *"A job that is no longer queued is no
  longer waiting for anything: the reason goes, while the `waiting` lines of
  `events.jsonl` keep the history of why it waited."*
- `raise`, `store.rs:2817-2820`: *"Cleared rather than rewritten here: the
  next dispatch pass writes the true one, and `events.jsonl` keeps the one
  that was."*

Both produce, today, a job whose `events.jsonl` names a reason its `job.json`
does not. Whatever a power cut could leave is in that same class, reached by a
different route.

**F-3 — recovery never reads `events.jsonl`.** `recover` lists `jobs/` through
`durable::names_in` and reads `job.json` through `read_stored`
(`store.rs:860-899`); `EVENTS_FILE` appears at four places in `store.rs`
(`:1285` the append, `:2312` the `job_events` query, `:3172` and `:3197`
tests) and at none of them in a recovery path. D103's own crash argument is
*"recovery reads `job.json`, so the hub that came back would disagree with its
own history for ever"* (`decisions.md:2826-2831`) — and that argument is
against writing `events.jsonl` **first**, which nobody proposes.

**F-4 — a torn append is already handled, and named.** `job_events` discards
an unterminated last line with the sentence *"A transition is only a
transition once its line is whole; this one is not there yet"*
(`store.rs:2307-2325`).

**F-5 — the timestamps are not comparable, by decision.** `Timestamp`'s doc:
*"Wall-clock time can step backwards, so the *order* of a job's transitions is
the order of the lines in `events.jsonl`, never a comparison of these"*
(`job.rs:58-61`). So a lost `record_waiting` leaving `updated_at` behind the
event line's `at` is not an observable inconsistency; nothing is allowed to
compare them.

**And the transitions themselves keep their ordering for free, on any
filesystem.** `move_to` writes the record and then appends the line
(`store.rs:1345-1348`, with the comment *"The record first, so that the moment
a transition is readable in `events.jsonl` the record agrees with it"*), and
`write_record` → `durable::write_file_with` **returns only after
`file.sync_all()` and `sync_dir(parent)`** (`durable.rs:35-56`). The event
append is not *issued* until the record is durable. That is not a filesystem
guarantee, it is a syscall-return guarantee, and it survives every candidate
in §4 because every candidate leaves the decisive tier alone.

**What is left, then, and it is small.** After a cut, `events.jsonl` may hold
a `waiting` line that `job.json`'s `waiting_for` does not name. Already
reachable (F-2), already unordered against the record (F-5), never read on
recovery (F-3). **It is not a D103 violation and it needs no mechanism — it
needs one sentence written down**, in `coverage.md`'s E-53 row and in the
decision entry, saying that D103 binds transitions. That sentence is Alexa's
to approve (§7), because clarifying the scope of a decision is a decision.

---

## 2. The hazard that does need a mechanism

Not ordering. Integrity of `job.json`, and its blast radius.

**F-6 — a `job.json` that will not parse stops the hub, not the job.**
`read_optional` returns `Ok(Some(""))` for a zero-length file
(`store.rs`, `read_optional`); `serde_json::from_str::<VersionProbe>("")`
fails; `read_stored` converts that to a `StoreError` with `?`
(`store.rs:1141-1142`); `recover` propagates it with `?` (`store.rs:899`);
`JobDirStore::open` calls `store.recover()?` (`store.rs:436`). The
`RECORD_VERSION` doc states the consequence in as many words: *"because that
error comes out of recovery, the hub refuses to start on that state root
rather than read it"* (`job.rs:27-32`). D52's quarantine returns
`Stored::Quarantined` for **labels** only (`store.rs:1150-1157`) — a parse
failure is never quarantined.

**F-7 — and it lands on exactly the jobs at risk.** `recover` reads the full
record set only when the index is not trusted; when it is, it reads the
*pending* (non-terminal) sequences (`store.rs:883-899`). `record_waiting`
writes only to a job that is `Queued` (`store.rs:2024`), which is
non-terminal, which is pending, which is read at every start.

**Why `07` does not account for this.** `07 §3 C` says of the tier: *"What is
given up is only survival of a power cut"*, and `07 §4`'s table says
*"migration: state root — none, forward and backward compatible"*. Both are
true of a lost **field**. Neither is true of a lost **file**, and
`write_file_with`'s own comment is the warning: *"The bytes, before the name:
a rename that beats its own contents to the disk would publish an empty
file"* (`durable.rs:44-45`). Dropping that `sync_all()` hands the premise of
that comment to the filesystem. **This is the one place where `07`'s costing
is incomplete, and it is why §0.2's mechanism keeps that one sync.**

---

## 3. What ext4 actually guarantees — what I could read, and what I could not

The brief said to read rather than assume, and to say so if documentation
will not settle it. It will not, entirely.

### 3.1 The host, and the mount, read from the host

`/proc/mounts`: `/dev/nvme0n1p1 /mnt/ssd_980 ext4 rw,relatime,stripe=32`, and
the same partition also at `/var/lib/chatons-leash` and
`/var/lib/libvirt/images`. `lsblk -o TRAN,MODEL`: `nvme0n1`, `nvme`, *Samsung
SSD 980 1TB*. `/proc/fs/ext4/nvme0n1p1/options`, the **effective** options
rather than the ones named at mount time:

```
data=ordered   barrier   auto_da_alloc   journal_checksum   delalloc
commit=5       max_batch_time=15000      min_batch_time=0
dioread_nolock errors=continue           block_validity    stripe=32
```

Two readings of that block beyond the obvious. **`errors=continue`**: an ext4
error does not remount read-only, which is the environment `03b`'s "one
unwritable record aborts the pass" lands in. **The state root's filesystem is
not dedicated** — `libvirt` images and `chatons-leash` are on the same
partition. That answers `07 §9` item 3 ("whether the deployment's state root
is a dedicated filesystem") with **no**, and it kills the `syncfs`
group-commit fallback `07` reserved for the case where the tier is refused:
`syncfs` there would flush a partition the neighbours are using (§4 M4).

`08`'s host is a different thing again, and its table was right: `zpool status
rpool` shows `mirror-0` of `ata-TOSHIBA_THNSNJ128GCST` and
`ata-TOSHIBA_THNSNF128GCSS`, SATA, no SLOG, and `/` is `rpool/root`.

### 3.2 What `man 5 ext4` on this host does say

From `e2fsprogs-1.47.4`'s page, quoted:

- *"Metadata is always journaled."*
- `data=ordered`, the default and the effective mode: *"All data is forced
  directly out to the main file system prior to its metadata being committed
  to the journal."*
- `auto_da_alloc`, on: *"ext4 will detect the replace-via-rename and
  replace-via-truncate patterns and force that any delayed allocation blocks
  are allocated such that at the next journal commit, in the default
  data=ordered mode, the data blocks of the new file are forced to disk before
  the rename() operation is committed. … avoids the 'zero-length' problem that
  can happen when a system crashes before the delayed allocation blocks are
  forced to disk."*
- `barrier`, on: *"Write barriers enforce proper on-disk ordering of journal
  commits."*
- `commit=5`: *"Start a journal commit every nrsec seconds. The default value
  is 5 seconds."*

So on **this mount**, the zero-length `job.json` of §2 is foreclosed by the
filesystem, documented, without any `fsync`. That is a real and welcome fact.

### 3.3 What I could not establish, and will not reason from the name

ORCH-074's candidate "ext4's own `data=ordered` semantics **if** you can
establish what they actually guarantee across two files" — **I cannot
establish it from documentation available on this host, and I am not going to
infer it.** What is missing is the step from *"journal commits are ordered on
disk"* to *"an operation issued at t1 cannot be durable after an operation
issued at t2 > t1"*. That step needs the rule that a metadata operation joins
the currently-open transaction, so that syscall order maps to non-decreasing
transaction numbers. The man page does not say it. No kernel
`Documentation/filesystems/ext4/` is present in the Nix store — searched, the
only ext4 documentation on this host is the `e2fsprogs` man page. **I.**

Three consequences, and they are why §0.2's mechanism does not lean on any
of this:

1. The cross-file ordering argument is **undocumented here**, not false. It
   may well be true. It is not something to put a shipped guarantee on.
2. Even if it were documented, it would be a guarantee of **ext4 with these
   options**. Herd is shipped, not operated (D161, invariant 9): the consumer
   runs every instance, a mount option is one line of someone's NixOS config,
   and a future consumer may be on anything. A guarantee we cannot see is not
   a guarantee we can hold.
3. `auto_da_alloc`'s protection of §2 is in the same position — documented for
   this mount, not for every mount. Which is exactly why the recommendation
   keeps the temporary's `fsync` and buys that protection in Herd rather than
   renting it from the filesystem.

---

## 4. The candidates, compared

> **Superseded in its figures by the amendment.** Every per-job and junction
> figure below is derived from `08`'s ZFS/SATA campaign. `11-sync-prices.md`
> measured them on both classes: on ext4/NVMe the pass at `N` = 1 000 is
> 3.128 s today, 1.085 s under M3a and 52.85 ms under M3b. The *ranking* of
> the candidates on durability and filesystem assumption — which is what this
> section is for — is unchanged; the prices are not.

Costs are per `record_waiting` on **`08`'s host** (ZFS, SATA mirror, no SLOG),
because that is the only host anything was measured on; the right-hand column
says what each one assumes about the filesystem, which is where they really
differ. Junction figures are at `N` = 1 000, where today's is 9.97 s
(`08` phase S0d) and 9.69 s (`06` phase 2).

**The two bounds in the M1 and M3a rows, and why there are two.** `08`'s
synced *floor* is 3.57 ms while its measured *per job* is 8.1-10.8 ms; the
difference is ZFS transaction-group stalls (`06`, `08` `F`-7). The low bound
assumes the stalls go with the syncs that cause them, the high bound assumes
they do not and scales the measured figure by the floor ratio. **Nobody has
measured which**, and that is exactly what H-3 and the plan's first task are
for. Treat both as derived, not measured.

| | syncs left | per job (08's host) | junction | filesystem assumption | crash test needed |
|---|---|---|---|---|---|
| **today** | 2 `fsync` + 1 `fdatasync` | 8.1–10.8 ms **F** | 9.97 s **F** | rename is atomic | none |
| **M1** sync the record, not the event | 2 | 2.4–7.1 ms **H** | 2.4–7.1 s **H** | rename is atomic | **none** |
| **M3a** ~~recommended~~, see amendment: sync the temporary only | 1 | **1.10 ms, measured (`11`)** | **1.085 s, measured (`11`)** | rename is atomic | recovery only |
| **M3b** sync nothing (`07`'s C as written) | 0 | 70–104 µs **F** | **129 ms F** | + replace-by-rename data forcing | yes, and on every target mount |
| **M2** one ordered write / merged pair | varies | — | — | — | — |
| **M4** journal or intent record (`07`'s D) | 1 per group | — | ~50 ms **H** | — | yes |
| **M5** lean on `data=ordered` | 0 | 70–104 µs **F** | 129 ms **F** | **unestablished** (§3.3) | yes |

**M1 — one `fsync` on whichever file must land first.** The brief's first
candidate. It guarantees D103's order absolutely and with no filesystem
assumption whatever, because `fsync` on `job.json` returns before the append
to `events.jsonl` is issued. Note the direction: it is the **record** that
must land first, and the record is the expensive half, so the saving is the
cheap half. `08` phase S0a puts the event half at 1.21 ms of a 3.57 ms floor
— about **a third of the prize**, leaving the junction still measured in
seconds and post C still essentially the whole window. **Rejected as the
mechanism; kept as the fallback**, and it has one genuine virtue: it is the
only candidate that needs no crash test of any kind.

**M3a — the recommendation.** §0.2. One sync, on the temporary. D103 held by
scope (§1) and not by this sync; `job.json`'s integrity held by this sync and
not by the filesystem (§2). Reversible by a flag. Touches no format, no
`RECORD_VERSION`, no protocol, no consumer-visible file layout.

**M3b — `07`'s C exactly, all three syncs gone.** This is the one `08` priced
and the one ORCH-052 was asked about. It collects the whole prize: 129 ms at
`N` = 1 000, **F**. It is the right destination and the wrong first step,
because it rents §2's protection from `auto_da_alloc` on a mount we are not
allowed to inspect as a standing fact (§3.3 consequence 3). **Deferred, not
refused:** it becomes a second, separately reversible step once M3a has
shipped, the crash-point enumeration is green, and §5.1's figures say what the
last sync is actually worth on the deployment's device. If H-1 holds, that
step may be worth very little and may never be taken.

**M2 — a single ordered write that makes the pair atomic.** Merging
`waiting_for` and its history line into one file, or moving the field out of
`job.json`, makes order unaskable. It also changes the on-disk shape that
`RECORD_VERSION` 1 freezes with a `!=` check and no migration anywhere in the
tree (`store.rs:1143`, `:2335`, `job.rs:27-52`, **F**), and `store.rs:2967`
asserts a job directory holds exactly five names. Taking the field out of
`job.json` is `07`'s option F, refused by D171's own sentence and E-53.
**Refused:** it buys ordering by spending the one thing `07` was careful not
to spend.

**M4 — a journal or intent record ahead of both.** `07`'s D, refused there on
migration and on the silent-rollback hazard. Two new facts make it worse, not
better. The state root is on a **shared** partition (§3.1), so the `syncfs`
group-commit variant `07` kept in reserve would flush a filesystem the
neighbours are using. And §2 says the existing single-point-of-unreadable-state
is already worse than `07` thought — a torn record stops the hub — so adding
a second file that can stop it is the wrong direction. **Refused, restated.**

**M5 — lean on ext4's own semantics.** **Refused, and refused on the grounds
the brief named rather than on the merits:** I could not establish the
cross-file guarantee from documentation on this host (§3.3), and a guarantee
nobody here can cite is a belief. It also fails invariant 9 twice over: it
would make Herd's correctness a property of a mount option on a machine
nobody in this project may inspect, in a product that is shipped and not
operated.

---

## 5. The hypotheses this plan rests on, and how to refute each

### 5.1 Mine

> **H-1 refuted, H-2 refuted, H-3 answered**, all three by
> `11-sync-prices.md`. Left as written, with the outcome marked on each.

**H-1 — REFUTED (`11`) — on the deployment's filesystem class, one `fsync`
costs little enough that M3a collects most of the prize.** It does not:
989 µs, and M3a collects 64.5–66.4 %. `08`'s 8.1–10.8 ms is a SATA ZFS mirror
with no SLOG; the deployment is ext4 on NVMe (§0.1, §3.1). If one `fsync`
there is in the tens-to-low-hundreds of microseconds, M3a's per-job cost is
near `08`'s fully-unsynced 70–104 µs and the last sync is not worth a
decision. **Refuted or confirmed by** extending
`herd-hub/tests/unsynced_write_cost.rs` with two more paths — *temporary-sync
only* and *directory-sync dropped only* — and running all five paths twice,
with `TMPDIR` on ZFS (to tie to `08`'s numbers) and with `TMPDIR` on
`/mnt/ssd_980/...`, which is ext4 on the NVMe. No product code, no new host,
`#[ignore]`d as `08`'s bench already is. **This is the plan's first task and
it is cheap.**

**H-2 — REFUTED in its size (`11`), confirmed in its direction — the
baseline also falls on NVMe, so the *prize* is smaller than 9.84 s.** It
falls to 3.128 s: 3.2x, not the order of magnitude this guessed at. The same measurement answers it, and it is the direction that would
embarrass the programme: if today's junction on ext4-NVMe is 1 s rather than
9.97 s, then the urgency `07 §5.6` already flagged as device-dependent is a
tenth of what the board has been quoting. **It changes no conclusion about the
shape**, as `07` says, and it changes how much of the plan is worth doing
before the `T` walk.

**H-3 — ANSWERED, and the thirds are real (`11 §6`) — the per-sync thirds
are really thirds.** On ext4 the three marginals are 1.05, 1.04 and 0.96 ms
and sum to 3.05 ms against a measured 3.00 ms. The decomposition below was
right in shape and wrong in which third was which: D is 991 µs, not 6.6 µs. My 1.19 / 1.17 / 1.21 ms
decomposition is arithmetic on `08` phase S0a (record half 2.36 ms, append
half 1.21 ms, floor 3.57 ms) against `06` phase 0 (`create`+`write`+`sync_all`
1.19 ms, `sync_dir` 6.6 µs, `append`+`sync_data` 1.20 ms). The two campaigns
disagree about where the record half's second millisecond is: `06` predicted
1.20 ms and `08` measured 2.36 ms end to end, attributing the difference to
the rename over an existing file and a dirty parent directory. **My figure for
M3a is a derived figure, not a measured one, and it is the weakest number in
this piece.** Settled by the same run as H-1, which measures the middle rows
directly instead of subtracting them.

**H-4 — the crash-point enumeration covers the recovery risk.** That building
the post-cut states by construction reaches every state a real cut could leave.
It cannot be proved, only widened; **a `dm-log-writes` campaign in a VM is what
would refute it**, by replaying to a flush point the enumeration did not think
of. That is why tier 4 of §0.3 stays on the list rather than being dropped.

### 5.2 `07`'s, and the measurement for each

| `07`'s claim | status | the measurement |
|---|---|---|
| `H`-1: unsynced `record_waiting` is 50–150 µs | **confirmed**, 70–104 µs (`08`) — on ZFS/SATA only | H-1 above, on ext4-NVMe |
| `H`-2: the decisive tier's burst is small | **never measured**; `06 I`-4 says no bench places or ends a job | a `06`-style cell whose corpus is all `Impossible`, so one pass calls `end_the_job` `N` times. This is the one that could revive M4 and it is the largest unmeasured thing in the programme |
| `H`-3: a thread-aware store-call guard would catch a reactor-path read | undesigned | prototype it; `server.rs:4384` is the specimen |
| `H`-4: the budget paces rather than falls behind | undesigned | V2 at a sustained cause-change rate, not one burst |
| §3 C: "what is given up is only survival of a power cut" | **false as stated** | §2, by reading; confirm in one command by putting a zero-length `job.json` in a job directory and starting a hub |
| §4: "state root forward and backward compatible" | true of M3a and of M3b | no record field changes; assert `RECORD_VERSION` 1 and `PROTOCOL_VERSION` 2 on every branch tip, as C2 and C3 already did |
| §5.5: the unsynced tier widens the stale-temporary window | **true in bytes, false in wall clock** | `durable::names_in` filters every dotfile (`durable.rs:211-221`) and `recover` lists through it, so a stale temporary is invisible to the hub; `killed_hub.rs:423-434` already asserts none is left after twelve `kill -9`s; and an unsynced write is 100 µs rather than 10 ms, so the `kill -9` window **narrows**. What genuinely widens is the power-cut window, and the answer is step 5's sweep |

---

## 6. Adopting `07` — the bounded tasks, in order

Each is one branch, one worktree, one PR, shippable and reversible on its own.
"Owns" names the files that task writes; nothing outside them.

**None of these ten tasks touches `PROTOCOL_VERSION` or `RECORD_VERSION`.**
Both are frozen at 2 and 1 since D73 and checked with `!=` at
`server.rs:2141`, `store.rs:1143` and `store.rs:2335` (**F**), which blocks
the move up and the move back, with no migration code anywhere in the tree.
No task below adds, removes or re-means a record field or a wire message, so
**the freeze is not a decision this plan needs**. Every branch asserts both
numbers, as C2 and C3 did.

| # | task | owns | touches | reversible by | exit |
|---|---|---|---|---|---|
| 1 | **Price the three syncs, on both filesystem classes** (H-1, H-2, H-3). **DONE: `11-sync-prices.md`, ORCH-077, PR #90 — H-1 and H-2 refuted** | `herd-hub/tests/sync_prices.rs`, `11-sync-prices.md` | nothing in `src/` | nothing to revert | delivered: both classes, all eight subsets, control held |
| 2 | **The D103 ordering oracle** | one new `herd-hub/tests/` file | nothing in `src/` | delete the test | green on `main` as it stands: every transition's record `fsync` returns before the event line's first byte |
| 3 | **Crash-point enumeration** | one new `herd-hub/tests/` file | nothing in `src/` | delete the test | a real hub starts and recovers on every constructed post-cut state; the empty-`job.json` case is named with what it does today (§2) |
| 4 | **The stop path** (`07` S1) | `herd-hub/src/main.rs`, `server.rs`, one test | invariant 2's *shape*, not its text | delete the handler | `07`'s V9: `SIGTERM` → clean exit ≤ 5 s, nothing decisive lost |
| 5 | **The lock guards memory, never I/O** (`07` S2) | `herd-hub/src/store.rs` | nothing contractual | per method | `07`'s V5 via `06` phase 1f, plus "no writer blocked > 1 ms by a reader" |
| 6 | **The explanatory tier. SPLIT by the amendment into A (drop E, ~31 %, no crash test), B (drop D, ~65 % cumulative), C (drop R, ~98 %, separate decision)** | `herd-hub/src/durable.rs`, `store.rs` (`record_waiting` only), `docs/coverage.md` E-53 | **D103's scope, from step B on** | `--durable-waits`, default off | A: task 2 green and ORCH-052. B: task 3 green, `07`'s V8. C: its own decision |
| 7 | **A sweep of stale temporaries at `JobDirStore::open`** | `herd-hub/src/store.rs` | nothing | delete the sweep | a planted `.job.json.tmp-*` is gone after a start, and `names_in` still hides one that is not |
| 8 | **The keeper** (`07` S3) | `herd-hub/src/server.rs`, state, ~8 test sites, D100's test | **D100's guarantee moves** from a mutex to a thread | one commit | `07`'s V3, whole suite green, V1 partially |
| 9 | **The bounded unit** (`07` S5) | `herd-hub/src/server.rs` | nothing contractual | budget = infinity | `07`'s V2 and V7 |
| 10 | **The guard, and the decision written** (`07` S6) | `herd-hub/src/durable.rs`, `docs/decisions.md` (by `herd-merger`) | — | the assertion | `07`'s V10, V1 in full, V6 |

**The order, and why.** 1 → 2 → 3 come first and change no product file at
all: they turn the two things this programme has been believing into things it
can observe, and task 1 is the gate on how much of the prize exists. 4 and 5
are independent of everything and of each other, and 4 must precede 6 for
`07`'s reason — a deferred write that an upgrade discards is worse than a
synchronous one, and the consumer upgrades by restarting the container. 6
depends on 1 (the figure), 2 and 3 (the oracles), 4 (the stop path) and on
Alexa's word (§7). 7 rides with or just after 6. 8 → 9 → 10 is a chain and
does not depend on 6 — if the tier is refused, 8, 9 and 10 still stand and
still bound the hub's responsiveness, which is `07 §10`'s fallback.

**Deliberately not in this list**, and not folded in:

- **ORCH-021's `T` walk**, and the `collections_owed` index. Alexa's order of
  2026-10-26 puts them after, as cleanup. They block nothing.
- **M3b**, the last sync. A separate, later, separately reversible step,
  decided on task 1's figures and task 3's oracle, and possibly never worth
  taking (H-1).
- **The `dm-log-writes` VM campaign.** Needed only if task 3 leaves a gap, and
  it needs Alexa's word for a VM on this host.
- **The decisive tier's burst** (`07 H`-2). A measurement, not a change, and
  the largest unmeasured thing in the programme. It belongs beside task 1.

### Where the first consumer would feel it

They are the sole operator and their nightly runs on what we ship (D161,
invariant 9), so this is the column that matters.

- **Tasks 1, 2, 3** — nothing. No product file changes; the benches are
  `#[ignore]`d, as `06` and `08` already are.
- **Task 4** — felt immediately and positively. Today `SIGTERM` kills the hub
  exactly as `SIGKILL` does (`killed_hub.rs:136,141`, **F**), so every
  `docker compose` restart is a `kill -9` mid-pass. After task 4 it is a stop.
- **Task 5** — faster under concurrent junctions, no behaviour change.
- **Task 6** — a faster hub, and one new way to lose an explanation: after a
  **power cut**, a queued job's `waiting_for` may be one cause behind or
  absent, and `events.jsonl` may name a reason the record does not. Both are
  already reachable today by `move_to` and `raise` (§1 F-2). No verdict, no
  exit code, no log and no state transition is affected: those stay in the
  decisive tier with all their syncs. After a `kill -9`, nothing changes at
  all — the page cache is the kernel's.
- **Task 8** — `herdctl submit` may return before its job is judged. That is a
  visible change to the shape of a submission, and the one in this list most
  likely to surprise them.
- **Tasks 7, 9, 10** — internal; 9 may lengthen the tail of a very deep
  queue's first pass while bounding every event's cost.

---

## 7. What needs Alexa's word, and when

Three things, and **the right move now is to ask none of them**, because the
first three tasks need no decision and the first of them is what gives the
questions their numbers. CLAUDE.md asks one question at a time, with a
recommendation; a question asked after task 1 carries a figure instead of an
H-3 estimate.

> **Task 1 has since run — it is `11-sync-prices.md` — so the question below
> can now be asked, and the amendment changes what it asks for: the first
> increment is step A, dropping the event `fdatasync` alone, which needs
> nothing beyond ORCH-052 and no crash test. Read the amendment's "revised
> recommendation" table with the question below.**

**Now that task 1 has run, ask this one, as one question:**

> **Adopt `07`'s "keeper" as the target architecture, in the order of §6, with
> the explanatory tier keeping one `fsync` on the record's bytes — and record
> that D103 binds state transitions, so an `events.jsonl` `waiting` line may
> name a reason `job.json` does not?**
>
> **Recommendation: yes.** The direction is already decided (durable writes
> first, 2026-10-26); what this adds is the shape of the first write change
> and one clarification. The clarification states something the code already
> does in two places (§1 F-2) and costs nothing. The mechanism gives up a wait
> explanation after a power cut — which ORCH-052 granted on 2026-10-26 — and
> keeps `job.json`'s integrity in Herd rather than renting it from a mount
> option (§2, §3.3).
>
> **Fallback if no:** §4 M1 — keep both of the record's syncs, drop the event
> line's. About a third of the prize, no contract change at all, no crash test
> needed, and tasks 4, 5, 8, 9 and 10 still stand.
>
> **Amended:** `11` makes that fallback the **first step** rather than the
> consolation prize — it is worth ~31 %, the same as every other sync, and it
> is the only one that costs nothing in contract, in filesystem assumption or
> in crash evidence. So the question to put is: adopt the keeper, and take the
> tier in the amendment's three steps, A now and B behind the enumeration.

**Held back, to be asked only if it becomes live:**

1. **Dropping the last `fsync` (M3b).** A separate decision, after task 1 says
   what it is worth and task 3 is green. May never be asked.
2. **A VM on this host for the `dm-log-writes` campaign.** Only if task 3
   leaves a gap. Recommendation then: yes, in a guest, never on this host's
   block devices.

**Not Alexa's, and named so nobody waits on him:** the dossier number (§0.1,
the orchestrator's); which agent takes which task; the `--durable-waits` flag's
name; and the `RECORD_VERSION` / `PROTOCOL_VERSION` freeze, which §6 does not
touch.

---

## 8. What I would not do

The brief asked for this plainly, so: plainly.

1. **I would not drop all three syncs as the first step**, which is what `07`'s
   S4 and `08`'s measurement together read as an instruction to do. §2 is the
   reason, and it is a reading of the code, not a preference.
2. **I would not invent an ordering mechanism for the explanatory pair.** §1
   says there is nothing to hold. Building a journal, an intent record or a
   merged write to protect an invariant that does not bind these writes would
   be the costliest kind of mistake this dossier can make: real work, real
   migration risk, to defend a property nobody needed.
3. **I would not put a shipped guarantee on an ext4 mount option** (§3.3), and
   I would not write down the cross-file ordering argument as established when
   the documentation on this host does not establish it. On that specific
   point ORCH-074's framing — that `08`'s ZFS argument "does not transfer" —
   is right, and the fix is to stop needing a filesystem argument at all
   rather than to find a better one.
4. **I would not spend a VM and a `dm-log-writes` harness before the two tests
   that need neither.** Tiers 1 and 2 of §0.3 are cheap, deterministic and
   cover the ordering guarantee and the recovery risk respectively. Tier 4
   samples; tier 2 enumerates.
5. **I would not keep quoting 9.97 s as the junction** until H-2 is answered.
   It is a SATA ZFS figure for an ext4 NVMe deployment, and the board, `06`,
   `07`, `08` and `status.md` all rest on it. It may be right about the shape
   and an order of magnitude wrong about the size. **Answered (`11 §8`): the
   figure for the deployment's class is 3.128 s, so the correction is 3.2x
   and not an order of magnitude — and the problem does not go away, because
   three seconds in one pass is still the dominant cost of that pass.**
6. **Where I think the keeper is right.** I do not think `07` picked the wrong
   shape. Its argument — that the term has to go, that between C and D the
   sound one is C, that the term is not the whole defect, and that the bound is
   the architecture — survives everything above. What changes is the **first
   increment**: one sync instead of three, with the integrity leg bought in
   Herd. That is a smaller change that collects most of the prize, which is
   what the brief asked me to say if I found one. **Revised by the amendment:**
   "most of the prize" was H-1's claim and H-1 is refuted. One sync instead of
   three collects 64.5–66.4 %, and the first increment is smaller still —
   drop the event `fdatasync` alone, ~31 %, which needs no crash test and no
   filesystem assumption at all. `07`'s shape is unaffected either way.

---

## 9. F, H, I

**F — read at a named `file:line`, or from a named host interface.**

1. `JobEvent::state()` is `None` for `Waiting`; `Transition` is a distinct
   variant. `job.rs:1495-1506`, `:1517-1600`.
2. `move_to` clears `waiting_for` and keeps the `waiting` lines
   (`store.rs:1322-1325`); `raise` does the same (`store.rs:2817-2820`).
3. `recover` reads `job.json` only, never `events.jsonl`
   (`store.rs:860-899`); `EVENTS_FILE`'s four sites are an append, a query and
   two tests.
4. `job_events` discards an unterminated last line (`store.rs:2307-2325`).
5. Transition order is line order, never a timestamp comparison
   (`job.rs:58-61`).
6. `move_to` and `record_waiting` both write the record before the event
   (`store.rs:1345-1348`, `:2031-2035`), and `write_file_with` returns only
   after `sync_all` and `sync_dir` (`durable.rs:35-56`) — so the event append
   is not issued until the record is durable.
7. A `job.json` that will not parse makes the hub refuse to start on the whole
   state root: `read_stored` `?` (`store.rs:1141-1142`) → `recover` `?`
   (`:896`) → `open` `?` (`:436`), stated in `job.rs:27-32`. D52's quarantine
   is labels only (`store.rs:1150-1157`).
8. `recover` reads the *pending* records when the index is trusted
   (`store.rs:883-899`), and `record_waiting` writes only to a `Queued`, hence
   pending, job (`store.rs:2024`).
9. `durable::names_in` filters every dotfile (`durable.rs:211-221`), so a
   stale temporary is invisible to `recover` and to the five-names assertion
   (`store.rs:2967`); `killed_hub.rs:423-434` asserts none is left after
   twelve `kill -9`s.
10. `RECORD_VERSION` 1 and `PROTOCOL_VERSION` 2, `!=`, no migration code:
    `store.rs:1143`, `:2335`, `server.rs:2141`, `job.rs:27-52`.
11. `SIGTERM` and `SIGKILL` go into the same helper
    (`killed_hub.rs:136,141`); the hub has no signal handling.
12. **Host.** `/mnt/ssd_980` is `ext4 rw,relatime,stripe=32` on
    `/dev/nvme0n1p1`, a Samsung SSD 980 1TB over `nvme`
    (`/proc/mounts`, `lsblk`). Effective options
    (`/proc/fs/ext4/nvme0n1p1/options`): `data=ordered barrier auto_da_alloc
    journal_checksum delalloc commit=5 errors=continue dioread_nolock`. The
    same partition carries `/var/lib/libvirt/images` and
    `/var/lib/chatons-leash`, so it is **not dedicated**. `rpool` — `08`'s and
    `06`'s host — is a mirror of two SATA Toshiba SSDs, no SLOG
    (`zpool status rpool`).
13. **Documentation.** `man 5 ext4` (`e2fsprogs-1.47.4`) says metadata is
    always journaled; `data=ordered` forces data out before its metadata
    commits; `auto_da_alloc` forces a replace-via-rename's data out before the
    rename commits, avoiding the zero-length problem; `barrier` enforces
    on-disk ordering of journal commits; `commit=5`.
14. `dm-log-writes` exists as a module on this host (`modinfo`), `libvirt` is
    installed, and neither has been loaded or used by this piece.
15. `09-runner-self-limits.md` is on `origin/c5-announced-job-ceiling`
    (`git ls-tree`), so `09` is taken.

**H — a hypothesis, with the measurement.** H-1 to H-4 in §5.1, of which
**H-1 and H-2 are refuted and H-3 answered** by `11-sync-prices.md`; `07`'s five
in §5.2.

**I — could not be decided here.**

1. **Whether ext4 orders two successive metadata operations on two files
   across a cut.** §3.3. No documentation on this host settles it, and this
   plan is built so as not to need it.
2. **Whether the hub's state root on the NixOS server is this workstation's
   `/mnt/ssd_980`.** The evidence is strong — the first consumer reported
   `/mnt/ssd_980/docker/volumes/herd_herd-state/_data` as "ext4 on NVMe", and
   `findmnt -T /mnt/ssd_980/docker` says `/mnt/ssd_980 ext4` — but invariant 9
   means a fact about their machines reaches us as a report, not as something
   we looked at, and `ls` on that volume returned `Permission denied`. **If it
   is the same machine, task 1's ext4 cell measures the deployment directly.**
   Worth one sentence from Alexa; worth no agent's `ls`.
3. **The size of the prize on the deployment's device.** H-1 and H-2. Still
   `08`'s `I`-3, still open, now cheap.
4. **The decisive tier's burst.** `07 H`-2, `06 I`-4. Unmeasured.
5. **Whether the device honours a flush.** §0.3. No test we can run settles
   it; the mount's `barrier` and the drive's specification are the stand-in.

---

## 10. How to reproduce what is in this piece

Nothing here needs a build. The host readings, in full:

```sh
findmnt -T /mnt/ssd_980 -o TARGET,SOURCE,FSTYPE,OPTIONS
cat /proc/fs/ext4/nvme0n1p1/options
lsblk -o NAME,SIZE,ROTA,TRAN,MODEL
zpool status rpool
man 5 ext4 | col -b | sed -n '393,435p;540,556p'
modinfo dm-log-writes | head -3
```

The code readings are `file:line` on `origin/main` at `96fc10b`. The
numbering collision:

```sh
git ls-tree --name-only origin/c5-announced-job-ceiling -- docs/remediation/
```
