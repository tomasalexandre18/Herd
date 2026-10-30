# Coverage — expectations E-01..E-55 → milestones

Derived from issue #1's expectations sheet (2026-10-12). Recounted priorities:
**44 must, 9 should, 2 could.** "Gated (M18)" means the acceptance criterion
needs a chatOnS deliverable (their declaration file, `cargo xtask pack` /
`test --pack`, results.json, push hook or nightly command); until then an
automated test against the fixture consumer stands in, and the real check
closes in M18.

**State** is as of 2026-10-30 and says where the work is, never where it
runs: `merged` is wholly on `main`; `part` is a row some of whose work is
on `main` and some of which is not — a two-milestone row with one half
landed, or a row whose own list is partly built; `planned` is unbuilt;
`waived` is a must the first consumer has waived for v1.0.0 and
`withdrawn` one they have retired as never needed (**D248**); neither is
ours to re-add, and `unmet by choice` went with D206's premise. What is
deployed is a tag behind `main`, so a `merged` row is not a promise about
the running system — `docs/status.md` holds the versions, the disk and who
operates them.

**The musts, re-derived row by row from the table below on 2026-10-30
against the tree at `df47266`:** **36 of 44** wholly on `main`; **5**
more with part of their work on `main` (E-12, E-16, E-17, E-37 and
**E-42**); **1** with none of it (**E-14**); **2** the consumer has
released us from — **E-49 waived** and **E-50 withdrawn** (**D248**).
36 + 5 + 1 + 2 = 44.

**E-42 moves from `planned` to `part`, and that one row is the whole of
what moved.** The sentence this replaces read *"4 more with part of
their work on `main` … 2 with none of it (E-14, E-42)"*, and it was
wrong about E-42. **The recount is the orchestrator's ruling,
2026-10-30**, taken after an execution agent measured the row and
declined to move a must between columns itself. **E-42 is two rules and
one of them is on `main` and asserted:** the hub derives a dedup key
over the command, the label set and the declared commit
(`herd-hub/src/job.rs:817`, length-prefixed fields, the commit a
counted third since **D156**), and `JobDirStore::submit` looks that key
up in `open_work` under the store's one lock and joins **any
non-terminal job, running included** (`herd-hub/src/store.rs:2234`),
asserted against the real binaries in
`herdctl/tests/declared_dedup.rs`. Coalescing exists nowhere, and
**D75** keeps the two apart — *"Dedup joins; coalescing discards. They
are two behaviours, not one"* — so joining a running job is right for
dedup and is **not** evidence that E-42's second rule is built.

**E-14 is `planned`, and a reading of 2026-10-30 that held it withdrawn
is refuted by the tree.** **D248** withdraws **E-50** alone and waives
E-49; E-14's State cell reads `planned`, its note reads *"gated on
their replication (M18)"*, and M12 has nothing on `main`. It is the one
must with none of its work built, and it stays a `planned` row:
**retiring a must is the consumer's act and nobody else's** (D248,
D249), so no count here moves a row into `withdrawn` without one.

**Only E-42's State cell was re-measured against the code for this
recount; the other 43 carry their 2026-10-30 reading**, and batches 18
to 24 are not claimed to have moved any of them. E-37 gained its fifth
hub transition and **stays `part`**, because the nightly's verdict
cannot be shown end to end until M14's nightly exists. ORCH-241 carries
the standing staleness of these paragraphs.

**The done figure did not move; the shape of the eight that are not did.**
Five landings reached `main` after the 2026-10-29 count (batches 12 to
16) and they put M11's pack half in the tree, so E-12 and E-17 move from
`planned` to `part`. No row became `merged`, none regressed, and the
other six were checked row by row and left alone.

**Two landed decisions give two different ceilings, and both are stated
settled, and the figure to quote is **42 of 44**.** D205 (Alexa,
2026-10-29) put M11's pack half inside v1.0.0 and said that made it
*"44 of 44 of the first consumer's musts"*; D206, the same day, declared
E-49 and E-50 unmet by choice, and the two together gave 42. **Alexa
settled it on 2026-10-30** — *« V1 c'est 44 ou demande a chatons »* —
and **the consumer was asked and waived both** (**D248**, **D249**):
v1.0.0 ships **42 of 44** and **names the two it does not meet** in the
release notes, which is their own condition for the waiver. **E-50 is
withdrawn rather than waived** — they do not need it, their fleet is the
NixOS server plus a 4060 PC on a dedicated Linux boot, and they need KVM
so native Linux — and **E-49 is waived with a measurement coming that is
not E-49**: the server runner's re-enrolment at the migration, which
they say themselves is not the new-machine walk, so E-49 is not declared
met on it. **D206 is overtaken, not reversed.**

