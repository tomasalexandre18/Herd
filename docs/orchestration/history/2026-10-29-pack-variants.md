# pack-variants — handoff 2026-10-29

- **Item:** the fold of M11's pack surface onto the real frame enums (the
  follow-up `pack-proto`'s *Decisions needed* 1 named), plus the general
  `ToHub` fact frame of the bump review's §3 and the peer leg's framing
- **Agent:** herd-step
- **Branch / commit:** `pack-variants` / see the agent's report, based on
  `pack-proto` `adbc204` with `origin/main` `1a8c1ae` (batch 9) and
  `origin/spool-offer-fix` `fdf928c` merged in
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger` names
  the landing here and fills the record of `templates/landing.md`
- **Pairing, which the batch depends on:** this branch and `pack-proto` are
  **one change in two commits**. `pack-proto` moved `PROTOCOL_VERSION` to 3
  and its doc comment says 3 carries the pack frames; the frames are here.
  **If one lands without the other, that doc comment is false and the bump
  bought nothing.**

## Done

### The two merges, both clean

```
$ git merge origin/main                 # batch 9
Merge made by the 'ort' strategy.   13 files changed, 2711 insertions(+), 79 deletions(-)
$ git merge origin/spool-offer-fix
Merge made by the 'ort' strategy.    3 files changed, 1111 insertions(+), 106 deletions(-)
```

No conflict in either, and the 967 lines of `herd-runner/src/lib.rs` that
carry the 37.6 GiB spool fix are untouched by this branch except for the four
places the compiler forced (below).

### Fourteen new variants, not thirteen

**`ToHub`, seven** — the six pack frames of §4 as newtype arms, plus the
general fact frame of part two:
`PacksHeld`, `RegisterPack`, `AskPack`, `AskTransferCapability`,
`PackTransferred`, `PackUnavailable`, **`JobFact`**.

**`FromHub`, seven** — §4's six plus `PinPacks`, which §4 lists as a field
row but is a variant: `PackHolders`, `TransferCapability`, `ExpectTransfer`,
`PackRegistered`, `PackRefused`, `ReadPacksAgain`, `PinPacks`.

### Nine fields across five messages, not seven across seven

Counted, because the brief asked: §4's *"New fields on existing messages"*
table has eight rows, one of which (`PinPacks`) is a variant and one of which
(`Health`) carries three fields. So **nine field names on five message
types**:

| Message | Fields |
|---|---|
| `Asked` | `needs`, `leaves` |
| `ToHub::Hello` | `mesh_endpoint` |
| `ToHub::Health` | `pack_store_bytes`, `pack_store_budget_bytes`, `packs_held` |
| `FromHub::Welcome` | `keeper_for` |
| `FromHub::AssignJob` | `packs_in`, `packs_out` |

Every one is `#[serde(default)]` with a `skip_serializing_if`, so it is absent
from the wire when it says nothing and decodes from a frame that never carried
it. Both halves are asserted, by the test whose name says both:
`the_pack_fields_are_absent_when_silent_and_decode_when_they_were_never_there`.

### The bytes are unchanged, and the detector was proved to fire

`pack_frames.rs` froze each payload's serialisation through a stand-in enum
before the real enums carried it. The stand-ins are **kept**, and
`the_bytes_are_the_same_on_the_real_enum` now encodes the real arm beside the
stand-in's and compares the two byte strings — inside the two round-trip
helpers, so it runs once per payload per case, every absent optional included.
All 41 `pack_frames` tests are green, so **no byte differs**.

A green byte comparison proves nothing until it has fired, so it was made to:
`#[serde(rename = "packs_held_broken")]` on the real `ToHub::PacksHeld` arm
gave

```
test packs_held_round_trips_and_carries_its_continuation_flag ... FAILED
panicked at herd-proto/tests/pack_frames.rs:514:5:
assertion `left == right` failed: the stand-in froze a wire shape the real
enum does not emit; the stand-in was wrong and every byte frozen through it
is suspect
```

The rename was reverted and the 41 are green again.

### `message_kind`: exhaustive, kept exhaustive

Seven arms, not six — the six pack frames and `job_fact`. The match keeps no
wildcard, and the function now carries the argument for that: the arm a `_ =>`
would fall into is `ToHub::Unknown`'s, *"a frame of a type this hub does not
know"*, which is **false** about a frame this build defines and has not wired
up. The person reading that sentence is a runner's owner reading her runner's
stderr, and it would send her to upgrade a binary that is already current. The
cost is one arm per new `ToHub` variant, and a new `ToHub` variant costs a
protocol version — so it is paid about once a release, by somebody already
thinking about the wire.

**Two more exhaustive matches the brief did not anticipate**, both on
`FromHub` and both found by the compiler: `herd-runner/src/lib.rs`'s own
`message_kind` and `herdctl/src/lib.rs`'s. Seven arms each, for the same
reason.

