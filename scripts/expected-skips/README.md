# The assertions a host cannot make, as a list and never as a count

`scripts/gate.sh` compares the skips of a run against the list for the host
it ran on. It is red on any skip that is not on the list, and it says
nothing about how many there are.

Why a list and not a count. D27 says a skipped assertion is printed as
skipped and never reported as passed. A count cannot carry that: this
workstation skips 12 at rest and 14 under load, GitHub Actions skips 14, and
one red CI run on 2026-10-27 printed "AT LEAST 15" because it had not
finished. None of those four numbers means anything on its own. The name of
an assertion that was not checked does.

Why these files are here and not under `docs/`. `scripts/check-docs.sh`
width-checks every `docs/*.md` at 79 columns and a skip line is far longer
than that. They are evidence, not prose.

| file | what it is |
|---|---|
| `local.txt` | the list this workstation skips at rest. Produced by `./scripts/gate.sh --bless` from a run whose last line was `test suite: PASSED`; the header records the host and the load1 it measured on. |
| `local-under-load.txt` | the **two names** a flood bench's own refusal can excuse, and nothing else. The gate does not apply this file because the host looks busy: it applies it **per skip**, when a skip's name is on this list *and* that skip's own reason names this bench's load ceiling with a figure above both that ceiling and `HERD_GATE_LOAD_NOTE` (8). A name on this list that skipped for any other reason, or claimed load with a figure under the ceiling, is **red** — a defect in the bench, and the gate is where it shows. |
| `ignored.txt` | every `#[ignore]` in the tree, with its reason: the assertions **nobody** checks, on any host. One file and not one per host — see *Two kinds of skip* below. |
| `ci.txt` | what `ubuntu-latest` skipped on green run 40564336703, for comparison. The gate does not read it on this host; `HERD_GATE_EXPECTED` can point at it on a host shaped like that one. |


## Two kinds of skip, two lists, never merged (ORCH-202)

An assertion can go unmade for two different reasons, and a reader who
cannot tell them apart cannot read this directory:

| marker | what it means | where it is expected |
|---|---|---|
| `HERD-SKIPPED` | a test **ran** and said *this host cannot check it* — no cgroup v2 delegation, no block device, a flood bench that declined for load. A different host would check it. | `local.txt`, `local-under-load.txt`, `ci.txt` — per host, because the answer is the host's |
| `HERD-IGNORED` | the test **never ran**: it carries `#[ignore]`. *Nobody is checking it until someone acts* — no host makes this assertion. | `ignored.txt` — one file, because `#[ignore]` is a property of the tree and the same on every machine |
| `HERD-IGNORED-UNEXPLAINED` | the test carries `#[ignore]` with **no reason**. Never expected anywhere: the gate is red on it, and no expectation file can match this marker. | nowhere, by construction |

Why this exists. Until ORCH-202 the gate could not see the second kind at
all. `scripts/test.sh` built its list from the `HERD-SKIPPED` lines a test
writes **while running**, and a test carrying `#[ignore]` never runs, so it
wrote nothing. Measured in batch 9: a new `#[ignore]`d test was printed by
cargo in its own words — `ignored, spool-offer-repro: reproduces the
unfixed startup-offer defect` — and the gate's skip list stayed at the same
twelve names with the same `sha256`. The assertion was visible in cargo's
output and invisible to the list the gate reads, which made `#[ignore]` a
silent exemption from D27.

So `test.sh` now also turns cargo's own `ignored` lines into
`HERD-IGNORED:` lines, prints them in a block of their own, and ends with an
`ignored census:` marker that says whether its list is whole. `gate.sh`
step 7 has two halves, `(a)` runtime and `(b)` compile-time, compared
separately and each by **name** (D187).

**The marker carries the kind, not the filename.** Taking a test off
`#[ignore]` and giving it a runtime skip instead — or the other way round —
therefore moves a line between two files *and* changes its marker. It cannot
be a wash. Batch 9's spool fix is the case that showed why this matters: the
transition should have been legible in the expectation, and before ORCH-202
it was not.

**The receipt carries two figures, separately.** `skip-list-sha256` is still
over the `HERD-SKIPPED` names **alone**, so every figure an older receipt
quoted still compares; `ignore-list-sha256` is the new one, over the
`HERD-IGNORED` names.

**A reason is not optional.** Cargo prints `ignored, <reason>` when the
attribute has one and a bare `ignored` when it does not. A bare one is
refused rather than listed: an assertion nobody makes must at least say why
nobody makes it, and what would undo it. Write
`#[ignore = "why, and what would undo it"]`.
The gate compares the assertion **name** of each line: the text between
`HERD-SKIPPED: ` and the first colon. The reason that follows is kept for
the reader and is deliberately not compared, because it carries a temporary
directory and a pid that change on every run.

Updating a list is a decision, not a chore: a new name on it is an
assertion nobody checks any more. Say in the landing record why it is
there.
