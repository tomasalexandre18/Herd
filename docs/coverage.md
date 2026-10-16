# Coverage — expectations E-01..E-55 → milestones

Derived from issue #1's expectations sheet (2026-10-12). Recounted priorities:
**44 must, 9 should, 2 could.** "Gated (M18)" means the acceptance criterion
needs a chatOnS deliverable (their declaration file, `cargo xtask pack` /
`test --pack`, results.json, push hook or nightly command); until then an
automated test against the fixture consumer stands in, and the real check
closes in M18.

| E | Prio | What | Milestone | State | Note |
|---|---|---|---|---|---|
| E-01 | must | submit by commit, never machine | M10 | planned |  |
| E-02 | must | ran-on line, verdict unchanged | M10 | planned | format frozen per Q7; results.json's real fields and a worked example are now known (issue #16); real verdict gated (M18) |
| E-03 | must | stream, detached mode, wait | M2 | merged | foreground streaming itself lands in M1 |
| E-04 | must | waits explained with figures | M8 | merged | the sentence carries the position, **what is ahead** (the jobs in front by id and level, three named and the rest counted) and why, with the figures E-04 asks for — `1.2 GiB free in tmpfs, 2 GiB needed` comes out of one formatter and is asserted in those words against the real binaries. Said again on its own clock, `--wait-restatement`, 60 s by default and printed at startup: the record keeps M6's news-only rule, the stream gets at most `wait / cadence + 1` lines, which is **eleven over ten minutes** and not one per dispatch pass (D91). Both failure modes are tested by mutation — removing the clock gives 1 line over 20 cadences, restating per pass gives 79 against a bound of 22 |
| E-05 | must | identical submissions deduplicated | M6 | merged | keyed on the command and the labels, length-prefixed (D69); only an open job is joined — work that is over is rerun (D70); the higher level wins; verified: two waits, one execution, one log |
| E-06 | must | cancel kills process tree | M3 | merged | propagation and the sibling-untouched case asserted in M3; the kill becomes a process-tree kill in M4. The 'output produced before the cancel survives' half now waits for that output to exist before cancelling: on two cores the kill beat the job's first write in one run of six, and the test read a job that printed nothing as output lost (D101) |
| E-07 | must | dry run shows placement | M9 | merged | `herdctl would-run` lists the eligible runners with the labels each answers for, what is queued ahead, and the verdict the claim step gives, out of **one** dispatch pass that is thrown away: the same `matcher::place`, the same `queue::order`, the jobs ahead of it placed against the same seats so the slot it would really find is the slot it reports. Nothing is minted (the hypothetical record carries an id this hub cannot mint), nothing is written, no slot and no reservation taken — verified by comparing **every byte of the state root** before and after, path by path, against a quiescent hub with one build already holding the only slot. Also verified not to lie: the real submission of the same job waits for the same reason |
| E-08 | must | refuse dirty or unpushed | M10 | planned |  |
| E-09 | should | deadline cancels unstarted job | M9 | merged | pulled forward: E-41 (must) needs it, **and** it is D88's only bound on a job no connected runner could ever fit. An absolute instant in Unix seconds, resolved from `--deadline +15m` or RFC 3339 by the CLI, judged on the hub's clock, recorded on the record (D94). It bounds waiting only: a job that has **started** is never touched by it (that is M4's time cap, exit 124), asserted with a job whose deadline passes while it runs and which finishes 0. A queued job past it ends `cancelled (deadline)`, exit 125 (D40), and the sentence names what it was waiting for when the moment came — so a job cancelled because its machine was switched off says `she is gaming` rather than only `deadline`. Verified: past at submission, past across a hub restart, and firing with no runner connected at all |
| E-10 | should | depend on one other job | M9 | merged | capped: one parent, no conditions (invariant 1). Start after its **success**, which is exit 0 and nothing else; every other ending — non-zero, signalled, could-not-start, stopped-by-cap, cancelled, lost, impossible — ends the child `cancelled (parent)` naming the parent and how it ended (D95). A cycle is **unrepresentable** rather than checked: a parent must already exist, ids are minted in order, so a job can only name one older than itself — no job can name itself and no two can name each other, verified by guessing the next id and being refused. A parent this hub cannot read is never read as one that succeeded |
| E-11 | must | consumer-declared commands, nix develop | M10 | planned | the field set, `[executor]` hooks and ten real commands are now known in full, including `labels_from` (D74) and the step timestamps (D77) (issue #16); gated: their real declaration file (M18) |
| E-12 | must | packs by (commit, key), no shared build trees | M11 | planned | D10: hub registers, peers transfer; the real shape (`manifest.tsv`, `pack.meta`, `objects/<sha256>`, id = sha256 of the manifest) and measured figures (~400 MiB, ~560 files, verified under 2 s) are now known (issue #16); gated on their pack (M18). **D148 holds the line this row is on**: a pack is read next by a *job*, so it never touches the hub; a record is read next by a *requester*, so it does. M11 builds both and splits record-first if it overflows, with both halves' frames designed in one move to `PROTOCOL_VERSION` 2, approved 2026-10-16, because a break is paid by the deployment and must not be paid twice |
| E-13 | should | content-addressed, complete or absent | M11 | planned | done with the store: retrofitting CAS would force a migration, and D10 needs it for free diffs |
| E-14 | must | warm caches ready before job | M12 | planned | Q5/D9: no cache engine in Herd; their own words, "you move packs; our executor owns warm caches" (issue #16), shrink M12 to moving packs, key-aware placement and GC/budgets — the cache-specific prepare step is dropped for the generic step timestamps (D77); gated on their replication (M18) |
| E-15 | must | full job record on disk | M2 | merged | byte-faithful logs (D18); a lost log chunk leaves a named marker, and a lost tail is asked for (D36). **Kept files are real** (D131): what a job leaves under `{out}` is collected before the workspace goes and written into the record's `files/` directory, capped at 32 MiB with a refusal carrying figures over it. **D148 keeps 32 MiB and changes what it means**: it stops being a refusal boundary and becomes the line between the inline carriage (the verdict frame, as today) and the streamed one (M11), so a requester never declares which it needs and cannot tell which it got. Retention of the record itself — 14/90 days by priority class — is M12's; so is the separate budget for a job's large files. One latent defect closed on the way (D148): `FromHub::JobLog` sent a job's whole log in one frame, so a log over ~48 MiB built a frame past the ceiling and a `herdctl wait` got exit 125 with no verdict — the backlog now travels in 8 MiB frames, no protocol move |
| E-16 | must | budgets, gc with pinning | M12 | planned | per runner, not central (D10); keepers pin. D148 adds the hub's **own** budget beside them, for the large files a record carries, with a floor in days and a named line when eviction breaks it; keepers pin packs only, because a record has left its runner by the time the job is reported finished |
| E-17 | must | atomic publication everywhere | M11 | planned | a manifest registers only once every blob is held; job-record atomicity already in M2 (Q1) |
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
| E-35 | must | machine-readable status | M13 | planned | format versioned per Q7 |
| E-36 | must | every decision explained | M7 | merged | one `claimed by …` line per placement, on the attempt, with the runner, the labels, the slot figures, any derived `-j` and the preference; verified by counting the lines against the attempts over a simulated busy day. "Nobody could ever run this" ends the job (`impossible`, exit 125 with the labels named); everything else waits, a label nothing offers included (D84). Refusal figures for memory and tmpfs completed in M8 |
| E-37 | must | events routable to ntfy | M13 | planned |  |
| E-38 | should | statistics over a period | — | planned | deferred past M18; raw data already written by M2/M4 records |
| E-39 | should | results searchable | — | planned | deferred past M18; on-disk records are greppable meanwhile |
| E-40 | should | health every 15 s | M3 | merged | the age is in the hub's state; surfaced in the status document in M13 |
| E-41 | must | scheduled nightly, exclusive | M14 | planned | now concrete: 03:00 local, daily, only if main moved, 5 h cap, `nightly` exclusive, 05:00 start deadline else skipped with the reason as an event (issue #16); gated: their nightly command, live at M18 |
| E-42 | must | push hook submits producer | M14 | planned | Herd side = scriptable detached submit; the hook is theirs; now concrete: `dedup_key` `produce:<commit>`, `coalesce` key `produce:main` superseding a queued older `produce`, never a running one (D75, issue #16) |
| E-43 | must | merger jobs start first | M6 | merged | a priority, never a side door; verified against the real binaries: a merger submitted last takes the next free slot. Which jobs a merge submits is theirs (M18) |
| E-44 | must | restart loses nothing | M2 | merged | verified: hub killed mid-job, result lands, record complete. The tail-loss case now holds the job open with a file until the runner has *seen* the link go and writes its own marker when it is over, so "the tail was written while no hub existed" is established rather than assumed from two sleeps (D101) |
| E-45 | must | one data root only | M4 | merged | cooperative confinement only — full containment needs a mount namespace E-46 forbids (D28) |
| E-46 | must | unprivileged, root via helper | M4 | merged |  |
| E-47 | must | server runner runs natively | M16 | planned |  |
| E-48 | must | per-runner approval | M15 | planned |  |
| E-49 | must | join in thirty minutes | M17 | planned | the timing is a manual check on real hardware |
| E-50 | must | WSL2 supported platform | M17 | planned |  |
| E-51 | must | version pin and refusal | M15 | planned | protocol-version refusal exists since M1 |
| E-52 | should | restarts with its host | M16, M17 | planned | server in M16, the PC's logon task in M17 |
| E-53 | should | records readable during run | M2 | merged | plain files by Q1. **What a reader may see** (D103): `job.json` is the authority on where a job stands now; `events.jsonl` is the history and may be one transition behind it, never ahead. A reader wanting a state *and* its transition line waits on `events.jsonl` |
| E-54 | could | offline re-judging | M2 | planned | E-15's kept files are now written (D131), so a record holds the result file a judge re-reads; the judge is their tool |
| E-55 | could | peer-to-peer artefact exchange | M11 | planned | **promoted to the default** by D10: direct transfer is the only path, the hub never carries bytes |

## State of play (2026-10-13)

Nine milestones are merged to `main`: M1 (substrate, no must of its
own), M2, M3, M4, M5 (the capability model), M6 (the queue and its
priorities), M7 (matching, slots and stickiness), M8 (admission and
explained waits) and M9 (owner control). **27 of 44 musts are
merged**; 12 are verified against the real binaries by an independent
adversarial pass (E-03, E-15, E-44 from M2; E-06, E-21 from M3; E-30,
E-34, E-45, E-46 from M4; E-24, E-26, E-28 from M5). M6's four (E-05,
E-18, E-20, E-43), M7's four (E-19, E-22, E-25, E-36), M8's four
(E-04, E-29, E-31, E-32) and M9's three (E-07, E-27, E-33) are merged
but have not had that pass. Four `should` rows are merged too: E-53
and E-40 beside M2 and M3, E-09 and E-10 beside M9. The remaining 17
musts are planned.

Six things a reader must not mistake for done:

1. **Authentication does not exist until M15.** A runner's name is
   self-asserted, so a hand-crafted peer reaching the hub's port can
   impersonate a runner and write a false verdict onto its job while
   the real runner is down. The hub listens on loopback by default
   (D14) and the ownership gate is not a credential (D57). **Herd must
   not be exposed beyond loopback before M15.**
2. **E-45 is cooperative confinement only** (D28): a job can write
   anything the runner's user can. Full containment needs a mount
   namespace E-46 forbids the runner the privilege for. On the
   deployment host the runner's user is additionally in the `docker`
   group, root-equivalent through the container socket — the runner
   warns about this at startup. It has been raised and settled: the
   consumer required it in issue #16 and Herd agreed without
   reservation (D78). The runner needs no Docker at all; three
   checkable commitments land in M16, and taking the user out of the
   group is a change on the host that the owner makes.
3. **E-30's real kernel ceiling is only asserted under a delegated
   cgroup**; without delegation the runner falls back to `RLIMIT_AS`,
   weaker in three named ways (D23). The WSL replay on real hardware
   is still a manual M17 check.
4. **Nothing is deployed, so every format is still free to change.**
   M8 moved both again: `PROTOCOL_VERSION` is **4** and
   `RECORD_VERSION` is **4**. A runner now announces the memory it
   keeps for itself and the tmpfs room jobs may reserve on it, and a
   submission may carry the whole-job memory and tmpfs a requester
   declares — all required rather than defaulted, for the reason D71
   gives. A state root written before M8 is refused by name with both
   figures; there is no migration and none is owed (D73). They freeze
   at M16.
5. **A fleet, with no authentication.** The hub holds many runners from
   M7, keyed on the name an attempt belongs to. That makes point 1 above
   worse rather than better: a hand-crafted peer can now join *beside*
   the real runners under any name it likes. Loopback only until M15
   still holds.
6. **E-22's exact `-j 7` is now host-dependent.** It is asserted only
   on a host with a real 17 GiB of memory, and skipped with its reason
   elsewhere (D27, D89), so CI shows **three** skips rather than two —
   GitHub Actions gives the runner 15 989 MiB. What is asserted on
   every host is stronger than before: the figure on the record, the
   figure in the explanatory line and the figure the process was given
   are one value. The exact figure itself is no longer checked by CI.

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

**Total: 44 of 44 musts placed.** M1 and M18 carry no must of their own: M1 is
the substrate every must builds on; M18 closes the gated acceptances (E-02,
E-11, E-12, E-14, E-41, E-42 above) against chatOnS's real deliverables.

## Not covered by any milestone

- **E-38 (should)** and **E-39 (should)** — statistics and search. Deferred
  until after M18 on purpose: they only read records that M2/M4 already write,
  so nothing is lost by waiting, and no must depends on them. E-38 is also
  gated on chatOnS's results.json for the load-vs-real failure split.

Every must is placed; no must is deferred or partial beyond the gated and
real-hardware notes in the table, each of which names its closing milestone.
