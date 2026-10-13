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
| E-04 | must | waits explained with figures | M8 | planned |  |
| E-05 | must | identical submissions deduplicated | M6 | in review (PR #15) | keyed on the command and the labels, length-prefixed (D69); only an open job is joined — work that is over is rerun (D70); the higher level wins; verified: two waits, one execution, one log |
| E-06 | must | cancel kills process tree | M3 | merged | propagation and the sibling-untouched case asserted in M3; the kill becomes a process-tree kill in M4 |
| E-07 | must | dry run shows placement | M9 | planned |  |
| E-08 | must | refuse dirty or unpushed | M10 | planned |  |
| E-09 | should | deadline cancels unstarted job | M9 | planned | pulled forward: E-41 (must) needs it |
| E-10 | should | depend on one other job | M9 | planned | capped: one parent, no conditions (invariant 1) |
| E-11 | must | consumer-declared commands, nix develop | M10 | planned | the field set, `[executor]` hooks and ten real commands are now known in full, including `labels_from` (D74) and the step timestamps (D77) (issue #16); gated: their real declaration file (M18) |
| E-12 | must | packs by (commit, key), no shared build trees | M11 | planned | D10: hub registers, peers transfer; the real shape (`manifest.tsv`, `pack.meta`, `objects/<sha256>`, id = sha256 of the manifest) and measured figures (~400 MiB, ~560 files, verified under 2 s) are now known (issue #16); gated on their pack (M18) |
| E-13 | should | content-addressed, complete or absent | M11 | planned | done with the store: retrofitting CAS would force a migration, and D10 needs it for free diffs |
| E-14 | must | warm caches ready before job | M12 | planned | Q5/D9: no cache engine in Herd; their own words, "you move packs; our executor owns warm caches" (issue #16), shrink M12 to moving packs, key-aware placement and GC/budgets — the cache-specific prepare step is dropped for the generic step timestamps (D77); gated on their replication (M18) |
| E-15 | must | full job record on disk | M2 | merged | byte-faithful logs (D18); a lost log chunk leaves a named marker, and a lost tail is asked for (D36) |
| E-16 | must | budgets, gc with pinning | M12 | planned | per runner, not central (D10); keepers pin |
| E-17 | must | atomic publication everywhere | M11 | planned | a manifest registers only once every blob is held; job-record atomicity already in M2 (Q1) |
| E-18 | must | four priorities with aging | M6 | in review (PR #15) | aging computed, not written (D67); default step 30 min, so the lowest level reaches the top in 90 min; the raise is upwards only (D72); the 90-min figure itself is a simulated-time and 6-second-real-clock assertion, not a measurement on a busy day |
| E-19 | must | no fallback while available | M7 | in review (PR #18) | preference is one declared integer per runner (D81); a draining runner does not count as available (D80); the limit is `--stickiness-limit`, 3 h, printed at startup and in every waiting sentence; verified against two real runners: the second build never moved while the PC was merely busy, and moved in well under 10 s when it died. Switch-off case retested with E-33 in M9 |
| E-20 | must | one queue for everything | M6 | in review (PR #15) | the queue is the unfinished job directories; one dispatch pass, which a submission goes through like everything else; no lane for a merger. "Every QEMU on every client listed as a job" needs their executor and the status document (M13, gated M18) |
| E-21 | must | lost runner, requeue once | M3 | merged | silence threshold is hub policy; a second loss ends the job `lost`; verified: two attempts, never a third, across six alternating runner lives |
| E-22 | must | build slots, derived -j | M7 | in review (PR #18) | slots are read from the records every pass and counted nowhere, so none can leak and none is rebuilt after a restart (D79); `-j = min(threads, 3/4 of memory / 1792 MiB)` per slot, rounded down, never 0 (D83) — the owner's 17 GiB PC derives E-22's own `-j 7`; the figure is on the record before the job is handed over (D82) and reaches the process through one `{j}` token. Verified: a `kill -9` mid-job gives the slot back, a hub restart does not double-book it. **The memory figure is `MemTotal` unless the owner says otherwise** — whole-job memory admission is M8's (E-29) |
| E-23 | should | placement prefers cache locality | M12 | planned | preference only; E-19 governs waiting |
| E-24 | must | probed/declared/denied labels | M5 | merged | the offer and its origins; matching them at claim time is M7 |
| E-25 | must | capability kinds and exclusion | M7 | in review (PR #18) | algebra and closure landed in M5 (symmetric, not transitive); enforced at claim time in M7, with the seats carried through a pass so two jobs offered together contend. Verified against the real binaries: two counted jobs serialise on one slot and an exclusive one overlaps neither, including a job that holds no label at all. Note for the consumer: on their declaration `gpu-venus excludes igpu-vfio` is **inert**, because `igpu-vfio` is exclusive and host-wide already blocks everything — what keeps the two apart when something outside Herd holds the iGPU is the busy probe (E-26) |
| E-26 | must | externally busy capability | M5 | merged | a probe nobody can read suspends too; an unknown *or duplicated* key in the capability file refuses the runner's startup; wait-reason wording polished by M8 |
| E-27 | must | metal lease, drain first | M9 | planned | lease model lands in M5 as counted(1) plus a probe; drain-first in M9 |
| E-28 | must | reference label carried | M5 | merged | a capability like any other; placement preferring it last is M12 |
| E-29 | must | memory admission with floor | M8 | planned |  |
| E-30 | must | kernel memory ceiling | M4 | merged | WSL OOM replay is a manual M17 check; real ceiling asserted only under a delegated cgroup; RLIMIT_AS fallback per D23 |
| E-31 | must | tmpfs reservation, wait | M8 | planned |  |
| E-32 | must | host-wide bound, load recorded | M8 | planned | p90 < 12 figure read after adoption (M18+) |
| E-33 | must | owner switch in 10 s | M9 | planned | desktop shortcut on the real PC in M17 |
| E-34 | must | niced, no inbound, disk budget | M4 | merged | D11: "no inbound" = no port on a router; a mesh-bound fetch endpoint is allowed. Windows logon task in M17 |
| E-35 | must | machine-readable status | M13 | planned | format versioned per Q7 |
| E-36 | must | every decision explained | M7 | in review (PR #18) | one `claimed by …` line per placement, on the attempt, with the runner, the labels, the slot figures, any derived `-j` and the preference; verified by counting the lines against the attempts over a simulated busy day. "Nobody could ever run this" ends the job (`impossible`, exit 125 with the labels named); everything else waits, a label nothing offers included (D84). Refusal figures for memory and tmpfs completed in M8 |
| E-37 | must | events routable to ntfy | M13 | planned |  |
| E-38 | should | statistics over a period | — | planned | deferred past M18; raw data already written by M2/M4 records |
| E-39 | should | results searchable | — | planned | deferred past M18; on-disk records are greppable meanwhile |
| E-40 | should | health every 15 s | M3 | merged | the age is in the hub's state; surfaced in the status document in M13 |
| E-41 | must | scheduled nightly, exclusive | M14 | planned | now concrete: 03:00 local, daily, only if main moved, 5 h cap, `nightly` exclusive, 05:00 start deadline else skipped with the reason as an event (issue #16); gated: their nightly command, live at M18 |
| E-42 | must | push hook submits producer | M14 | planned | Herd side = scriptable detached submit; the hook is theirs; now concrete: `dedup_key` `produce:<commit>`, `coalesce` key `produce:main` superseding a queued older `produce`, never a running one (D75, issue #16) |
| E-43 | must | merger jobs start first | M6 | in review (PR #15) | a priority, never a side door; verified against the real binaries: a merger submitted last takes the next free slot. Which jobs a merge submits is theirs (M18) |
| E-44 | must | restart loses nothing | M2 | merged | verified: hub killed mid-job, result lands, record complete |
| E-45 | must | one data root only | M4 | merged | cooperative confinement only — full containment needs a mount namespace E-46 forbids (D28) |
| E-46 | must | unprivileged, root via helper | M4 | merged |  |
| E-47 | must | server runner runs natively | M16 | planned |  |
| E-48 | must | per-runner approval | M15 | planned |  |
| E-49 | must | join in thirty minutes | M17 | planned | the timing is a manual check on real hardware |
| E-50 | must | WSL2 supported platform | M17 | planned |  |
| E-51 | must | version pin and refusal | M15 | planned | protocol-version refusal exists since M1 |
| E-52 | should | restarts with its host | M16, M17 | planned | server in M16, the PC's logon task in M17 |
| E-53 | should | records readable during run | M2 | merged | plain files by Q1 |
| E-54 | could | offline re-judging | M2 | planned | no Herd work beyond E-15's kept files; the judge is their tool |
| E-55 | could | peer-to-peer artefact exchange | M11 | planned | **promoted to the default** by D10: direct transfer is the only path, the hub never carries bytes |

## State of play (2026-10-13)

Six milestones are merged to `main`: M1 (substrate, no must of its
own), M2, M3, M4, M5 (the capability model) and M6 (the queue and its
priorities). **12 of 44 musts are merged and verified** against the real
binaries by an independent adversarial pass (E-03, E-15, E-44 from M2;
E-06, E-21 from M3; E-30, E-34, E-45, E-46 from M4; E-24, E-26, E-28
from M5); M6's four (E-05, E-18, E-20, E-43) are merged but have not had
that pass. M2 and M3 also carry two `should` rows merged alongside them
(E-53, E-40). Four more musts are written and in review, not merged:
E-19, E-22, E-25, E-36 (M7, matching and slots). The remaining 24 musts
are planned.

Five things a reader must not mistake for done:

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
   now warns about this at startup, and it is the first thing to raise
   with the first consumer.
3. **E-30's real kernel ceiling is only asserted under a delegated
   cgroup**; without delegation the runner falls back to `RLIMIT_AS`,
   weaker in three named ways (D23). The WSL replay on real hardware
   is still a manual M17 check.
4. **Nothing is deployed, so every format is still free to change.**
   M7 moved both again: `PROTOCOL_VERSION` is **3** and
   `RECORD_VERSION` is **3**. A runner now announces how big its host is
   and how preferred it is, a submission may carry the memory one build
   thread needs, an attempt carries the line explaining its placement
   and a record carries the `-j` derived for it — all required rather
   than defaulted, for the reason D71 gives. A state root written before
   M7 is refused by name with both figures; there is no migration and
   none is owed (D73). They freeze at M16.
5. **A fleet, with no authentication.** The hub holds many runners from
   M7, keyed on the name an attempt belongs to. That makes point 1 above
   worse rather than better: a hand-crafted peer can now join *beside*
   the real runners under any name it likes. Loopback only until M15
   still holds.

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