| E | Prio | What | Milestone | State | Note |
|---|---|---|---|---|---|
| E-01 | must | submit by commit, never machine | M10 | merged | a submission names a declared command and a commit resolved in the requester's own checkout (D106); no field of it names a machine |
| E-02 | must | ran-on line, verdict unchanged | M10 | merged | the `ran on: <runner> (<why>)` line is asserted on `herdctl`'s own stdout, first line, with the verdict unchanged behind it; format frozen per Q7; results.json's real fields and a worked example are now known (issue #16); real verdict gated (M18) |
| E-03 | must | stream, detached mode, wait | M2 | merged | foreground streaming itself lands in M1 |
| E-04 | must | waits explained with figures | M8 | merged | the sentence carries the position, **what is ahead** (the jobs in front by id and level, three named and the rest counted) and why, with the figures E-04 asks for — `1.2 GiB free in tmpfs, 2 GiB needed` comes out of one formatter and is asserted in those words against the real binaries. Said again on its own clock, `--wait-restatement`, 60 s by default and printed at startup: the record keeps M6's news-only rule — news being the job's own **cause and weighed level**, not the queue figures wrapped round them, which move whenever any other job does (D171) — the stream gets at most `wait / cadence + 1` lines, which is **eleven over ten minutes** and not one per dispatch pass (D91). The figures a requester is told are the last pass's, held in memory and not on the record, so a job queued 74th of 184 is not still saying so when it is 3rd of 12. Both failure modes are tested by mutation — removing the clock gives 1 line over 20 cadences, restating per pass gives 79 against a bound of 22 — and a third, that a queue which merely *moves* must cost no write at all about the jobs standing still in it (13 history lines on v0.3.6 against 1) |
| E-05 | must | identical submissions deduplicated | M6 | merged | keyed on the command, the labels and the commit a declaration names, length-prefixed (D69, D156); only an open job is joined — work that is over is rerun (D70); the higher level wins; verified: two waits, one execution, one log |
| E-06 | must | cancel kills process tree | M3 | merged | propagation and the sibling-untouched case asserted in M3; the kill becomes a process-tree kill in M4. The 'output produced before the cancel survives' half now waits for that output to exist before cancelling: on two cores the kill beat the job's first write in one run of six, and the test read a job that printed nothing as output lost (D101) |
| E-07 | must | dry run shows placement | M9 | merged | `herdctl would-run` lists the eligible runners with the labels each answers for, what is queued ahead, and the verdict the claim step gives, out of **one** dispatch pass that is thrown away: the same `matcher::place`, the same `queue::order`, the jobs ahead of it placed against the same seats so the slot it would really find is the slot it reports. Nothing is minted (the hypothetical record carries an id this hub cannot mint), nothing is written, no slot and no reservation taken — verified by comparing **every byte of the state root** before and after, path by path, against a quiescent hub with one build already holding the only slot. Also verified not to lie: the real submission of the same job waits for the same reason |
| E-08 | must | refuse dirty or unpushed | M10 | merged | against a real git fixture: a dirty tree is refused naming the file, an unpushed commit naming the hash |
| E-09 | should | deadline cancels unstarted job | M9 | merged | pulled forward: E-41 (must) needs it, **and** it is D88's only bound on a job no connected runner could ever fit. An absolute instant in Unix seconds, resolved from `--deadline +15m` or RFC 3339 by the CLI, judged on the hub's clock, recorded on the record (D94). It bounds waiting only: a job that has **started** is never touched by it (that is M4's time cap, exit 124), asserted with a job whose deadline passes while it runs and which finishes 0. A queued job past it ends `cancelled (deadline)`, exit 125 (D40), and the sentence names what it was waiting for when the moment came — so a job cancelled because its machine was switched off says `she is gaming` rather than only `deadline`. Verified: past at submission, past across a hub restart, and firing with no runner connected at all |
| E-10 | should | depend on one other job | M9 | merged | capped: one parent, no conditions (invariant 1). Start after its **success**, which is exit 0 and nothing else; every other ending — non-zero, signalled, could-not-start, stopped-by-cap, cancelled, lost, impossible — ends the child `cancelled (parent)` naming the parent and how it ended (D95). A cycle is **unrepresentable** rather than checked: a parent must already exist, ids are minted in order, so a job can only name one older than itself — no job can name itself and no two can name each other, verified by guessing the next id and being refused. A parent this hub cannot read is never read as one that succeeded |
| E-11 | must | consumer-declared commands, nix develop | M10 | merged | the field set, `[executor]` hooks and ten real commands are now known in full, including `labels_from` (D74) and the step timestamps (D77) (issue #16); gated: their real declaration file (M18) |
| E-12 | must | packs by (commit, key), no shared build trees | M11 | part | D10: hub registers, peers transfer; the real shape (`manifest.tsv`, `pack.meta`, `objects/<sha256>`, id = sha256 of the manifest) and measured figures (~400 MiB, ~560 files, verified under 2 s) are now known (issue #16); gated on their pack (M18). **D148 holds the line this row is on**: a pack is read next by a *job*, so it never touches the hub; a record is read next by a *requester*, so it does. M11 builds both and splits record-first if it overflows, with both halves' frames designed in one move to `PROTOCOL_VERSION` 2, approved 2026-10-16, because a break is paid by the deployment and must not be paid twice. **M11 is merged and this row is not.** What shipped first is the record half (D148, D150, D152). **Recounted 2026-10-30 against the tree at `26e83cf`, `planned` to `part`: the sentence this replaces — that the pack half has no frame on either protocol enum and no code behind one — is refuted by the tree.** On `main`: `herd-proto/src/pack.rs`, with five pack variants on `ToHub` and five on `FromHub`; the hub's `PackRegistry` keyed on `(commit, key)` (`herd-hub/src/registry.rs`); the runner's content-addressed store, its peer fetch client and its mesh-bound serving endpoint (`herd-runner/src/packstore.rs`, `fetch.rs`, `endpoint.rs`, `mesh.rs`); and a runner that announces what it holds, measured against both binaries as processes (`herd-hub/tests/packs_announced.rs`). What is **not** on `main` is the path a job takes through any of it: the hub sends `FromHub::AssignJob`'s `packs_in` and `packs_out` empty (`packs_in: Vec::new()` in `herd-hub/src/server.rs`), the runner binds and drops both (`herd-runner/src/lib.rs`), and `ToHub::RegisterPack` is constructed nowhere under `herd-runner/src` — so no job is ever given a pack and no pack a job leaves is ever published (ORCH-234). `main`'s own prose says the same in `herd-hub/src/registry.rs` and `docs/status.md`: wired at the edges, not through |
| E-13 | should | content-addressed, complete or absent | M11 | planned | done with the store: retrofitting CAS would force a migration, and D10 needs it for free diffs. **Amended 2026-10-29:** the deduplication that ships with the new disk policy is not that retrofit and forces no migration — `objects/<sha256>` beside `jobs/`, every file in a job's `files/` a hard link to it, **new arrivals only**, `LAYOUT_VERSION` unchanged, and an older binary reading a shared root sees ordinary files (`docs/disk-policy.md` §2.9). Per-file content addressing for packs, which is what this row is about, is unchanged |
| E-14 | must | warm caches ready before job | M12 | planned | Q5/D9: no cache engine in Herd; their own words, "you move packs; our executor owns warm caches" (issue #16), shrink M12 to moving packs, key-aware placement and GC/budgets — the cache-specific prepare step is dropped for the generic step timestamps (D77); gated on their replication (M18) |
| E-15 | must | full job record on disk | M2 | merged | byte-faithful logs (D18); a lost log chunk leaves a named marker, and a lost tail is asked for (D36). **Kept files are real** (D131): what a job leaves under `{out}` is collected before the workspace goes and written into the record's `files/` directory, capped at 32 MiB with a refusal carrying figures over it. **D148 keeps 32 MiB and changes what it means**: it stops being a refusal boundary and becomes the line between the inline carriage (the verdict frame, as today) and the streamed one (M11), so a requester never declares which it needs and cannot tell which it got. Retention of the record itself — 14/90 days by priority class — is still M12's; the separate budget for a job's large files shipped in M11 instead (D152), because streaming without a sweeper is a disk-full outage with a delay fuse. **Re-cut 2026-10-29 (`docs/disk-policy.md` §0): the two numbers are unchanged and now mean a maximum age rather than a deletion time.** What decides is space — free space on the filesystem the state root sits on — with the span as the backstop that acts when nothing is under pressure and the output floor doubling as the guard that keeps space pressure off a fresh record. The record side is 0.32 % of the first consumer's state root as they read it on 2026-10-29, so record removal is a correctness, privacy and walk-bounding instrument rather than a space one; the space is in the collected files, and deduplication by hard link ships on by default to divide their arrival rate. M12's done-when — a step result past retention gone while a 60-day merger result reads fine — survives verbatim and is the backstop's test. One latent defect closed on the way (D148): `FromHub::JobLog` sent a job's whole log in one frame, so a log over ~48 MiB built a frame past the ceiling and a `herdctl wait` got exit 125 with no verdict — the backlog now travels in 8 MiB frames, no protocol move |
| E-16 | must | budgets, gc with pinning | M12 | part | per runner, not central (D10); keepers pin. D148 adds the hub's **own** budget beside them, for the large files a record carries, with a floor in days and a named line when eviction breaks it — **built in M11** (D152), so what is left here is the per-runner budgets and keeper pinning. **The floor in days survives (D202, re-affirmed 2026-10-27) and gains a second job on 2026-10-29: it is the record guard as well as the byte floor**, and a second bound joins it — a free-space floor on the filesystem, which is the only quantity that can see a record byte, a log byte and a collected byte at once, because the output budget governs the collected files only by its own documentation. Nothing is kept without a bound: what can no longer serve is removed after a stated delay, saying so, or moved to a configurable archive path, and whatever is held appears in a report with its size and counts toward the space calculation (`docs/disk-policy.md` §0.6, §0.7); keepers pin packs only, because a record has left its runner by the time the job is reported finished |
| E-17 | must | atomic publication everywhere | M11 | part | a manifest registers only once every blob is held; job-record atomicity already in M2 (Q1). **Recounted 2026-10-30, `planned` to `part`: the manifest half is built, and the sentence this replaces — that the manifest is part of M11's unbuilt pack half — is refuted by the tree.** Atomicity is the registry's first stated property: every file of a pack is written and fsynced inside `packs/manifests.part/<unique>/`, that directory is fsynced, and only then does it take the published name in one `rename(2)`, so a hub killed anywhere leaves a pack that is absent or whole and there is no third outcome (`herd-hub/src/registry.rs`). The runner's own store is whole-or-absent beside it — a half-fetched pack a kill left behind is removed and said (`herd-runner/src/fetch.rs`). What is not on `main` is the publication there would be anything to register: no runner constructs `ToHub::RegisterPack`, so the *only once every blob is held* clause has no live path, and nothing lays a pack out at `{in}/<key>` for a later job to read (E-12, ORCH-234). Completeness is deliberately **not** the hub's to compute — holdings are per manifest, so `PackRegistry::state_of` never returns `PackState::Incomplete` and a fetcher consults its own store for what it lacks |
| E-18 | must | four priorities with aging | M6 | merged | aging computed, not written (D67); default step 30 min, so the lowest level reaches the top in 90 min; the raise is upwards only (D72); the 90-min figure itself is a simulated-time and 6-second-real-clock assertion, not a measurement on a busy day |
| E-19 | must | no fallback while available | M7 | merged | preference is one declared integer per runner (D81); a draining runner does not count as available (D80); the limit is `--stickiness-limit`, 3 h, printed at startup and in every waiting sentence; verified against two real runners: the second build never moved while the PC was merely busy — proved now by three of the hub's own restatements rather than by a 600 ms sleep (D101) — and moved in well under 10 s when it died. Switch-off case retested with E-33 in M9 |
| E-20 | must | one queue for everything | M6 | merged | the queue is the unfinished job directories; one dispatch pass, which a submission goes through like everything else; no lane for a merger. "Every QEMU on every client listed as a job" needs their executor and the status document (M13, gated M18) |
| E-21 | must | lost runner, requeue once | M3 | merged | silence threshold is hub policy; a second loss ends the job `lost`; verified: two attempts, never a third, across six alternating runner lives |
| E-22 | must | build slots, derived -j | M7 | merged | slots are read from the records every pass and counted nowhere, so none can leak and none is rebuilt after a restart (D79); `-j = min(threads, 3/4 of memory / 1792 MiB)` per slot, rounded down, never 0 (D83) — the owner's 17 GiB PC derives E-22's own `-j 7`; the figure is on the record before the job is handed over (D82) and reaches the process through one `{j}` token. Verified: a `kill -9` mid-job gives the slot back, and the hub that came back **says** the one slot is taken, with its figures, while the inherited job still holds it — before this sweep the restart case was a comparison of two intervals that a slow host satisfied with a slot that had already been freed (D101). **The memory figure is `MemTotal` unless the owner says otherwise** — whole-job memory admission is M8's (E-29) |
| E-23 | should | placement prefers cache locality | M12 | planned | preference only; E-19 governs waiting |
| E-24 | must | probed/declared/denied labels | M5 | merged | the offer and its origins; matching them at claim time is M7 |
| E-25 | must | capability kinds and exclusion | M7 | merged | algebra and closure landed in M5 (symmetric, not transitive); enforced at claim time in M7, with the seats carried through a pass so two jobs offered together contend. Verified against the real binaries: two counted jobs serialise on one slot and an exclusive one overlaps neither, including a job that holds no label at all. Note for the consumer: on their declaration `gpu-venus excludes igpu-vfio` is **inert**, because `igpu-vfio` is exclusive and host-wide already blocks everything — what keeps the two apart when something outside Herd holds the iGPU is the busy probe (E-26) |
| E-26 | must | externally busy capability | M5 | merged | a probe nobody can read suspends too; an unknown *or duplicated* key in the capability file refuses the runner's startup; wait-reason wording polished by M7 |
| E-27 | must | metal lease, drain first | M9 | merged | lease model landed in M5 as counted(1) plus a probe; drain-first is **a declared flag on the capability** in the machine operator's own file (`drain_first`, D86/D98) — nothing in Herd reads the `metal-` prefix, and a test proves it by declaring the same label without the flag and watching the trial run beside a build. The flag means a quiet host **and** a reservation: from the moment such a job is queued the host claims nothing new, computed from the records before a pass places anything so queue order cannot starve it, bounded by `--drain-first-limit` (15 min, from submission, D79) past which the sentence says the drain did not win. The lease is read on the consumer's own codes (0 free, 10 busy, 1/64/127/signal unreadable → suspended with the code named, D48 — the stricter reading they agreed to). A declared requeue code (D76) gets `--requeue-budget` 3 goes with the capability re-probed, then ends `lost` naming the code, the figures and the machines that answered; counted off the attempts, separately from E-21's two (D96). Verified against the real binaries: a leased trial waits naming the holder then runs when the lease clears, a build submitted during a drain is held off by name, and three 75s leave E-21's two goes unspent |
| E-28 | must | reference label carried | M5 | merged | a capability like any other; placement preferring it last is M12 |
| E-29 | must | memory admission with floor | M8 | merged | a running job is costed at what it **declared**, never at what it uses: a job admitted a second ago has allocated nothing, and M4's cap is what makes the declaration honest (D87). The sums are derived from the records every pass through the same filter as the slots, so nothing can leak and nothing is rebuilt after a restart (D79), and the pass carries them forward so two jobs offered together contend. A build's need is its per-thread figure times the `-j` it was given (D82). Floor announced by the machine, a fifth of what is lent between 1 and 8 GiB by default — 6.2 GiB on their server, 3.4 GiB on the PC, against the 6 and 3 they name. An override larger than the real `MemTotal` is **clamped loudly** where it is written, because a floor over memory that does not exist guards nothing (D89). An absent floor is unknown and admits nothing. Verified against the real binaries: 4 GiB held by an inherited job, 3 GiB free of 7, a 5 GiB job told so by figure |
| E-30 | must | kernel memory ceiling | M4 | merged | WSL OOM replay is a manual M17 check; real ceiling asserted only under a delegated cgroup; RLIMIT_AS fallback per D23 |
| E-31 | must | tmpfs reservation, wait | M8 | merged | the runner announces the mount's **capacity** (`/dev/shm` by default, checked to be a tmpfs, or `--host-tmpfs`); free is that less what the records declare, which is the only arithmetic that cannot leak a reservation (D90). Short → **waits for that machine**, because a job ending there frees the room; more than the machine has in all → *not here ever*, so the job does not stick to a machine that could never hold it (D88). A figure that cannot be read is absent and the job is refused there **by name**, never `Some(0)`. Verified: 3 GiB twice on a 4 GiB tmpfs serialise with no overlap, the second starts when the first ends, and a job needing the whole 4 GiB runs straight afterwards — the proof nothing leaked. **Honest limit:** a job no *connected* runner could ever fit waits rather than ending, with a sentence saying "ever"; the bound on such a wait is E-09's deadline in M9 (D88) |
| E-32 | must | host-wide bound, load recorded | M8 | merged | the sums are per host and host-wide, carried through a pass so two jobs offered together cannot both be admitted past the bound. Load reaches the hub on the handshake and every health report, lands on the line explaining every placement (`load 0.42 there, recorded and not a gate (E-32)`), and gates nothing: the same job goes to the same machine at load 0.1 and at load 50, the two lines differ in that figure alone, and a test reads the module's own source for any comparison against a load. p90 < 12 figure read after adoption (M18+) |
| E-33 | must | owner switch in 10 s | M9 | merged | three positions, with the owner's reason required for `off` and `drain` (D97). A **file** under the data root, not a port (invariant 3): `herd-runner switch off --reason …` on her machine, `herdctl switch <runner> off --reason …` through the hub, and the hub's half writes the same file so there is one state and it outlives a reboot. `herdctl switch` prints the fleet's positions with their reasons. **What `herdctl switch` now guarantees (D99):** it reports success only once the runner has **written the position down**, fsynced, on its own disk — both paths write before they apply, the acknowledgement rides the runner's own control connection (`ToHub::SwitchApplied`, protocol 6), and the wait is bounded by E-33's ten seconds. Anything short of that exits 2 with the sentence on stderr, naming the runner, the position asked for, what the hub actually sees and what nobody has confirmed; it is never reported as plain success. The window this closes was real: before D99 success came back about 1.5 s before the machine knew anything, and a runner killed inside it came back on its previous position. **Neither position touches the job in flight** — that is the difference from the shutdown drain — and `off` stops serving content too (D11): the gate is `Switch::serves_content`, asserted, with the rule that M11's fetch endpoint is built behind it; the endpoint does not exist yet, so that half is a gate and a decision rather than a measurement. Measured with the real binaries from **both** sides while a job was running: claims stop well inside 10 s, the running job finishes 0, a job assigned as the switch moves is handed back naming the switch, a drain finishes its job then leaves, a runner killed with `kill -9` in the instant `herdctl switch` returned comes back off with her reason, and an acknowledgement that times out (runner paused) is a partial result naming the runner. The fsync is a code path, not a measurement: no power was pulled from a machine. Desktop shortcut on the real PC in M17 |
| E-34 | must | niced, no inbound, disk budget | M4 | merged | D11: "no inbound" = no port on a router; a mesh-bound fetch endpoint is allowed. Windows logon task in M17 |
| E-35 | must | machine-readable status | M13 | merged | **landed 2026-10-29 in batch 8** (D215): the document is `status.json` in the state root, with its own `STATUS_VERSION` (Q7), published on change and never on a clock, and `herd-hub status` as the one text screen over it. A document in the state root and not a protocol frame — readable with no connection, no token and no protocol version in hand; reading it takes no lock. Proved end to end against a real hub and a real runner, including that a quiet hub rewrites nothing. M3's health ages are surfaced in it as an instant and not an age (E-40). What M13 still owes is E-37's remainder and the use-against-budget figure, which stays `herd-hub report`'s because it is a walk of the volume |
| E-36 | must | every decision explained | M7 | merged | one `claimed by …` line per placement, on the attempt, with the runner, the labels, the slot figures, any derived `-j` and the preference; verified by counting the lines against the attempts over a simulated busy day. "Nobody could ever run this" ends the job (`impossible`, exit 125 with the labels named); everything else waits, a label nothing offers included (D84). Refusal figures for memory and tmpfs completed in M8 |
| E-37 | must | events routable to ntfy | M13 | part | **Four landed 2026-10-29 in batch 8 (D216) and the fifth on `detached-end-event`, 2026-10-30.** Issue #1's expectations comment names five things and no more — *a detached job's end when the submitter asked, the nightly's verdict, a client lost or joined, an owner switch changed from the owner's side, a job lost twice*, and *"never per step job by default"*, with the acceptance criterion *"a day's ntfy count does not grow with the number of step jobs"*. The sink is one setting — a URL or a command string — with no filter, template or per-event switch, and the payload carries its own `EVENT_FORMAT_VERSION`. **The four of batch 8**: a runner joining, a runner lost by either path, the owner's own switch moving (`asked_for` is remembered so that *"the owner's side"* means something), and E-21's second loss. **The fifth, a detached job's end**, is the submitter's asking carried on `ToHub::SubmitJob` as `announce_end`, through `NewJob`, onto `job.json`, and read back at every door a record reaches a terminal state through — so a nightly cancelled at its deadline and a job out of requeues announce exactly as a verdict does. `herdctl submit --announce-end` is how it is asked for and `herdctl run` is offered no such flag, which is what makes *"detached"* true. **Neither version number moved**: the wire field is absent when nobody asked and the record field is absent when nobody asked, both asserted on the written bytes, which is the condition both constants' own docs set for a field to be additive. The criterion is measured and not argued: `herd-hub/tests/events.rs` runs eight detached submissions of which two ask, and the topic holds two ends. **The row is `part` and not `merged` for one reason only**: *the nightly's verdict* is this same event and needs no event of its own, and what it waits on is M14's nightly existing — `herdctl if-moved` passing the asking through on its submission, one line in `herdctl`, not hub work. Until a scheduled pass can be run end to end there is nothing to demonstrate, so nothing here is promoted on an argument |
| E-38 | should | statistics over a period | — | planned | deferred past M18; raw data already written by M2/M4 records |
| E-39 | should | results searchable | — | planned | deferred past M18; on-disk records are greppable meanwhile |
| E-40 | should | health every 15 s | M3 | merged | the age is in the hub's state, and is **surfaced in the status document since 2026-10-29** (E-35, D215): `last_health_at` and the report count, as an instant and not an age, so a reader computes the age against its own clock |
| E-41 | must | scheduled nightly, exclusive | M14 | merged | now concrete: 03:00 local, daily, only if main moved, 5 h cap, `nightly` exclusive, 05:00 start deadline else skipped with the reason as an event (issue #16); gated: their nightly command, live at M18. **The predicate is the requester's, not the hub's** (D207): `herdctl if-moved` submits a declared command when a named ref has moved since the last run it recorded, under a lock so two overlapping timers cannot both submit, and refuses rather than guess when its state file is missing, unreadable or from another version. The **timer** is the consumer's own, as E-42's hook is — `docs/scheduling.md` says what we ship and what they write. **One deviation, and it is not closeable by hub work:** a skip that submitted nothing is `herdctl`'s own stderr line and never an event, because the hub never heard of the job. The skip E-41 *can* have as an event is the other one — a pass submitted with `--deadline` that nothing started by 05:00, which the hub cancels and reports (E-09, D40). **Landed 2026-10-29 in batch 8** (`herdctl/src/moved.rs`, `herdctl/tests/if_moved.rs`, `docs/scheduling.md`, D217). `merged` here means **Herd's side is on `main` and nothing closeable is left**: the timer is the consumer's by design (D106, D207) and the no-submission skip is `herdctl`'s own stderr line by decision (D217), so this row is delivered with that one deviation on the record rather than closed as E-41 words it |
| E-42 | must | push hook submits producer | M14 | part | Herd side = scriptable detached submit; the hook is theirs; concrete as `dedup_key` `produce:<commit>` and a `coalesce` key `produce:main` (D75, issue #16). **Recounted 2026-10-30, `planned` to `part`, by the orchestrator's ruling: the row is two rules, and the one it names first is on `main` and asserted.** **Dedup — joining — is built:** the hub derives the key over command, labels and the declared commit (`herd-hub/src/job.rs:817`, length-prefixed, the commit a counted third field since D156), and `JobDirStore::submit` looks it up in `open_work` under the store's one lock and joins **any non-terminal job, running included** (`herd-hub/src/store.rs:2234`); asserted against the real binaries in `herdctl/tests/declared_dedup.rs`, whose own doc comment reads *"A push hook firing twice is the case this exists for"*. **Coalesce — discarding — exists nowhere:** no `coalesce` field on `herd_proto::Asked`, no `superseded` job outcome, and the declared `coalesce` and `dedup_key` are parsed and validated in `herdctl/src/declaration.rs` and read nowhere else, so neither declared field reaches the wire — the delivered behaviour rides a key the **hub derives itself**. D75 keeps the two apart, *"Dedup joins; coalescing discards"*, and considers dedup first, so joining a running job is right for dedup and is no evidence for coalesce; coalescing supersedes a queued older `produce`, never a running one. **The clause that the second submission *"says so"* is met by a detached submit returning the existing id** (the orchestrator, 2026-10-30): the hub sends `JobAccepted` and then a `JobNote` carrying the join sentence, but `submit` returns on `JobAccepted` and closes the socket, so the note reaches a connection that is gone; the documented wrapper is `id=$(herdctl submit …)` then `herdctl wait "$id"`, and **the id is the answer**. **No test asserts that silence, deliberately** — asserting a behaviour nobody chose would freeze it. **Whether the coalesce half is built before v1.0.0 is open and Alexa's, and is not decided here:** D75 fixed its shape (`superseded { by }`, exit 125) and not its timing, and building it needs an additive `serde(default)` field on `Asked`, which D229 leaves `PROTOCOL_VERSION` alone for, **and** a new job-outcome variant an older `herdctl` would meet inside a frame it decodes — not literally one of D229's two named unrecoverable forms, but the same hazard. The row carries no `gated` marker and *Gated on chatOnS* below says why |
| E-43 | must | merger jobs start first | M6 | merged | a priority, never a side door; verified against the real binaries: a merger submitted last takes the next free slot. Which jobs a merge submits is theirs (M18) |
| E-44 | must | restart loses nothing | M2 | merged | verified: hub killed mid-job, result lands, record complete. The tail-loss case now holds the job open with a file until the runner has *seen* the link go and writes its own marker when it is over, so "the tail was written while no hub existed" is established rather than assumed from two sleeps (D101) |
| E-45 | must | one data root only | M4 | merged | cooperative confinement only — full containment needs a mount namespace E-46 forbids (D28) |
| E-46 | must | unprivileged, root via helper | M4 | merged |  |
| E-47 | must | server runner runs natively | M16 | merged | the server's runner is a native, unprivileged systemd unit from `deploy/nixos-herd-runner.nix`, no container in its path; read back off the host with `systemctl cat herd-runner` |
| E-48 | must | per-runner approval | M15 | merged | `herd-hub/tests/approval.rs` drives the real binaries: an unapproved runner is turned away and nothing is placed on it, one `herdctl runner approve` lets it run a real job, `remove` takes it back out, and the token an operator was shown is not readable back out of the hub's state |
| E-49 | must | join in thirty minutes | M17 | waived | **Waived by the first consumer for v1.0.0 (D248), on their own condition that it be named in the release notes as not met.** Not *unmet by choice*: D206's premise — that M17 sat outside v1.0.0 — was moved by Alexa on 2026-10-30, and the waiver is the consumer's own answer to the question that replaced it. The timing has never been measured on real hardware and the walk has no automated form. **A measurement is coming and it is not this one:** they will time the **re-enrolment** of the server runner at the v1.0.0 migration, fresh hub and fresh runner, and send duration, steps, version numbers and the first job's id. **They say themselves it is not E-49's new-machine walk, and E-49 is not declared met on it** — they have been told so in writing. Today the measurement is zero, so this is a large improvement that is still not this row. |
| E-50 | must | WSL2 supported platform | M17 | withdrawn | **Withdrawn by the first consumer, not waived, and the difference is load-bearing (D248).** Their words: *"nous n'en avons pas besoin, l'attente a été écrite quand le parc était incertain"* and *"vous pouvez retirer la ligne plutôt que prouver la plateforme"*. **The reason is recorded beside the withdrawal and not only the withdrawal, because without it somebody re-adds WSL2 in six months believing they are closing a gap:** their actual fleet is the NixOS server with its runner in a systemd unit, and the 4060 PC whose runner runs in a dedicated Linux boot (`.#herd-kvm`, via `pc4060-tether.sh`) and **not under Windows**. WSL served only a build-host experiment (HC.5), never Herd, and that path is abandoned — **they need KVM, so native Linux.** No Herd runner has ever run on WSL2, and **in no direction does Herd claim a platform it has not run on** — which is why this row is retired rather than promised. |
| E-51 | must | version pin and refusal | M15 | merged | protocol-version refusal since M1, in one line naming both versions; a state root from a newer Herd is refused the same way, by version and with both figures (D73) |
| E-52 | should | restarts with its host | M16, M17 | part | the server half is merged and read back off the host: a declarative unit whose generation holds its store path, so no garbage collection can take it. The PC's logon task is M17 |
| E-53 | should | records readable during run | M2 | merged | plain files by Q1. **What a reader may see** (D103): `job.json` is the authority on where a job stands now; `events.jsonl` is the history and may be one transition behind it, never ahead. A reader wanting a state *and* its transition line waits on `events.jsonl` |
| E-54 | could | offline re-judging | M2 | planned | E-15's kept files are now written (D131), so a record holds the result file a judge re-reads; the judge is their tool |
| E-55 | could | peer-to-peer artefact exchange | M11 | planned | **promoted to the default** by D10: direct transfer is the only path, the hub never carries bytes |

## Gated on chatOnS

**This is the one list, and it has seven rows.** `docs/plan.md` and the
M18 line below name it rather than restate its members, after four
hand-kept copies disagreed about them (ORCH-206). Each member below names
the deliverable it waits on and quotes the words in its own row above
that gate it, so a reader checks the list against the rows instead of
trusting it.

*Gated* carries this file's opening meaning — the acceptance criterion
needs a chatOnS deliverable, and a test against the fixture consumer
stands in until M18. It says **nothing** about whether Herd's own side is
built: E-14 is gated *and* has nothing on `main`, while E-12 and E-42
are gated *and* partly built, which are two different columns. The
State column and the counted figure at the top of this file are what say
which is which.

- **E-02** — their results.json: *"real verdict gated (M18)"*.
- **E-11** — their declaration file: *"gated: their real declaration
  file (M18)"*.
- **E-12** — their pack: *"gated on their pack (M18)"*.
- **E-14** — their replication: *"gated on their replication (M18)"*.
- **E-20** — their executor and the status document: *"needs their
  executor and the status document (M13, gated M18)"*. **Neither older
  summary carried this row** and its own note does, so `docs/plan.md`'s
  M18 paragraph now names it too: **this list is derived from the rows,
  and the milestone follows them** (the orchestrator, 2026-10-30,
  ORCH-206). A note that gains a `gated M18` marker gains a place here
  and in M18, and never the other way round.
- **E-41** — their nightly command: *"gated: their nightly command, live
  at M18"*, with the one deviation its row records.
- **E-42** — their push hook: its row says *"the hook is theirs"*, and
  the opening paragraph of this file names the push hook among the
  gating deliverables. **The row carries no `gated` marker**, which is
  why a scan of the markers alone misses it: the marker is missing, the
  fact is not.

**Not this list, though one older summary mixed them in with it: a manual
or real-hardware check.** E-30 (*"WSL OOM replay is a manual M17
check"*) and E-49 (*"a manual check on real hardware"*, and waived by
the consumer, D248) wait on somebody walking a machine, not on anything
of the consumer's. A sentence may hold them beside the gated rows only when it
says "gated **or** real-hardware", as the closing line of this file does.

**A second group: gated on them, and with no milestone that closes it.**
These two wait on a chatOnS deliverable like the seven above, but no
milestone closes the wait, so no fixture test "stands in until M18" for
them and they are not M18's to walk. The criterion is in this heading on
purpose: a reader who finds the dependency here, with the reason it is
not in the list above, does not re-add it — and re-adding is how four
copies of that list came to exist (the orchestrator, 2026-10-30,
ORCH-206, preferring this to leaving the two out unexplained).

- **E-38 (should)** — their results.json, for the load-vs-real failure
  split; written out under *Not covered by any milestone* below. The row
  names no milestone and is deferred *past* M18.
- **E-54 (could)** — their judge: *"the judge is their tool"*. The row
  is `planned` and names no closing milestone.

**One row an older list claimed that nothing supports.** E-53 (should):
nothing in its row, and none of the five deliverables named at the top
of this file, needs anything of theirs — it is `merged`, *"plain files
by Q1"* and D103. **Left out**, and said here rather than dropped
quietly; leaving it out was a reading, not a judgement.

## State of play (2026-10-30)

**Thirteen milestones are merged to `main`:** M1 (the substrate, no must
of its own), the scheduler spine M2 to M9, M10 (the command declaration
contract), M11 (its record half, with the pack half part-built and
delivering no must yet, see below), M15 (approval and versions) and M16
(the hub deployed, the server's runner native). **M13
and M14 are not among them and each now has one of its two musts on
`main`**, landed 2026-10-29 in batch 8: E-35 and E-41 (D215, D217),
with E-37 partly built (D216): four of its listed transitions were
emitted and two of its five items were not. **The fifth — a detached
job's end — is built on `detached-end-event`, 2026-10-30**, so what is
left of E-37 is the nightly's verdict, which is that same event and
waits on M14's nightly existing rather than on any further hub work.

**36 of 44 musts are wholly on `main`.** Of the eight that are not:
**five** are partly built (E-16, whose hub-side budget shipped in M11;
E-37; M11's two pack musts E-12 and E-17, recounted on 2026-10-30; and
E-42, whose dedup half is built and asserted and whose coalesce half is
absent, recounted on 2026-10-30); **one** has nothing on `main`
(E-14); and two the consumer has released us from — **E-49 waived**,
**E-50 withdrawn** (D248). Four `should`
rows are merged — E-09 and E-10 beside M9, E-40 beside M3, E-53 beside
M2 — and E-52 is half of one. Twelve musts have had an independent
adversarial pass against the real binaries (E-03, E-15, E-44 from M2;
E-06, E-21 from M3; E-30, E-34, E-45, E-46 from M4; E-24, E-26, E-28
from M5); the musts of M6 through M16 are merged and tested, but have
not had that pass.

Five things a reader must not mistake for done:

1. **Merged is not running.** **The newest published release is
   v0.4.1** (`docs/status.md`; this line said v0.3.0 until 2026-10-30
   and was two releases stale). What the consumer's machines actually
   run is a fact that reaches us only as a dated report (D161), and the
   last one in the tree is `docs/handover.md`'s — pin `v0.3.2`, the
   live unit `herd-runner-0.3.2`, the hub container `herd-hub:0.3.3`,
   with D163, D164 and D165 shipped in v0.3.4 and reaching nothing
   until that pin moves. **The figures are not restated here**, because
   two documents disagreeing about their fleet is how this line went
   stale: `docs/status.md` carries the live figures, the traps in the
   deployed runner, and who operates what.
2. **M11 is merged, E-12 and E-17 are not delivered, and the pack half
   is now partly in the tree.** The record half shipped first: a job's
   files come home, large ones stream, and the hub sweeps them against
   its own budget (D148, D150, D152). The pack half — frames on both
   protocol enums, the hub's registry, the runner's content store, a
   peer fetch and a mesh-bound endpoint, and a runner that announces
   what it holds — landed in batches 12 to 16, and it is **wired at the
   edges and not through**: no job is assigned a pack and no pack a job
   leaves is registered (ORCH-234). That is why both rows now read
   `part` and why neither reads `merged`.
3. **E-45 is cooperative confinement only** (D28): a job can write
   anything the runner's user can. Full containment needs a mount
   namespace E-46 forbids the runner the privilege for. On the
   deployment host that user is also in the `docker` group,
   root-equivalent through the container socket, and the runner says so
   at startup; it has been raised and settled (D78). The runner needs
   no Docker itself, and taking the user out of that group is a change
   on the host its operator makes.
4. **E-30's real kernel ceiling is only asserted under a delegated
   cgroup.** Without delegation the runner falls back to `RLIMIT_AS`,
   weaker in three named ways (D23). That is most of why the suite
   reports **12** loud skips on a host with no delegated subtree on a
   block device (D27): every one of the twelve is a cgroup-delegation
   or ZFS `io.max` assertion. E-30's WSL replay stays a manual M17
   check.
5. **The three format numbers have all moved, and this paragraph gave
   the older figures until 2026-10-30.** Read off the tree at
   `26e83cf`: `PROTOCOL_VERSION` is **3**
   (`herd-proto/src/lib.rs`), `RECORD_VERSION` **2**
   (`herd-hub/src/job.rs`) and `LAYOUT_VERSION` **2**
   (`herd-hub/src/store.rs`); every release to date carries 2, 1 and 1
   (`docs/handover.md`). A peer or a state root from another version is
   refused by name with both figures and there is no migration (D73),
   so a fleet crosses a protocol line only by a deliberate upgrade of
   every machine in it. **D199's three moves are therefore spent**, all
   three in M11's pack half — 2 → 3, 1 → 2, 1 → 2 — and because there
   is no migration path the last two destroy an existing state root as
   a working root. The sentence this replaces also said the ordinary
   release before v1.0.0 ships at layout 1 and record 1: no release cut
   from `main` can now do that. **What followed for the release is
   settled: the next tag is v1.0.0** (D212), so there is no ordinary
   release before it for that sentence to bind — D243 ruled the
   constraint moot rather than open, and ORCH-156 is closed as caduque.

## Musts per milestone

| Milestone | Musts | Which |
|---|---|---|
| M2 | 3 | E-03, E-15, E-44 |
| M3 | 2 | E-06, E-21 |
| M4 | 4 | E-30, E-34, E-45, E-46 |
| M5 | 3 | E-24, E-26, E-28 |
| M6 | 4 | E-05, E-18, E-20, E-43 |
| M7 | 4 | E-19, E-22, E-25, E-36 |
| M8 | 4 | E-04, E-29, E-31, E-32 |
| M9 | 3 | E-07, E-27, E-33 |
| M10 | 4 | E-01, E-02, E-08, E-11 |
| M11 | 2 | E-12, E-17 |
| M12 | 2 | E-14, E-16 |
| M13 | 2 | E-35, E-37 |
| M14 | 2 | E-41, E-42 |
| M15 | 2 | E-48, E-51 |
| M16 | 1 | E-47 |
| M17 | 2 | E-49, E-50 |

**Total: 44 of 44 musts placed**, across **16** must-bearing milestones.
*Placed* is where each must's work belongs and not whether it is done: the
done figure is **36 of 44** and it is counted at the top of this file. M1
and M18 carry no must of their own: M1 is the substrate every must builds
on; M18 closes the acceptances of *Gated on chatOnS* above — the one
list, derived from the rows — against chatOnS's real deliverables. This
line restated six of them until 2026-10-30 and was one of the three
disagreeing copies (ORCH-206).

## Not covered by any milestone

- **E-38 (should)** and **E-39 (should)** — statistics and search. Deferred
  until after M18 on purpose: they only read records that M2/M4 already write,
  so nothing is lost by waiting, and no must depends on them. E-38 is also
  gated on chatOnS's results.json for the load-vs-real failure split.

Every must is placed, and three kinds of must are **not** simply owed:
five are partial (E-12, E-16, E-17, E-37, E-42), two the consumer has
released us from (**E-49 waived**, **E-50 withdrawn**, D248), and
nine carry a gated or real-hardware note that names its closing
milestone — the seven of *Gated on chatOnS* above, plus E-30 and E-49,
whose checks are manual and wait on nothing of the consumer's. This
figure read **eight** while E-42 was uncounted, its row naming the
deliverable without the word `gated`.
A reader must not read a placed must as a delivered one; the State column
and the counted figure at the top of this file are what say which is which.