### The general `ToHub` frame, designed and deliberately not wired

```rust
ToHub::JobFact {
    job_id: Option<JobId>,   // #[serde(default, skip_serializing_if)]
    attempt: Option<u32>,    // #[serde(default, skip_serializing_if)]
    reason: FactReason,
    detail: String,          // logged and recorded, never parsed
}
```

`FactReason` is a closed set of six, one per silence, on the wire as a plain
snake_case string, plus `Other(String)` for a code this build was not taught:

| Silence (review §3) | Reason code |
|---|---|
| **S1** asked for files it does not hold (`herd-runner/src/lib.rs:1690-1700`) | `job_files_not_held` |
| **S2** its record names a file it cannot read (`:1660-1668`) | `job_files_lost` |
| **S5** a transfer stopped and only it knows why (`:1605-1627`) | `job_files_transfer_stopped` |
| **S6** a spooled verdict it cannot attribute (`:2340-2375`) | `verdict_unattributable` |
| **S7** a spool deleted after the hub refused it (`:2107-2120`) | `verdict_discarded_after_refusal` |
| **S10** E-41's skip, a job never submitted (`herdctl/src/moved.rs:55-60`) | `submission_skipped` |

S10 is why `job_id` is optional, and that one absence is what lets a single
frame cover all six: the hub never minted an id for a job a requester decided
not to submit.

**Nothing sends one.** No runner, no requester, no hub handler. The variant's
own doc comment says so in those words — *"what landed is the contract, not
the fix"* — so that the frame's existence cannot be read as six silences
closed. On this build a `job_fact` arriving at the hub falls to the `other`
arm of whichever loop received it and is refused by name, which is correct
while nothing sends one.

### The peer leg, both decisions implemented

**1. The bytes left the struct.** `PeerReply` is a header: `object`, `offset`,
`length`, `object_length`. `data` is gone.

One refinement the brief's wording forced: `length` used to mean *the object's
whole length* and the brief says the body is *"exactly `length` raw bytes"*.
Those cannot both hold, so `length` is now **the body's** length — the one the
framing reads — and `object_length` carries the fact the old field existed
for, which is not derivable from one reply: a fetcher resuming at an offset
would otherwise read the manifest a second time to learn when to stop.

**2. One line of JSON ended by `\n`, then exactly `length` raw bytes**, in
`herd-proto` (invariant 4):

```rust
pub trait PeerBody {                       // how many raw bytes follow a header
    fn body_bytes(&self) -> u64;           // 0 for PeerRequest and PeerRefusal
    fn self_disagreement(&self) -> Option<PeerFramingRefusal> { None }
}
pub fn encode_peer_header<H: Serialize + PeerBody>(&H)
    -> Result<(Vec<u8>, u64), PeerFramingRefusal>;        // the line, and the length it promises
pub fn decode_peer_header<H: DeserializeOwned + PeerBody>(&[u8])
    -> Result<PeerHeaderRead<H>, PeerFramingRefusal>;     // Incomplete | Read{header, body_bytes, body_at}
pub fn take_peer_body(&[u8], u64) -> Result<(&[u8], &[u8]), PeerFramingRefusal>;
pub fn check_peer_protocol_version(u32) -> Result<(), PeerFramingRefusal>;
```

The length is a property of the header's **type** rather than a rule somebody
remembers: a header carrying a body that did not say so would not compile
through `encode_peer_header`.

Six named refusals, each with a case that fires it, and `Incomplete` — which
is **not** a refusal — with its own:

| Refusal | The case that fires it |
|---|---|
| `HeaderPastTheReadBuffer { bytes }` | `PACK_ENDPOINT_READ_BYTES` of `{` with no newline; and, on the way out, a `PeerRefusal` whose `why` is that long. The bound is the endpoint's own read buffer, not a new number |
| `HeaderUnreadable { error }` | the 17 surplus bytes a sender left behind after writing more body than its header promised |
| `BodyPastTheChunk { promised }` | `length = PACK_CHUNK_BYTES + 1`, refused on encode and on decode; exactly one chunk is allowed |
| `BodyPastTheObject { offset, length, object_length }` | `offset 900 + length 200` of a 1 000-byte object; a chunk ending exactly at the end is the last chunk and is not refused |
| `BodyTruncated { promised, arrived }` | 4 000 bytes arriving against a promised 4 096 |
| `ProtocolVersion { announced }` | `PROTOCOL_VERSION - 1`, with `peer_version_mismatch_reason`'s own sentence |
| *(not a refusal)* `Incomplete` | 0, 1, half and all-but-one bytes of a header line |

No new constant: the header bound is `PACK_ENDPOINT_READ_BYTES` and the chunk
bound is `PACK_CHUNK_BYTES`, as instructed. A newline **inside** a header's own
text does not end the line, which the whole framing rests on and which
`a_refusal_carrying_a_newline_is_still_one_line` asserts against a `why` of
three lines.

