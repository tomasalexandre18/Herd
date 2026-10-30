# pack-proto — handoff 2026-10-29

- **Item:** ORCH-159 (D209's capability frame, *"numbered on neither protocol
  enum today"*), with D199 and ORCH-156's version triple; step S1 of the M11
  pack-half plan
- **Agent:** herd-step
- **Branch / commit:** `pack-proto` / `a99dfa9`, based on origin/main `b37b2a4`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger` names
  the landing and fills the record of `templates/landing.md`

## Done

**`PROTOCOL_VERSION` is 3** (`herd-proto/src/lib.rs:147`), and its ~67-line
doc comment is rewritten: it was written throughout about *"this 2"* and said
*"the day this becomes 2"*. The new text says what 2 bought (the record half
alone — D205's correction to M11's own false *"paid once for both halves"*),
what 3 buys, and **what is not foreclosed**, which is the premise the brief
told me to inherit rather than rediscover: D204 constrains the feature
surface and not this constant, so `#[serde(default)]` fields on existing
variants and `FromHub` variants only a runner sees stay free afterwards. What
still has to move it — a `ToHub` variant, a requester-facing `FromHub`, a
removal or a re-meaning — is listed as the reason the `ToHub` set had to be
exhaustive.

**A new module, `herd-proto/src/pack.rs`** (1 086 lines), holding the whole
pack surface including the peer wire: invariant 4 does not except the leg that
never touches the hub (D10). Nothing in it does I/O and nothing in it is
behaviour.

### The identity rule, as implemented

`<id>` is the **sha256 of `manifest.tsv`'s bytes** (issue #16, the consumer's
own measurement of 2026-10-13). `PackManifest` is built so a re-serialisation
cannot happen by accident, and four things make it unavailable rather than
discouraged:

1. `bytes` is a **private** field; the accessor returns `&[u8]` — the bytes
   that went in, never bytes assembled from the parse;
2. it derives **no `Serialize` and no `Deserialize`**, and `ManifestEntry`
   derives neither either. A struct on a frame has to be serialised to be
   sent, so what rides a frame is `Vec<u8>` base64
   (`crate::bytes_as_base64`) and this type never leaves its process;
3. **no `Display`, no `ToString`**, no method anywhere returning manifest
   text. There is nothing to call;
4. its `Debug` is hand-written and prints `{id, bytes: N, rows: M, files: K}`
   — a summary, so even a debug print cannot be pasted anywhere as a
   manifest. Asserted: `debug_prints_a_summary_and_not_the_manifest`.

`PackId` has exactly two constructors and they are named for the difference:
`of_manifest_bytes(&[u8])` computes, `as_announced(String)` takes a peer's
word. `PackManifest::disagreement_with` is the hub's check, and it names both
digests.

### The `d`/`l` fixture, and what it proved

The fixture (`pack.rs` unit tests and `tests/pack_frames.rs`, independently)
is four rows: two `kind f`, one `kind d` (`bin`, mode 0755, sha
`2222…`), one `kind l` (`bin/tool-latest` → `bin/tool`, sha `3333…`). It
proved three things a fixture of `f` rows alone would have let pass:

- `objects_needed()` returns **2** entries, not 4: the `d` and `l` shas are
  absent from it by name (`!needed.contains_key("2"*64)` and `"3"*64`). A
  completeness check without the `kind == f` filter would look them up under
  `objects/` and report a good pack as incomplete;
- the `l` row's seventh column is parsed as `link_target` and the `f` row's
  is `None` — so the 6-or-7-column rule is exercised in both arms;
- `mode` is carried as the **text** `"0777"`, never parsed, since Herd never
  applies a mode.

And `ManifestKind::from_letter` returns `None` for an unknown letter, which
every call site treats as a **refusal**: a kind read as "not a file" would
drop the row out of the completeness check, so a pack missing those bytes
would register as complete. The test
`an_unknown_kind_letter_is_refused_rather_than_dropped` asserts that the
refusal text says what reading it the other way would cost.

### The one-line defect, fixed

`FromHub::Welcome::version_note` now carries `#[serde(default)]`. It was the
only `Option` on either frame enum without it, so a `Welcome` missing that key
was a serde **missing-field error** and not `FromHub::Unknown` — outside the
catch-all the bump to 2 bought. The error kills the link, the runner redials,
and it reads the same undecodable frame again: D128's loop, on the one frame a
runner cannot get past. Asserted by
`a_welcome_without_a_version_note_decodes`.

