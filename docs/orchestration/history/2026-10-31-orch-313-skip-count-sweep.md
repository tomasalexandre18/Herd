# orch-313-skip-count-sweep — handoff 2026-10-31

- **Item:** ORCH-313 (re-opened and closed wider), ORCH-048, ORCH-070,
  one `ORCH-NEXT` row
- **Agent:** herd-step
- **Branch / commit:** `orch-313-skip-count-sweep` / see the agent's
  report, based on `main` `9f76b548678208592be6e58b01643a5729ab188b`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**The defect, stated once.** D187 says the skip list is compared as a
list and never as a count. Several of the files that *state* that rule
counted in their own prose, so batch 31's thirteenth name
(`a_tmpfs_byte_a_job_writes_is_charged_to_its_own_memory_ceiling`)
falsified them. ORCH-313 was opened for this and closed on the two
sentences its own Item cell enumerated. The live population was larger.

**Eighteen sentences in seven files** now name the list or derive the
figure instead of carrying a hand-maintained total. Twelve was not
rewritten as thirteen anywhere: that reproduces the defect one landing
later, which is exactly what happened here.

| file | what it said | what it says |
|---|---|---|
| `scripts/gate.sh:24`-`:26` | *"this host skips 12 at rest and 14 under load"*, inside **rule 1**, the rule that says a count is not a signal | the expected names are in `scripts/expected-skips/`, and no count taken off those files means anything |
| `scripts/test.sh:21`-`:24` | *"a complete run reports 12"*, and *"a runner that reports 15 when green"* | a complete run skips every name in `local.txt`; CI's runner has a skip list of its own (`ci.txt`) |
| `scripts/expected-skips/README.md:8`-`:13` | *"this workstation skips 12 at rest and 14 under load, GitHub Actions skips 14 … None of those four numbers"* | the at-rest number moved when batch 31 added one name and the rule behind it did not; CI's 14 pinned to green run 40564336703; *"nothing here states a total"* |
| `scripts/expected-skips/README.md:81` (heading) | *"An in-scope list is not `local.txt` minus twelve"* | *"… minus its delegation names"*, which is also how `docs/runner-confinement.md:723` already phrased it |
| `scripts/expected-skips/README.md:83`-`:85` | *"The twelve names in `local.txt` … can make all twelve … `docs/runner-confinement.md`, *Making those twelve*"* | *"Every name in `local.txt` … all of them … *Making those**"* — that cross-reference was **dead**: the heading is now `### Making those: the invocation is the finding` |
| `scripts/expected-skips/README.md:91` | *"the list loses twelve names"* | *"loses its delegation names"* |
| `scripts/expected-skips/local-under-load.txt:10` | *"(12 at rest)"* | *"whose names were not in local.txt on that date"*, with the **14 kept** as that blessed run's measurement and said to be one |
| `scripts/expected-skips/local-under-load.txt:20` | *"would put all 14 there"* | *"all of that run's names"* |
| `docs/orchestration/workflow.md:213` | *"would put all fourteen names there"* | *"the at-rest names **and** the two flood benches"* |
| `docs/orchestration/workflow.md:469`-`:484` | *"Neither host checks the twelve confinement assertions"*, *"All **twelve** skips measured here read …"*, *"neither host runs the twelve"* | *"the confinement assertions behind invariant 2"*, *"Every skip measured here came back to cgroup v2 delegation, most of them in those words"*, *"neither host runs the delegation list"*; *"The difference measured that day, 12 names here against 14 on CI"* keeps the measurement and dates it |
| `docs/orchestration/gotchas/a-delegated-scope-blinds-the-preflight.md:3` | *"the twelve cgroup-delegation assertions"* | *"this workstation's cgroup-delegation assertions — `local.txt` names them"* |
| `…:21` | *"The method that unlocks twelve assertions disables check 4."* | *"unlocks those assertions"* |
| `board.md` **ORCH-048** | *"two of the twelve reasons"*, *"each of the twelve is in exactly one test binary"* | *"two of the reasons in `local.txt`"*, *"each name on that list"* — the **two** is still correct and was re-checked, see *Risks* |
| `board.md` **ORCH-070** | *"This workstation reports 12 …; CI's `check` reports 15 …"*, present tense | past tense and dated to 2026-10-26, *"Both figures have moved since and the point has not"*, pointing at `scripts/expected-skips/` |