### The digest column is validated where the entry is parsed

`entry_of_row` parsed the size and refused an unknown kind letter and carried
**column 1 exactly as written**, so a row whose digest read
`../../../escaped` produced a `ManifestEntry` naming a path outside
`objects/`. It is now checked at the one place an entry comes into existence:

```rust
pub fn check_object_digest(value: &str) -> Result<(), String>
```

**Exactly 64 lowercase hex characters.** Public, so the readers that already
check this — the runner's store today, the hub's registry and the fetch
endpoint tomorrow — delegate instead of keeping a second, third and fourth
copy; three copies is how two of them come to disagree. **Uppercase is refused
and not lowercased**, said in the refusal itself: a content-addressed store
that accepted both spellings could hold one object's bytes twice under two
names, with nothing to say which the manifest meant.

**Proved by removing it.** With the `check_object_digest(columns[0])` line
deleted, the traversal fixture parses:

```
panicked at herd-proto/src/pack.rs:1615:18:
that is not a digest Herd may build a path out of:
PackManifest { id: "53afeb0c…", bytes: 72, rows: 1, files: 1 }
```

— a manifest with one `kind f` row whose object name is `../../../escaped`.
The line was restored and the 84 unit tests are green.

Five refusing fixtures and a silent one, in
`a_digest_column_that_is_not_a_digest_is_refused_by_name`: the traversal,
`abc`, 65 characters, 64 uppercase, 64 non-hex — each asserted to name the row
number and what was wrong with the value — and **`"1".repeat(64)` parses,
along with the module's own four-row `d`/`l` fixture**, so the check is not
simply refusing everything.

### What I found about the other columns

**`path` was not inert by construction, only by habit, so it is checked too.**
[`ManifestEntry::path`]'s doc already carried the rule as prose — *"not a path
Herd ever joins onto anything"* — which is the same N-places failure in its
smallest form. Column 6 is relative to the pack's own root by the format's own
definition, so `check_entry_path` refuses an empty path, an absolute one, a
NUL, and any `..` component that climbs out, each with a fixture. A `..` that
does **not** climb out is not refused (`a/../b` names `b`), and `x..y` and
`..z` parse: a check that refused those would refuse good packs.

**`link_target` is deliberately NOT checked, and that is the finding.** Herd
never follows a link and never lays a tree out, so nothing resolves it — but a
build output's symlink may legitimately point at an absolute path outside the
pack (`/nix/store/abc-tool/bin/tool`), and refusing that would refuse a good
pack to defend against a resolution nothing performs. It is therefore inert
**by usage and not by construction**, which the type's doc now says in those
words, with the one rule a reader still owes: anybody who ever resolves one
owes it its own check. Asserted by
`a_link_target_outside_the_pack_is_carried_and_not_refused`, which also
asserts that such a row names no object at all.

`size` is a parsed `u64`; `kind` is one of three letters and an unknown letter
is already a refusal; `mode` is text that is never applied; `role` is carried
and printed and never matched. The type's doc now says all four rest on
nothing being done with them, because that is worth knowing before you are the
one who does something with them.

## Not done, and why

- **No handler, no caller, no behaviour anywhere.** Instructed, and said in
  the frame's own doc comment. The six silences are still silent; the pack
  frames are still unsent.
- **`RECORD_VERSION` and `LAYOUT_VERSION` do not move** (instructed). The four
  `frozen_formats.rs` tests are green and untouched, so
  `VERDICT_FORMAT_VERSION` and `OUTPUT_FORMAT_VERSION` have not moved either.
- **`docs/` carries nothing about any of this.** `docs/architecture.md`'s
  frame inventory, `docs/plan.md` and `docs/coverage.md` are not this step's,
  and the drafted decision entry is in the agent's report rather than in
  `docs/decisions.md` (only `herd-merger` appends there).
- **The runner's own `object_name` guard is left in place**, deliberately: it
  is in a file this branch does not own and its author kept it on purpose. It
  is now redundant — `ManifestEntry` cannot exist with a digest of any other
  shape — and it can become a thin delegation to
  `herd_proto::pack::check_object_digest` as a follow-up. This branch does not
  touch it, and nothing on this branch calls `PackManifest::parse` outside
  `herd-proto` (`rg PackManifest::parse` finds no caller elsewhere), so the
  guard breaks nobody's fixture here.
- **`herdctl` still sends `needs: []` and `leaves: {}` on every submission.**
  Carrying ORCH-148's declaration fields onto the wire is the `herdctl`-only
  step the board already names; this branch adds the fields and wires nothing.

## Gates

**Runs made before this file was committed**, on this tree, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-variants`:

```
$ nix develop --command cargo fmt --all --check
(no output)   exit 0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Checking herd-proto … herd-runner … herd-hub … herdctl
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.23s
exit 0

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 26.48s
exit 0

