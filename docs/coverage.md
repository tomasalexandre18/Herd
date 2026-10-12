# Coverage — expectations E-01..E-55 → milestones

Derived from issue #1's expectations sheet (2026-10-12). Recounted priorities:
**44 must, 9 should, 2 could.** "Gated (M18)" means the acceptance criterion
needs a chatOnS deliverable (their declaration file, `cargo xtask pack` /
`test --pack`, results.json, push hook or nightly command); until then an
automated test against the fixture consumer stands in, and the real check
closes in M18.

| E | Prio | What | Milestone | Note |
|---|---|---|---|---|
| E-01 | must | submit by commit, never machine | M10 | |
| E-02 | must | ran-on line, verdict unchanged | M10 | format frozen per Q7; real verdict gated (M18) |
| E-03 | must | stream, detached mode, wait | M2 | foreground streaming itself lands in M1 |
| E-04 | must | waits explained with figures | M8 | |
| E-05 | must | identical submissions deduplicated | M6 | |
| E-06 | must | cancel kills process tree | M3 | kill mechanics hardened in M4 |
| E-07 | must | dry run shows placement | M9 | |
| E-08 | must | refuse dirty or unpushed | M10 | |
| E-09 | should | deadline cancels unstarted job | M9 | pulled forward: E-41 (must) needs it |
| E-10 | should | depend on one other job | M9 | capped: one parent, no conditions (invariant 1) |
| E-11 | must | consumer-declared commands, nix develop | M10 | gated: their real declaration file (M18) |
| E-12 | must | packs by (commit, key), no shared build trees | M11 | D10: hub registers, peers transfer; gated on their pack (M18) |
| E-13 | should | content-addressed, complete or absent | M11 | done with the store: retrofitting CAS would force a migration, and D10 needs it for free diffs |
| E-14 | must | warm caches ready before job | M12 | Q5/D9: no cache engine in Herd, but a declared prepare step, key-aware placement, and cache diffs publishable as packs; gated on their replication (M18) |
| E-15 | must | full job record on disk | M2 | resource metrics in M4, retention in M12 |
| E-16 | must | budgets, gc with pinning | M12 | per runner, not central (D10); keepers pin |
| E-17 | must | atomic publication everywhere | M11 | a manifest registers only once every blob is held; job-record atomicity already in M2 (Q1) |
| E-18 | must | four priorities with aging | M6 | |
| E-19 | must | no fallback while available | M7 | switch-off case retested with E-33 in M9 |
| E-20 | must | one queue for everything | M6 | |
| E-21 | must | lost runner, requeue once | M3 | |
| E-22 | must | build slots, derived -j | M7 | |
| E-23 | should | placement prefers cache locality | M12 | preference only; E-19 governs waiting |
| E-24 | must | probed/declared/denied labels | M5 | |
| E-25 | must | capability kinds and exclusion | M7 | model in M5; contention enforcement tested in M7 |
| E-26 | must | externally busy capability | M5 | wait-reason wording polished by M8 |
| E-27 | must | metal lease, drain first | M9 | lease model in M5; drain-first in M9 |
| E-28 | must | reference label carried | M5 | |
| E-29 | must | memory admission with floor | M8 | |
| E-30 | must | kernel memory ceiling | M4 | WSL OOM replay is a manual M17 check |
| E-31 | must | tmpfs reservation, wait | M8 | |
| E-32 | must | host-wide bound, load recorded | M8 | p90 < 12 figure read after adoption (M18+) |
| E-33 | must | owner switch in 10 s | M9 | desktop shortcut on the real PC in M17 |
| E-34 | must | niced, no inbound, disk budget | M4 | Windows logon task in M17 |
| E-35 | must | machine-readable status | M13 | format versioned per Q7 |
| E-36 | must | every decision explained | M7 | refusal figures completed in M8 |
| E-37 | must | events routable to ntfy | M13 | |
| E-38 | should | statistics over a period | — | deferred past M18; raw data already written by M2/M4 records |
| E-39 | should | results searchable | — | deferred past M18; on-disk records are greppable meanwhile |
| E-40 | should | health every 15 s | M3 | ages surfaced in status in M13 |
| E-41 | must | scheduled nightly, exclusive | M14 | gated: their nightly command, live at M18 |
| E-42 | must | push hook submits producer | M14 | Herd side = scriptable detached submit; the hook is theirs |
| E-43 | must | merger jobs start first | M6 | |
| E-44 | must | restart loses nothing | M2 | re-asserted across reconnects in M3 |
| E-45 | must | one data root only | M4 | |
| E-46 | must | unprivileged, root via helper | M4 | |
| E-47 | must | server runner runs natively | M16 | |
| E-48 | must | per-runner approval | M15 | |
| E-49 | must | join in thirty minutes | M17 | the timing is a manual check on real hardware |
| E-50 | must | WSL2 supported platform | M17 | |
| E-51 | must | version pin and refusal | M15 | protocol-version refusal exists since M1 |
| E-52 | should | restarts with its host | M16, M17 | server in M16, the PC's logon task in M17 |
| E-53 | should | records readable during run | M2 | plain files by Q1 |
| E-54 | could | offline re-judging | M2 | no Herd work beyond E-15's kept files; the judge is their tool |
| E-55 | could | peer-to-peer artefact exchange | M11 | **promoted to the default** by D10: direct transfer is the only path, the hub never carries bytes |

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