**One `**twelve**` was corrected to a verbatim-safe word, not to a
number.** `workflow.md:471` claimed all twelve skips *read* a given
sentence. Measured on the live file: **11 of the 13** lines in
`local.txt` contain `cgroup v2 delegation is unavailable` verbatim; the
two ceiling assertions name delegation as the cause in their own words.
*Read* was an unwarranted qualifier at twelve names too, so the sentence
now says the skips *came back to* delegation, *"most of them in those
words"*.

**ORCH-313's row** carries the sweep and says in one sentence that its
earlier closure was **met-vs-scope**: the two sentences it named were
genuinely fixed, its own title — the prose that counts the skip list —
was not, and a row whose title states a class while its Item cell lists
instances closes on the list.

**Both script edits are line-count-neutral**, deliberately.
`scripts/gate.sh` is 733 lines and `scripts/test.sh` 302, the same as on
`main`, because `board.md` and the gotchas reference `gate.sh:150-152`,
`:160`, `:240`, `:244`, `:278` and `test.sh:90`, `:149`, `:160`, and
`local-under-load.txt` references `gate.sh:45, :215-228`. A one-line
growth above those anchors would have silently falsified eight
pointers.

**Figures re-derived from the live tree, not copied from the brief.**
All three receipt hashes reproduce: `skip-list-sha256` over the 13
sorted at-rest names is
`67ae71ed344e1ed18cb6c1c75002aabcc15e77de010c0601d1b89a81c1b20489`; over
the 15 observed under load,
`c9dd3040103efcedea68c62392b44e653304b933528293d0886f69b29a36222f`;
`ignore-list-sha256` over the 38 `HERD-IGNORED` names is
`f7a38e4ee5f51977f0a1adbb9afb55aedfb5496268202f6c4744d2aed7a46545`.
`local.txt` 13 lines, `local-under-load.txt` 2, `ci.txt` 14,
`ignored.txt` 38.

## Not done, and why

- **`scripts/report-skips.sh:20`** carries the same false sentence
  `test.sh` did — *"a runner that reports 15 skips when green"* — and
  the file is **not among the brief's allowed paths**, so it is a
  finding and not an edit. Drafted as the `ORCH-NEXT` row below.
- **`docs/status.md` and `docs/coverage.md`** are **correct today** (13
  runtime / 15 loaded / 38 ignored; 13 loud skips) and outside the
  brief's paths: `status.md` is fenced at 90 lines (D276) and
  `coverage.md` is held by another branch. Both totals are still
  hand-maintained, so the next name on `local.txt` falsifies them
  exactly as batch 31 did. Same `ORCH-NEXT` row.
- **`scripts/expected-skips/ci.txt` untouched**, and the brief's reading
  is confirmed, not merely accepted: 14 `HERD-SKIPPED` lines, 13 distinct
  names, 12 saying cgroup v2 delegation. A different host's population.
- **`herd-runner/`, `herd-hub/`, `herd-proto/`, `herdctl/` untouched** —
  no file under them is in this diff.
- **`docs/decisions.md` untouched.** No decision is drafted: every
  change applies D187 and D27 as they already stand.

## Gates

`scripts/gate.sh` and `scripts/test.sh` are outside CLAUDE.md's
prose-exempt set, so **all four Rust gates plus `check-docs.sh` apply**
and were run as one whole `./scripts/gate.sh`, through the queue (D258),
not locally.

Run **before** this file was committed, verbatim:

```
./scripts/check-docs.sh   (see the agent's report for the block)
```