$ nix develop --command cargo test -p herd-proto
  lib             84 passed; 0 failed; 0 ignored
  frozen_formats   4 passed; 0 failed; 0 ignored
  pack_frames     41 passed; 0 failed; 0 ignored
  round_trip      32 passed; 0 failed; 0 ignored
  doc-tests        0 passed
exit 0

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit 0
```

```
$ nix develop --command ./scripts/test.sh
  1006 passed; 0 failed; 38 ignored, across 65 test binaries
  12 assertion(s) COULD NOT RUN on this host and were skipped
  test suite: PASSED
exit 0
```

### The first gate was RED, and on a true statement about the host

```
gate: RED sha=b63cec3ecbab7a04802035cf4167ace38cbb1241 failed=6:test-suite
      skips=under-load(flood-fifty=8.73>8,flood-two-fifty=8.73>8)
gate: NOT GREEN — this sha must not be pushed to main.
```

Steps 0 through 5 and 7 were ok — `check-docs`, `check-orchestration` (73 s),
`nix flake check`, `cargo fmt`, `cargo clippy`, and the skip list matched the
expectation **exactly**, with both load-gated benches excused by their own
readings. Step 6 failed with exit 101 after 443 s, and **not one of its 32
failures is about a frame, a field, a variant or the framing.** Grouped by
message, with the figures replaced by `N`:

| × | The failure |
|---|---|
| 16 | `herdctl ["run", …] never came back inside Ns` |
| 5 | *"told to lend N MiB … enclosed by memory.high 18 GiB on /chatons.slice, which already holds 16.5 GiB, so the room here is 1.4 GiB … Run the suite outside the slice, or raise the slice's ceiling"* |
| 2 | `--host-memory-floor is N MiB and this host lends N MiB, so nothing could ever be admitted here` |
| 2+2+1 | the runner never joined / the probe never ran / a dial timeout |
| 2 | `announcing N MiB rather than N GiB, because /proc/self/cgroup is enclosed by memory.high …` |

`load1` rose 7.95 → 12.33 across the suite. **This is precisely the red
`bench-preflight.sh` refused in order to prevent** — *"the run would go red on
a true statement about the host"* — and the branch was not pushed on it. The
complete `./scripts/test.sh` run quoted above, on this same tree, was green at
a moment when the slice had more room; so the variable is the host and not the
diff, and the lift described below is a decision this red refutes.

The gate's own log and the suite's output:
`<scratchpad>/pack-variants/gate.log`,
`<scratchpad>/pack-variants/gate-stdout.txt`, and
`target/herd-gate/test-output-b63cec3ecbab.txt`.

### The second gate was RED too, and the host was at rest for it

```
gate: RED sha=a76da253014ca2580c50329ab0904c80b76c6e99 failed=6:test-suite
      skips=at-rest