### The protocol refusal's new text

`version_mismatch_reason` (`herd-proto/src/lib.rs:2548`) said only the two
numbers. It now reads, in full:

> protocol version mismatch: the hub speaks version 3, the peer announced
> version 2. This peer cannot work at all until its host is rebuilt — it will
> take no job and submit none, and every other peer still on version 2 is
> refused the same way as it dials, so a fleet goes quiet machine by machine.
> The check is equality and not a floor, so the hub and its peers have to move
> together: rebuild this peer at version 3, or roll the hub back to version 2,
> but a peer and a hub on two different numbers never speak

The constant's own doc said all of this; the sentence a peer reads said none
of it, and a runner's owner is reading her runner's stderr and not a doc
comment. `herd-hub/src/server.rs:2984` calls the function and is untouched; no
test anywhere pinned the old wording (`rg 'the hub speaks version'` outside
`herd-proto/src` → no hits).

`pack::peer_version_mismatch_reason` is its sibling for the peer wire, which
gets **no number of its own**: both ends are joined to the same hub at the same
version, and it says so and sends the operator to the handshake.

### Assertions rewritten, old beside new

| Site | Old | New |
|---|---|---|
| `herd-hub/tests/announced_job_ceiling.rs:119` | `assert_eq!(PROTOCOL_VERSION, 2, "the handshake is unchanged")` | `assert_eq!(PROTOCOL_VERSION, 3, "3 is D199's bump for M11's pack half; an announced job ceiling is additive and moved nothing")` |
| same, next line | `assert_eq!(RECORD_VERSION, 1, "every existing state root still reads")` | same literal `1`, message now says D199 moves it to 2 **with the hub's pack registry and not for this field** |
| `herd-hub/tests/ran_commit_across_versions.rs:197` | identical pair | same rewrite, worded for `ran_commit` |
| `herd-hub/src/job.rs:2477` | `assert_eq!(RECORD_VERSION, 1);` **and** `assert_eq!(herd_proto::PROTOCOL_VERSION, 2);` | both given messages; the protocol one is 3. **The brief is right that there are two and that the second is the protocol one** |
| `herd-hub/src/status.rs:627-628` | `protocol_version: 2, record_version: 1` | `protocol_version: herd_proto::PROTOCOL_VERSION, record_version: crate::job::RECORD_VERSION` |

Each doc comment was rewritten too; none of the four tests was renamed,
**deliberately** — `neither_version_number_moves` and
`the_record_version_does_not_move` are cited by name in
`docs/decisions.md:9228`, `docs/remediation/09-runner-self-limits.md:487` and
`:844-845`, and three handoffs, none of which this step may edit. A rename
would have made five citations stale.

The three pins stay **literals** on purpose: written against the constant they
are pinning they would be tautologies and would catch nothing. That is why the
`store.rs:4296` relative shape (`RECORD_VERSION + 1`) applies to the *fixture*
in `status.rs` — where it fits exactly, and where `protocol_version: 2` had
become a fixture describing a hub that cannot exist — and not to the
assertions.

`RECORD_VERSION` and `LAYOUT_VERSION` are **unmoved**, as instructed.

### The silently-false round-trip test

`herd-proto/tests/round_trip.rs`'s
`a_hello_carries_its_job_ceiling_in_both_directions_without_moving_the_version`
is the dangerous one: at 3 it still compiled and still passed. Its name
claimed *"without moving the version"*, its fixture was labelled *"the frame
as `main` emits it"*, and its last assertion compared the constant against a
frame the test had built **from** the constant. Confirmed against the tree.
What it now does:

- the doc comment carries a section saying **what the test says about
  `PROTOCOL_VERSION` and what it does not**: the literal fixture is a peer
  built before the field and therefore before the bump, so it announces 2,
  **and that is a frame the hub now refuses at the handshake**; this test
  asserts only that serde reads every field of it, which is what additiveness
  means and is a separate question from admission;
- a new `assert_ne!(protocol_version, PROTOCOL_VERSION, …)` states that fact
  rather than leaving it to a reader, and goes red if the fixture ever stops
  standing in for an older peer;
- the additiveness claim is then **made again at this build's own version** —
  a hello announcing `PROTOCOL_VERSION` with no `jobs_at_once` key decodes as
  `None` — where it is not entangled with the bump at all;
- the tautological final assertion keeps a comment saying it is not a version
  check.