The **last run**, named and not quoted here, on the committed tree:
`./scripts/gate.sh` submitted whole through
`/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791 run
--label build --memory-mib 4700`, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-313-skip-count-sweep`.
Its output and its receipt path are in the agent's report.

**Skips:** the names this host cannot make are
`scripts/expected-skips/local.txt`, plus the two load-gated flood benches
in `local-under-load.txt` if the host is loaded; this handoff states no
total, which is the whole point of the task. Every `HERD-SKIPPED` line
the gate observed is in its receipt and in the agent's report, named one
by one.

## Risks and findings

1. **`report-skips.sh`'s and `test.sh`'s *"15 when green"* was wrong
   twice over, and `workflow.md` already held the evidence.**
   `workflow.md:433`-`:436` says the *"15 on CI"* came from run
   **40439537929**, a **red** run that printed *"AT LEAST 15
   assertion(s)"* — a floor from an incomplete run (D179). CI's green
   figure is **14** (`ci.txt`, green run 40564336703). So the false part
   was the qualifier *when green*, not the number alone. This is the
   brief's own *"a true claim can carry a false qualifier"* shape, and
   it is why `test.sh` now points at `ci.txt` rather than at a number.
2. **ORCH-048's *"two of the reasons"* was re-measured, not assumed.**
   Only `a_ceiling_this_host_can_carry_…` and
   `an_io_weight_that_cannot_bite_…` embed a PID-bearing temporary path;
   the thirteenth name carries no path. The **two** still holds, so only
   the denominator was wrong and only the denominator was changed.
3. **`local.txt` and `ci.txt` are no longer a 2-name delta.** Measured:
   two local-only names and two CI-only names, 11 shared. `ci.txt` was
   blessed on 2026-10-27, before batch 31, and the two
   `an_io_weight_that_cannot_bite_is_said_out_loud` entries differ by a
   parenthetical branch suffix, so they compare as **different names**.
   Nothing in the tree claims otherwise any more, but a future brief
   that re-derives *"the difference is two CI-only skips"* from
   `workflow.md` would be quoting a 2026-10-28 measurement.
4. **The detector was proved before its silence was trusted.** ugrep's
   empty-alternation false negative was avoided by running every pattern
   against a known-positive line first
   (`README.md:9`, *"workstation skips 12 at rest"* — matched, exit 0).
   The sweep was then run twice: spelled number words across `scripts/`
   and `docs/`, and any numeric token on a line mentioning *skip*.
5. **Sites examined and left, with their class** — this is the list the
   brief asked for, and it is a list and not a count:
   - *Dated records, true when written:* `gate.sh:33`, `gate.sh:574`,
     `test.sh:63`, `README.md:43`, `check-orchestration.sh:475`,
     `check-orchestration.sh:923` — all six are batch 9's measurement
     that an `#[ignore]` left the twelve-name list unmoved, which is the
     evidence for ORCH-202 and must keep its figure.
   - *Dated measurements whose figure is the evidence:* `gate.sh:194`
     and `check-orchestration.sh:856` (the two collation hashes over
     that day's twelve names — the brief is right that the pinned case
     below `:856` is a synthetic two-name fixture; neither was touched),
     `README.md:95` (*"the count went 12 → 12 → 12 while the reasons
     changed completely"*, the punchline of a dated section),
     `README.md:89` (**+11, not +12**, now explicitly *"measured against
     the twelve names `local.txt` held on 2026-10-30"*),
     `workflow.md:241`-`:246` (*"0 failed, 12 skips"* on sha `ee8dec0`,
     the run the whole method rests on).
   - *Synthetic fixtures, not claims about this host:*
     `check-orchestration.sh:1408` (a fake batch-9 landing record with a
     fake CI run id), and the flood fixtures at `:1043`-`:1223`
     (`10.67`, `10.12`, `9.04`, `12.00`, *"2 assertion(s)"*).
   - *A different population, and correct:* `ci.txt`'s header.
   - *Already correct and the model the rest now follows:*
     `docs/runner-confinement.md:634`-`:726`, which states its own
     denominator — *"The list held **twelve** names that day, so every
     denominator below is twelve and is that run's, not this list's
     length today"*. Every fix above is that shape.
   - *Landing records, out of scope by the brief:* `board.md:438`,
     `:458`-`:463`, `:473`-`:474`, `:526`, `:840`, `:1002`, `:1247`,
     `:1455`-`:1506` — all inside *Next*, all dated, and `:1505` already
     records the at-rest list as **13** and under load **15**.
   - *Not about skips at all:* `bench-preflight.sh:18`,
     `check-orchestration.sh:1804` and `workflow.md:643`-`:656`
     (twelve **cores**); `operating.md:556` (twelve per-file **lines**);
     `coverage.md:314`, `:339` (milestones, musts);
     `road-to-v1.md:104` (steps); `board.md` ORCH-087, ORCH-114,
     ORCH-144 (call sites, event lines, `expect_green` cases);
     `gotchas/an-attribute-census-needs-an-anchor.md` (grep hits, and
     its 38 is still exact).
6. **Nothing in the sweep contradicted the brief.** Its two swapped
   labels were not found repeated anywhere in the tree: no prose claims
   the receipt's `skip-list-sha256` is over the at-rest file, and no
   prose implies a reworded `#[ignore]` reason moves
   `ignore-list-sha256`. `gate.sh:240` keeps the assertion name only,
   with the comment the brief quotes.

## Drafted for `herd-merger`

**No `docs/decisions.md` entry.** Every change applies D187, D27 and
D279 as they stand; none needs a number.

**One `board.md` row, already written on the branch as `ORCH-NEXT`**, at
the end of *Open* immediately after ORCH-330, for the merger to number.
Its full text is the row in the tree; it is reproduced here so the
merger need not diff for it:

```
| ORCH-NEXT | **Three hand-maintained skip totals survive ORCH-313's sweep because they are outside its paths, and one of them is already false.** (1) `scripts/report-skips.sh:20` reads *"on a runner that reports 15 skips when green"*. CI's green figure is **14** — `scripts/expected-skips/ci.txt`, blessed from green run 40564336703 — and the 15 came from run 40439537929, which `docs/orchestration/workflow.md` already identifies as a **red, incomplete** run whose output was a floor (D179). So the sentence's own qualifier, *when green*, is the part that is wrong, and `scripts/test.sh` carried the identical sentence until this sweep. (2) `docs/status.md:67`-`:68` carries **13** runtime, **15** loaded, **38** ignored and *"the thirteen needing"*; (3) `docs/coverage.md:390`-`:391` carries **13** loud skips and *"every one of the thirteen"*. Both are **correct today** and both are hand-maintained, so the next name on `local.txt` falsifies them exactly as batch 31 did — this is ORCH-070's unwritten rule biting twice more | ORCH-313's sweep, `orch-313-skip-count-sweep`, 2026-10-31; `report-skips.sh` was outside the brief's allowed paths, `status.md` is fenced at 90 lines (D276) and `coverage.md` is held by another branch | herd-step | open | For (1), one sentence: take the figure out and point at `ci.txt`, the way `test.sh` now does — it is a change to a gate script, so all four Rust gates. For (2) and (3), decide with **ORCH-070** rather than separately: either both sentences name the list instead of a total, or `check-docs.sh` gains a check that a skip total in `docs/` is named with the host and the date it was measured on. `status.md` has **zero line slack** (D276), so a fix there must not be longer than what it replaces | ORCH-070 | `scripts/report-skips.sh:20`; `scripts/expected-skips/ci.txt` header; `docs/orchestration/workflow.md` *What the local merge costs*; `scripts/expected-skips/local.txt`; `history/2026-10-31-orch-313-skip-count-sweep.md` |
```

The three rows this branch **edits in place** — ORCH-313, ORCH-048 and
ORCH-070 — are in the tree and need nothing from the merger beyond the
ordinary merge. All three are single-line table rows, so a conflict with
another branch would be adjacency and not content.

## Decisions needed

None. The one open question the sweep raises is already a board row,
**ORCH-070** — *where does the rule live that a skip total is named with
its host and date* — and this sweep is the second time its absence has
cost a landing. The drafted `ORCH-NEXT` row depends on it.

## Resume point

`herd-merger` picks the branch up for the batch's one landing. Nothing
is owed before that. If a reviewer wants the single strongest check that
the sweep is complete, re-run its detector and classify every hit:

```sh
grep -rniE '\b(eleven|twelve|thirteen|fourteen|fifteen)\b' scripts/ docs/ \
  | grep -vE '^docs/orchestration/history/|^docs/decisions\.md|^docs/remediation/'
```

Every remaining hit should fall into one of the six classes listed under
*Risks and findings* item 5. Prove the pattern fires on
`scripts/expected-skips/README.md` first: ugrep returns an empty result
with exit 2 on an empty ERE alternation, which reads as a clean sweep.

## Processes

No background process was started and no PID was killed. The queue job
that ran the gate was submitted with `herdctl` and is identified in the
agent's report by its job id.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-313-skip-count-sweep`,
off the root filesystem, removed with `rm -rf` once the branch was
pushed.