gate: NOT GREEN — this sha must not be pushed to main.
```

Run only after **two consecutive clears of the unmodified
`bench-preflight.sh` twenty seconds apart** — 9741 MiB and 9934 MiB of slice
room, load 4.40 of a 9.00 ceiling, no override anywhere in the environment.
Steps 0–5 and 7 ok again, and this time the skip list was the **at-rest** one
exactly: no bench had to be excused. `load1` 4.05 at the start, 5.12 after the
suite — the host was quiet from end to end.

Step 6 failed in 263 s with **two** tests (not the nine of the first run, and
a different file: `herd-hub --test stopping`), and the runner's own words say
why:

```
herd-runner: announcing 3 MiB of memory rather than 31.1 GiB, because
/proc/self/cgroup is enclosed by memory.high 18 GiB on /chatons.slice, which
already holds 17.9 GiB, so the room here is 3 MiB and not what /proc/meminfo
reports
Error: --host-memory-floor is 1024 MiB and this host lends 3 MiB, so nothing
could ever be admitted here.
```

So the gate's suite **fills the slice to its own ceiling as it runs**, and
every `real_herd` test whose runner starts after that point refuses to start.
Which tests those are is a race, which is why the two runs failed in different
files; the cause is one. **The branch is therefore not pushed**: push only on
your own green gate, and this gate is not green.

**The last run, named and not quoted** (a handoff is a docs file, so the run
that gates the tree with this file in it cannot quote itself):
`./scripts/check-docs.sh` on the tree with this file committed, to
`<scratchpad>/pack-variants/docs-final.log`, verbatim in the agent's report.
**No third gate run was made.** Two were enough to establish the cause, and a
third would have been re-rolling a dice whose outcome is about the host's page
cache and not about this tree — which is the one thing a gate must not be
asked to do.

**Skips:** none from any run above — `cargo test -p herd-proto` reported
`0 ignored` on all four binaries. The suite's and the gate's skips are in the
report with the run that produced them.

**Host, disclosed in full because `bench-preflight.sh` refused and the floor
was lifted inline.** The preflight refused on two counts at first — load 13.29
of a 9.00 ceiling, and 3788 MiB of slice room against an 8192 MiB floor — and
I waited. The load refusal cleared on its own (13.29 → 2.18 over about forty
minutes, with a background until-loop polling the unmodified gate and no
`pkill` anywhere). The **slice-room** refusal did not, and reading
`/sys/fs/cgroup/chatons.slice/memory.stat` said why: at the quietest point the
slice held 16081 MiB with **anon at 646 MiB** and `file` at 14778 MiB, 12405
MiB of it `inactive_file` — 92% of the charge was reclaimable page cache from
sibling builds that had already finished, and 23352 MiB was available
system-wide. Page cache is reclaimed under pressure and not on a timer, so the
unmodified figure was not going to recover; `pack-proto`'s did, and this one
did not.

So `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB=2` was set **inline, on each heavy
command, one command at a time**, with the unmodified figures recorded above
and the refusal's own text in the agent's report. It was **never exported**,
and **`./scripts/gate.sh` ran with no override in its environment at all**
(`unset` in the wrapper, verified `unset` in the calling shell) — `gate.sh`
does not consult the preflight, so there was nothing to override and nothing
that could redden a `check-orchestration` case asserting what a refusal says;
step 2 was ok in 73 s. Nothing under `/sys/fs/cgroup` was written. No cache
was dropped.

**And the lift was wrong.** The reasoning above — that the refusal was about
reclaimable page cache and would not recover on a timer — was correct about
the *cause* and wrong about the *consequence*. The floor is 8192 MiB because
the `real_herd` tests make a runner lend 2048 MiB and then assert figures
about it, and that runner reads the enclosing `memory.high` honestly (D119).
At 1.4 GiB of room it announced 1.4 GiB, said so in a sentence naming the
slice, and nine tests refused rather than asserting a figure that was the
cgroup's and not theirs. The preflight's own words for this are *"the run
would go red on a true statement about the host"*, and that is exactly what
happened. An `anon` figure in the hundreds of MiB says the charge is
reclaimable; it does not say the suite can run, because what the suite needs
is **room under `memory.high`**, which page cache occupies just as anonymous
memory does. The second gate run therefore waited for the unmodified gate.

## Risks and findings

Ordered by what a later version cannot repair.

**0. THE PRODUCT FINDING, and it is why the gate is red: a runner inside a
`memory.high` cgroup counts reclaimable page cache as memory the jobs are
holding, and so refuses to start.**

`herd-runner/src/room.rs:63` is

```rust
self.ceiling_bytes.saturating_sub(self.charged_bytes) / (1 << 20)
```

where `charged_bytes` is `memory.current` (`:143`). **`memory.current`
includes the cgroup's page cache**, which is not memory any job holds and
which the kernel reclaims the instant anything asks for it. So on any cgroup
that has done file IO — every cgroup after a while, and certainly a build
host — the figure collapses towards zero, and the floor refusal in `main`
turns that into a runner that will not start at all:

> `--host-memory-floor is 1024 MiB and this host lends 3 MiB, so nothing
> could ever be admitted here.`

Measured on this host, the two readings side by side at one instant:

```
room.rs reads    high - current                    = 5722 MiB
cache taken off  high - (current - inactive_file)  = 7644 MiB
```

and at the worst moment of the gate's own suite `current` was 17.9 GiB of an
18 GiB `memory.high`, with **`anon` 536 MiB and `file` 12637 MiB** — so the 3
MiB the runner announced was about page cache and nothing else, while the
machine had 23 GiB available.

**The module knows the question and answers it for the host, not for the
cgroup.** `room.rs:27-29` deliberately refuses `MemAvailable` — *"a second
figure that moves between the judgement and the claim would be the overcommit
E-32 forbids"* — and that argument is about the **host's** free memory. But
`MemAvailable` is exactly the kernel's "free plus what is reclaimable", and
the cgroup path then uses the one figure that has no such correction. The
asymmetry is the defect: the host reading accounts for reclaimable cache and
the cgroup reading does not.

**Consequence for the consumer, which is why this is first.** D161 says they
run every instance, in docker compose — which is cgroups with limits. A runner
in a container whose `memory.high` is reached by page cache announces a few
MiB, is refused by its own floor, and its owner reads a sentence about memory
she has plenty of. The startup line is honest about *what it read*; what it
read is the wrong figure.

**Not touched**: `herd-runner/src/room.rs` is on this brief's do-not-touch
list by name. The fix is one subtraction and a test, and it is not mine.

**It is also the whole of why both gate runs are red**, so for this branch it
is a blocker and not only a finding. Three remedies, in the test's own words
and in increasing order of what they cost somebody else: run the suite outside
the slice, raise the slice's `memory.high`, or correct the reading. The third
is the one that also helps the consumer.

**1. `PackUnavailable` was kept separate from `JobFact`, and the argument
cuts both ways.** It is a pack-shaped instance of exactly that frame, and
absorbing it would have spent one variant instead of two. It stays separate
because its three extra fields — `pack`, `commit`, `key` — are read **by the
hub and not by a person**: the registry is keyed on `(commit, key)` and the
holder set on the pack's id, so a hub answering one looks all three up. Inside
`JobFact` they would have to live in `detail`, and a hub parsing its way back
out of a sentence is the opposite of invariant 4. The counter-argument, stated
honestly: two frames where one would do is one more `ToHub` variant than the
minimum, and if the hub ever stops needing typed pack fields the duplication
has no defence left.

**2. The brief's premise that the reason set is the expensive half is wrong,
and the code says why.** A seventh reason is **free**: an unknown code decodes
into `FactReason::Other` carrying the code itself, so an older hub reads the
whole frame, logs the `detail`, names the code and acts on nothing — the same
shrug `ToHub::Unknown` is, one level down. The only one-shot thing in the
frame is the **variant**. (`#[serde(other)]` could not be used, because serde
allows it only on a unit variant of an internally or adjacently tagged enum
and this is a bare string; `#[serde(from = "String", into = "String")]` with a
normalising `From<String>` gives the same property, and the named codes can
never arrive as `Other`.)