The name is unchanged, for the citation reason above; the claim now lives in
the body, where it is true.

`store.rs:4272`'s `{"layout_version":99}` and `crash_points.rs:346`'s
`{"record_version":99}` were left alone, as instructed: known-bad fixtures,
wrong at any version.

### New tests

`herd-proto/tests/pack_frames.rs`, 27 tests, all green, plus 15 unit tests in
`pack.rs`. Round trip per frame payload and per new type; the id over verbatim
bytes against the `d`/`l` fixture; the one-byte-difference test; the two
version refusals; an unknown pack frame reaching the catch-all in all three
directions; the `Welcome` decode.

## Not done, and why

**The thirteen frame variants and the seven new fields on existing messages
are not in this branch, and they cannot be inside this step's ownership.**
This is a premise of the brief that the code refutes, and it is the one thing
in this handoff that needs a decision. Measured, not reasoned: I added one
field (`Hello.mesh_endpoint`) and one variant (`ToHub::PacksHeld`) to a scratch
copy and ran `cargo check --workspace --all-targets`:

```
4 herd-proto/tests/round_trip.rs  error[E0063]: missing field `mesh_endpoint` in initializer of `ToHub`
1 herd-runner/src/lib.rs          error[E0063]: missing field `mesh_endpoint` in initializer of `ToHub`
1 herdctl/src/lib.rs              error[E0063]: missing field `mesh_endpoint` in initializer of `ToHub`
1 herd-hub/src/server.rs          error[E0027]: pattern does not mention field `mesh_endpoint`
1 herd-hub/src/server.rs          error[E0004]: non-exhaustive patterns: `&ToHub::PacksHeld(_)` not covered
```