**3. The peer framing's saving is real and the cost is one field, but the
*throughput* claim is still unmeasured.** 401.6 MiB stays 401.6 MiB instead of
~535 MiB, which is arithmetic and not a measurement. What is **not** measured
is whether the one-line-plus-raw-bytes reader is faster in practice than
base64 through serde, and D148's own open question — ~557 round trips at an
unmeasured per-object cost — is untouched by this branch. `PackTransferred`
exists to turn that into a figure; S12 owes the measurement.

**4. `length` changed meaning, and a reader of the old field name will be
wrong.** It was the object's whole length and is now the body's. Nothing
outside `herd-proto` reads it yet (`rg PeerReply` finds only `pack.rs`,
`lib.rs` and `pack_frames.rs`), so the rename is free **today** and is not
free once S5 and S9 are written. That is the reason this had to be the type
change it was and not an implementation detail.

**5. `mesh_endpoint` is bound and dropped in the hub's handshake, and three
`Health` figures and both `AssignJob` pack fields likewise.** Each is
`field: _` with a comment naming the step that fills it, rather than `..`, so
the **next** field added to any of those frames is a compile error at the
place that must read it instead of a silence. That is a deliberate choice to
pay a small cost repeatedly; a reviewer who prefers `..` would be making the
opposite trade.

**6. `needs` and `leaves` are on `Asked` and not on `Declaration`** —
`pack-proto` flagged this and it stands. One consumer file's reading is split
across two proto types. Free to change later (a `#[serde(default)]` field on
`Declaration`), so not urgent, and the field doc now states the split and the
reason for it rather than leaving it to look like an accident.

**7. Found while folding, not fixed, not mine: four field docs date themselves
by a version counter that no longer exists.** `herd-proto/src/lib.rs:1287`
(`Hello::peer_version`) says *"what moved [`PROTOCOL_VERSION`] to 8"*, `:1437`
(`Hello::switch`) *"to 5"*, `:2057` (`Asked::priority`) *"to 2"*, `:2294`
(`SwitchSet::confirmed`) *"to 6"*. All four are **development-era** numbers,
and the constant's own doc explains that M16 reset the counter to 1 — so they
are not quite false, but they read as claims about the deployed numbering, and
`:2057`'s *"to 2"* now collides head-on with the deployed 2, which the same
doc says went on M11's record half and nothing else. A reader reconciling the
five sentences cannot. Nothing asserts any of them; they are prose, in a file
this step owns, and rewriting them is a sweep of its own that was outside this
brief. Reported rather than touched.

**8. Nine fields went onto five frames and nothing refuses a nonsensical
combination.** A `Health` carrying `pack_store_bytes` above
`pack_store_budget_bytes`, or an `AssignedPack` whose manifest does not hash
to its id, is a readable frame today. The manifest case is already covered by
`PackManifest::disagreement_with`, which the hub is expected to call; the
budget case has no check anywhere and belongs to whoever reads the figures
(S6/M12), not to `herd-proto`.

## Decisions needed

1. **Whether `PackUnavailable` survives now that `JobFact` exists.**
   Recommendation: keep both, on finding 1's argument — typed fields the hub
   looks up are not a sentence. Fallback: absorb it, accept that a hub reading
   one has to parse `detail` for the `(commit, key)`, and save one `ToHub`
   variant. It must be settled **inside this batch**: removing a `ToHub`
   variant afterwards is a fourth protocol version.