The probe was reverted and the workspace re-checked clean. Two of those files
are **named in the brief's do-not-touch list** — `herd-hub/src/server.rs`
(S7's) and `herd-runner/src/lib.rs` (held by another agent tonight) — and the
collision is unavoidable, not a matter of care:

- `message_kind` at `herd-hub/src/server.rs:8152` is an **exhaustive match on
  `ToHub` with no wildcard arm**, so any new `ToHub` variant breaks it;
- Rust requires every field in a struct-variant expression, so a new field on
  `Hello`, `Health`, `Welcome`, `AssignJob` or `Asked` breaks every
  construction site. There are 11 `ToHub::Hello` constructions outside
  `herd-proto`, in `herd-runner/src/lib.rs:3176`, `herdctl/src/lib.rs:1407`,
  the eight hub test files named under *Resume point*, and
  `herdctl/tests/inline_digest_mismatch.rs`, plus the `Welcome`,
  `AssignJob` and `Health` sites beside them.

So the step delivered the half that has no collision, and **froze the wire
shape of the half that does**, so that folding it in is mechanical. The
payloads are structs in `pack.rs`; an internally tagged enum with a newtype
variant whose inner type is a struct serialises to **exactly the same bytes**
as a struct variant carrying those fields, which `pack_frames.rs` proves by
round-tripping each one through a stand-in enum declared in the test. The
follow-up's edit to `lib.rs` is thirteen one-line newtype arms, six arms in
`message_kind`, and the seven fields.

`herd-proto/src/declaration.rs` is in this step's ownership and is
**untouched**: §4 asks for nothing in it. See *Risks and findings* for why
that may be a gap worth one `#[serde(default)]` field later.

No pack field went on the verdict document, as instructed:
`herd-proto/tests/frozen_formats.rs` is untouched and its 4 tests are green,
so `VERDICT_FORMAT_VERSION` and `OUTPUT_FORMAT_VERSION` have not moved.

## Gates

**Runs made before this commit**, on the tree of `a99dfa9`, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-proto`:

```
$ nix develop --command cargo fmt --all --check
(no output)   exit 0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Checking herd-proto v0.4.1 … herd-runner … herdctl … herd-hub
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.59s
exit 0

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 26.82s
exit 0

$ nix develop --command cargo test -p herd-proto
  lib            80 passed; 0 failed
  frozen_formats  4 passed; 0 failed
  round_trip     31 passed; 0 failed
  pack_frames    27 passed; 0 failed
  doc-tests       0 passed; 0 failed
exit 0

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit 0
```

**The last run, named and not quoted**, on the tree with this file committed:
`nix develop --command ./scripts/test.sh` (the full workspace suite plus its
skips, D27) and `./scripts/check-docs.sh`. Output goes to
`<scratchpad>/pack-proto/suite.log` and `<scratchpad>/pack-proto/docs.log`, and
verbatim into this agent's report.

**Skips:** none from any run above — `cargo test -p herd-proto` reported `0
ignored` on all four binaries and wrote nothing to the skip log. The full
suite's skips are in the report with the run that produced them; this host
reports 12 when green.

**Host, disclosed because `bench-preflight.sh` refused twice and I did not
override it.** First refusal, 23:5x: *"only 6722 MiB of memory left under
memory.high on /chatons.slice, floor is 8192 MiB"*. I read
`/sys/fs/cgroup/chatons.slice/memory.stat` as the brief allows and found
`anon` = **3 888 MiB** against `file` = 12 174 MiB — **not** the "few hundred
MiB of anon" the brief makes the condition for lifting the floor, so I did not
lift it and waited instead. Second refusal, minutes later, at 2 885 MiB of
room: `anon` had fallen to **1 309 MiB** against `file` = **14 308 MiB**, of
which **12 032 MiB inactive** — 88% of the slice's 16 269 MiB charge was
reclaimable page cache from the sibling builds. That is the brief's case, and
I set `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB=2` once to confirm it cleared — then
**unset it**, because the unmodified gate's own figure had recovered to 9 405
MiB on its own, above the 8 192 MiB floor. The figure then oscillated (2 885 →
9 405 → 7 574 MiB inside one minute) with load at 8.56 of a 9.00 ceiling, so I
waited for **two consecutive clears of the unmodified gate 20 s apart** rather
than start a suite into a host where a red result would be about somebody
else's compile. Nothing under `/sys/fs/cgroup` was written and no cache was
dropped.

## Risks and findings

Ordered by what a later version cannot repair.

**1. The reviewer's general-purpose `ToHub` frame is not in §4, and this is the
last cheap moment for it.** The review's §3 found ten places where a runner or
a requester concludes something and has only **stderr** — "I do not hold it",
"my record names a file I cannot read", a stopped transfer, an unattributable
spooled verdict, a refused spool, E-41's skip. Six of them need a `ToHub`
frame, which is **the one direction that cannot be widened later**. Its own
recommendation was one reason-coded `ToHub` *"here is a fact about work you
gave me, and why"*, so those six become fields inside one numbered frame and
the next one is free. `PackUnavailable` is a pack-shaped instance of exactly
that frame. I did not add the general one: it is outside the pack half and
outside this brief, and inventing a reason code vocabulary unilaterally is the
kind of guess the brief told me not to dress as a decision. **But it is the
single most expensive omission available tonight**, and the constant is moving
now.

**2. `PeerReply` carries its bytes base64 inside JSON, which costs 33% on the
one leg that moves hundreds of megabytes.** §4 specifies
`PeerReply { object, offset, length, data }` and I implemented it. On the
control plane base64 is forced (D4's text frames, D18); on the peer leg
nothing forces JSON at all, and a JSON header line followed by `length` raw
bytes would carry the consumer's 401.6 MiB as 401.6 MiB instead of ~535 MiB.
That framing would mean `data` is **not a field of `PeerReply`**, so it is a
type change and not an implementation detail. It is recoverable **inside this
batch** because S5 has not been written, and not after. Recommend deciding it
before S5 starts.

**3. §4 does not say how the peer leg is framed on the stream at all** —
length-prefixed, newline-delimited JSON, or something else. The control plane's
framing is D4's; the peer plane's is undecided and is a wire property, which
invariant 4 arguably puts in `herd-proto`. It must be settled **inside this
batch**: changing it later breaks every peer, which is a fourth protocol
version in substance even if the constant could stay.

**4. `PacksHeld` had an unstated semantics that was one-shot, and I wrote it
down.** §4 gives `packs` and `more` and does not say whether an announcement
**replaces** or **adds to** the hub's view. If it added, nothing in this
surface could ever retract a holding — a runner that deleted a pack under disk
pressure or an M12 eviction would have no way to say so — and the frame that
said it would be a **new `ToHub` variant**, the one shape that needs a fourth
version. I documented it as *replace*, with `more: true` accumulating and
`more: false` applying, and the test asserts `"more":false` is on the wire even
when false, because an empty announcement is the real statement "I hold none".
This is the one place I added substance to §4 rather than only implementing it.

**5. `needs` and `leaves` are on `Asked` and not on `Declaration`.** §4 puts
them on `Asked`, which works — the hub records them and resolves `packs_in` /
`packs_out` per attempt — but they come out of the same consumer TOML file that
produces `Declaration`, so one file's reading is split across two proto types.
Recoverable for free (a `#[serde(default)]` field on `Declaration`), so not
urgent. Flagged because it is the kind of split that reads like an accident
later.

**6. `PackState` has no frame carrier in §4, and I checked whether that is a
gap. It is not.** Its three values do not fit `PackUnavailable` (`Registered`
would be nonsense there) and the hub cannot compute `Incomplete`, since
holdings are per manifest and not per blob. Its natural carriers are the hub's
**record** (`packs_in` / `packs_out` on `job.json`) and `herd-hub report`, both
of which are S6's. So the enum is correctly numbered here and correctly
uncarried. Reported because the absence looks like an oversight and is not.

**7. `PROTOCOL_VERSION`'s doc describes a frame set that is not in this
commit.** It says 3 carries the pack half's frames, and the frames land in a
later step of the same batch. The claim is true of the batch and ahead of the
commit, which I judged better than the alternatives: not moving the constant
leaves the brief's central instruction unmet, makes the refusal unable to name
3, and moots the five assertion rewrites. **If the frames do not land in this
batch, that doc comment is false and the bump bought nothing** — which is a
sequencing risk for the orchestrator and not a defect in this branch.

**8. The `#[serde(other)]` catch-all currently swallows the real pack tags.**
Because the variants are not on the enums yet, a genuine `packs_held` frame
decodes to `ToHub::Unknown` on this build. That is harmless tonight (nothing
sends one) and it is why the catch-all test uses a deliberately invented tag,
`pack_rehomed`, which stays unknown for ever instead of flipping when the
variants land.

**9. Not checked, and named so: the eight other version constants.** The
review counted **eleven** version constants, of which this step moved one and
considered one other. `EVENT_FORMAT_VERSION`, `ROSTER_VERSION`,
`STATUS_VERSION`, `STATE_FORMAT_VERSION`, `INDEX_VERSION`, `READINGS_VERSION`,
`OUTPUT_FORMAT_VERSION` and `VERDICT_FORMAT_VERSION` were out of this brief's
scope and I did not sweep them. `docs/architecture.md:334-348` still opens
*"Six version numbers"*. Prose that goes false with this commit and is **not**
mine: `herd-hub/tests/unknown_frames.rs:4`, `docs/architecture.md:219` and
`:334-353`, `docs/operating.md:560-563`, `docs/handover.md:67`.

## Decisions needed

1. **Who folds the thirteen variants and the seven fields in, and when.**
   Recommendation: one step, owning `herd-proto/src/lib.rs`,
   `herd-hub/src/server.rs`, `herd-runner/src/lib.rs`, `herdctl/src/lib.rs`
   and the hub/herdctl test files, **based on this branch** and sequenced
   after the agent now holding `herd-runner/src/lib.rs` has pushed. It is
   mechanical: the wire shape is frozen and tested here. Fallback: give it to
   S7 and S9, which already own `server.rs` and the runner's `lib.rs`, at the
   cost of the variants arriving in two commits.
2. **The one reason-coded `ToHub` frame of the review's §3** — in this bump or
   accept a version 4 for it. Recommendation: in, and designed now;
   `PackUnavailable` already shows the shape. Fallback: out, recorded as a
   known cost, with the understanding that six silences then wait for a
   fourth protocol version.
3. **The peer leg's framing and whether `PeerReply.data` stays in the
   struct.** Recommendation: decide both before S5 is briefed; a JSON header
   plus raw bytes saves a third of the bytes on the only leg that carries
   hundreds of megabytes. Fallback: keep §4's shape, and record the 33% as a
   measured cost rather than discovering it in S12.

### Drafted decision entry

```markdown
## D-NEXT — 2026-10-29 — `PROTOCOL_VERSION` is 3, and the pack surface is
numbered with it

`PROTOCOL_VERSION` moves 2 → 3, which is the move D199 called for before
v1.0.0 and the first of its triple to land. Nothing published is affected:
Alexa's direction tonight is that no intermediate release ships, so the next
tag cut from `main` is v1.0.0 and the consumer resets at it.

**What the number carries.** M11's pack half, which version 2 did not: M11's
own claim that the version-2 break was *"paid once for both halves"* is false
and D205 corrected it — version 2 went on the record half alone. M12's
pack-side surface rides with it, because E-14 and E-16 are `must`
expectations inside v1.0.0 (D205) and a second bump for them would be a fleet
going quiet twice.

**What was NOT foreclosed, against an earlier belief.** It was thought this
is the last bump the project may ever make and that every future frame had to
be numbered now. D204 constrains the **feature surface**, not this constant;
it says the triple moves *before* v1.0.0 and not that it may never move
after. So `#[serde(default)]` fields on existing variants, `FromHub` variants
only a runner sees, and `#[serde(default)]` fields in a record on disk stay
additive afterwards. What is genuinely unrecoverable is a new `ToHub` variant
— an older hub answers `Refused` and drops the link — and a `FromHub` variant
a **requester** sees, which `herdctl` bails on. The `ToHub` set was therefore
made exhaustive and the rest was not held for.

**The pack identity rule, which the types enforce.** A pack's `<id>` is the
sha256 of `manifest.tsv`'s **bytes** (the consumer's measurement, issue #16,
2026-10-13), so a parsed-and-reprinted manifest is a different pack.
`PackManifest` keeps the bytes private, derives no `Serialize`, has no
`Display` and no method that produces manifest text, and prints a summary
from `Debug`: there is nothing to call. On the wire a manifest is `Vec<u8>`
base64 and never a struct. **Herd may never re-serialise a manifest**, and the
type is what makes that true rather than a rule somebody remembers.

**The `kind` column is load-bearing.** A `kind d` or `kind l` row carries a
sha256 of something that is not a file's bytes, so every completeness check
filters on `kind == f`, and a kind letter Herd has not been taught is a
**refusal** and never a row read as "not a file". Fixtures carry a `d` row and
an `l` row so the filter is proven rather than assumed.

**`PacksHeld` replaces and never appends.** A runner's announcement is its
whole holding; a pack it has deleted is retracted by not being in the list,
and `more: bool` is required because a hub that read one frame of a chunked
announcement as the whole of it would drop every pack the runner holds. Had
the frame appended, a retraction would have needed a new `ToHub` variant and
therefore a fourth version.

**Said in the refusal, not only in the doc.** `version_mismatch_reason` now
says that a refused peer cannot work at all until its host is rebuilt, that
the fleet goes quiet machine by machine, and that the check is equality so hub
and peers move together in either direction. The constant's doc comment said
all of this and the sentence a peer's owner reads said none of it.

`RECORD_VERSION` and `LAYOUT_VERSION` do **not** move here: they move with the
on-disk shapes they describe.
```

## Resume point

```sh
cd <scratchpad>/pack-proto/wt       # branch pack-proto at a99dfa9
git log --oneline -2
```

The next task is the one named in *Decisions needed* 1: fold the thirteen
payloads into `ToHub` and `FromHub` as newtype arms and add the seven fields,
**based on `pack-proto`**, owning `herd-proto/src/lib.rs`,
`herd-hub/src/server.rs` (six arms in `message_kind` at `:8152`),
`herd-runner/src/lib.rs` (`Hello` at `:3176`, `Health`, and the `Welcome` /
`AssignJob` destructures), `herdctl/src/lib.rs` (`Hello` at `:1407`, `Asked`)
and the nine hub/herdctl test files listed in *Not done*. The exact field list
is §4 of `2026-10-29-m11-pack-half-plan.md`; the payload structs and their
frozen wire shape are in `herd-proto/src/pack.rs`.

S2 (the runner's pack store), S5 (the fetch endpoint) and S6 (the hub's
registry) need only `pack.rs` and can start on this branch now: `PackId`,
`PackManifest`, `ManifestEntry`, `ManifestKind`, `FetchCapability`,
`PeerRequest`, `PeerReply`, `PeerRefusal` and the five constants are all
landed and tested.

## Processes

Two background shells, both started and ended by this agent: the
`bench-preflight.sh` until-loops waiting for the cgroup floor to clear
(`bf7bwrs3p`, stopped with `TaskStop` once the figures were read directly;
`b4qhj8ihx`, the two-consecutive-clears wait). No `pkill`, no kill by name, no
PID of any other agent touched. Nothing of `/etc/nixos`, `/srv`, the
consumer's data or the chatOnS project; nothing written under
`/sys/fs/cgroup`; no cache dropped.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pack-proto`, outside the worktree,
to be deleted with `rm -rf /mnt/ssd_980/herd/build/pack-proto` once the branch
is pushed (gotcha `build-weight-off-root`).