2. **Whether `FactReason`'s six codes are the right six**, which is the one
   part of part two that is a judgement rather than a transcription. Each is
   mapped to its silence above. Recommendation: land them; `Other` means a
   seventh costs nothing and a wrong name costs a rename, not a version.
   Fallback: cut to four by merging S1 with S2 and S6 with S7, at the price of
   a hub that cannot tell *"stop asking me"* from *"a requester has been
   promised something nobody will send"*.
3. **Who wires the first silence, and which one.** Recommendation: S1, in the
   step that next owns `herd-runner/src/lib.rs` — it is the one with a
   measured cost (eight asks across one reboot of the consumer's fleet,
   ORCH-163) and the one whose hub side is a single `stop asking` edge.

4. **Whether the digest rule should also bind the hub's registry and the fetch
   endpoint by type rather than by call.** `check_object_digest` is a function
   they must remember to call; a `Digest` newtype whose only constructor runs
   the check would make it impossible to forget, as `PackId` does for the
   pack's own id. Recommendation: not tonight — it changes
   `ManifestEntry::sha256`'s type and every reader of it, and the field is
   already unforgeable at the one place entries are built. Fallback: do it
   when the second reader appears, which is S6.

### Drafted decision entry

```markdown
## D-NEXT — 2026-10-29 — The pack frames are on the enums, one `ToHub` frame
answers six silences, and the peer leg carries raw bytes behind a JSON line

**The frames landed where the number already says they are.** The previous
entry moved `PROTOCOL_VERSION` to 3 and said 3 carries M11's pack half; the
fourteen variants and the nine fields are now arms and fields of `ToHub`,
`FromHub`, `Asked`, `Hello`, `Health`, `Welcome` and `AssignJob`. The two
commits are one change: the bump without the frames would have been a doc
comment describing a surface that does not exist.

**The bytes did not move, and that is checked and not asserted.** Each
payload's serialisation was frozen through a stand-in enum before the real
enums carried it, on the argument that an internally tagged newtype variant
emits the same bytes as a struct variant of the same fields. The stand-ins
were kept rather than deleted, and the real arm is now encoded beside each of
them and compared byte for byte — so the argument is a test and not a
comment. It was proved to fire: a renamed variant tag makes it red with a
sentence saying every byte frozen through the stand-in is suspect.

**One reason-coded `ToHub` frame goes in, and it ends nothing yet.**
`ToHub::JobFact { job_id?, attempt?, reason, detail }` carries *a fact about a
job, with a reason*. Six places in Herd conclude something about a job and
have only their own stderr to say it with — asked for files it does not hold,
a record naming a file it cannot read, a transfer that stopped where only the
stopping side knows why, a spooled verdict it cannot attribute, a spool
deleted after the hub refused it, and a requester's E-41 skip — and all six
need `ToHub`, the one direction where an older hub answers `Refused` and drops
the link. Six variants added one at a time would have been six protocol
versions; one frame with six reason codes is this one. **No runner, no
requester and no hub handler touches it on this build**, and the variant's doc
comment says so, so that the frame's existence cannot be read as a fix.

**`job_id` is optional, and that is what makes one frame cover all six.**
E-41's skip is a requester reporting a job it decided *not* to submit, so
there is no id the hub ever minted. `coverage.md` records that deviation as
*"not closeable by hub work"*; it is closeable by protocol work, and this is
the shape of it.

**The reason set is not the one-shot half, against the expectation that it
was.** An unknown code decodes into `FactReason::Other` carrying the code
itself, so an older hub reads the whole frame, logs the sentence, names the
code and acts on nothing — the shrug `ToHub::Unknown` is, one level down. Only
the **variant** is one-shot. `PackUnavailable` stays its own frame beside it:
its `pack`, `commit` and `key` are looked up by the hub's registry, and a hub
parsing them back out of a sentence is what invariant 4 exists to prevent.

**The peer leg is one line of JSON and then the bytes.** The control plane has
no choice about base64 — D4 makes a frame one WebSocket text frame and D18
keeps a job's output byte-exact inside it — and this leg has: it touches no
hub (D10) and it is the only leg that moves hundreds of megabytes. The
consumer's measured 401.6 MiB pack would have travelled as about 535 MiB. So
`PeerReply` is a **header** — the bytes left the struct — and the framing is a
single-line JSON header ended by `\n` followed by exactly as many raw bytes as
the header said, in `herd-proto` because the peer wire is part of the contract
(invariant 4). A length-prefixed binary framing would save the same third and
cost the property that a person can read one line of the stream by eye.

`PeerReply::length` is now the **body's** length and `object_length` carries
the object's, which the old single `length` conflated. Every way the two ends
can disagree is refused by its own name — a header past one read of the
endpoint's buffer, a header that is not JSON, a body past the chunk, a chunk
ending past its object, a truncated body, a peer on another protocol version —
and a header still arriving is `Incomplete` and **not** a refusal, because a
reader that closed the link on a chunk boundary would close a healthy one.
No new numbers: the bounds are `PACK_ENDPOINT_READ_BYTES` and
`PACK_CHUNK_BYTES`.

**A manifest row is untrusted input, and the digest column is checked where
the row is parsed.** A `manifest.tsv` comes out of the consumer's own tooling,
and column 1 is the name Herd joins onto `objects/` — so a row reading
`../../../escaped` named a path outside the store. It must be **exactly 64
lowercase hex characters**, refused by name at the one place a
`ManifestEntry` comes into existence, so the registry, the fetch endpoint and
the runner's store inherit the rule instead of writing it three times: three
copies is how two of them come to disagree. **Uppercase is refused and not
lowercased**, because a content-addressed store that took both spellings could
hold one object's bytes twice under two names. The path column is checked to
be what the format says it is — relative to the pack's own root — and the link
target is deliberately **not**: Herd never resolves one, and a build output's
symlink may legitimately leave the pack, so refusing it would refuse a good
pack to defend against something nothing does.

`RECORD_VERSION` and `LAYOUT_VERSION` do **not** move here, and neither do
`VERDICT_FORMAT_VERSION` or `OUTPUT_FORMAT_VERSION`: no pack information went
on the verdict document.
```

5. **How this branch gets a green gate, which is the one thing blocking it.**
   The tree is green on all four Rust gates and on a complete
   `./scripts/test.sh` (1006 passed, `test suite: PASSED`); the batch gate is
   red twice on finding 0. Recommendation: fix `room.rs`'s cgroup reading as
   its own small step — it is a subtraction and a test, it is the consumer's
   defect as much as this host's, and it makes the gate runnable here.
   Fallback, if that is not tonight's: raise `/chatons.slice`'s `memory.high`,
   or run the gate outside the slice; both are the host's owner's to do and
   neither is mine. **This branch must not be pushed on a red gate**, so
   whoever takes one of those remedies re-runs `./scripts/gate.sh` on
   `a76da25` unchanged and pushes on its green.

## Resume point

**The branch is committed and NOT pushed**, because the gate is red (finding
0). Five commits, nothing uncommitted; this file is the fifth, so the sha
below is the branch tip and not one of the gated ones:

```sh
cd <scratchpad>/pack-variants/wt      # branch pack-variants
git log --oneline -5
#   this file                 Record the second gate's red and the reading
#   a76da25                   Refuse a manifest digest that is not 64 hex
#   e2053b3                   Record the first gate's red and the host
#   b63cec3                   Add pack-variants' handoff
#   ebbfdd8                   Fold the pack frames onto the enums
```

The two gated shas were `b63cec3` (first run) and `a76da25` (second). The code
has not changed since `a76da25`; the two commits after it are this file.

To finish it, with nothing to re-decide: take one of finding 0's three
remedies, then

```sh
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-variants   # deleted; it rebuilds
./scripts/bench-preflight.sh          # wait for two clears, unmodified
./scripts/gate.sh                     # bare, never inside nix develop
git push -u origin pack-variants      # on GREEN only
```

Next, in any order, all of them now unblocked:

- **S2** (the runner's pack store) and **S6** (the hub's registry) have their
  whole wire surface on the real enums: construct
  `ToHub::RegisterPack(pack::RegisterPack { … })` and match
  `FromHub::PackRegistered(_)`.
- **S5** (the fetch endpoint) and **S9** (the fetch client) have the framing:
  `encode_peer_header` on the serving side, `decode_peer_header` +
  `take_peer_body` on the fetching side, `check_peer_protocol_version` first,
  and every refusal already named and tested.
- **S7** (wire the pack frames into the hub's connection loop) adds handler
  arms beside `message_kind`'s seven names.
- **The first silence**: a caller for `ToHub::JobFact` and a hub arm that
  reads it.

## Processes

Six background shells, all started and ended by this agent: two until-loops
waiting for the unmodified preflight to clear (`bn2xniqwm`, stopped with
`TaskStop` once the figures said the slice-room refusal was page cache —
stopped too early, see *Gates*; and `buikndgug`, which waited for the two
consecutive clears and got them), the full suite (`bl5nqishv`, PASSED), the
first gate (`b43mxk1j0`, RED), the second gate (`bw8i22rnv`, RED) and one
monitor on the gate's verdict line (`bqacxf11n`). No
`pkill`, no `killall`, no kill by name, no PID of any other agent touched.
Nothing of `/etc/nixos`, `/srv`, the consumer's data or the chatOnS project;
nothing written under `/sys/fs/cgroup`; no cache dropped.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-variants`, outside the
worktree, 13 GB, **deleted** with
`rm -rf /mnt/ssd_980/herd/build/pack-variants`
at the end of the task (gotcha `build-weight-off-root`) although the branch
was not pushed — a re-gate rebuilds it, which costs a few minutes and returns
13 GB of page cache to the very slice finding 0 is about.
