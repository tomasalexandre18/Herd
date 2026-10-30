# orch-273-prove-io-max — handoff 2026-10-30

- **Item:** ORCH-273
- **Agent:** herd-step
- **Branch / commit:** `orch-273-prove-io-max` / `402a64a`, based on
  origin/main `8af300e` (batch 22's landing; `origin/main` moved to
  `83bc78a`, batch 23, while this task ran — see *Risks and findings*)
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**`Ceiling::prove_io_max` reads a controller list instead of a file.**
One file changed, `herd-runner/src/cgroup.rs`, 382 insertions and 59
deletions over two commits.

- `prove_io_max` now reads `<probe>/cgroup.controllers` — the
  controllers that probe cgroup was handed — **before it writes
  anything**, and refuses naming the controller when `io` is not among
  them. The old `control.exists()` reading is gone.
- The write is left to answer only the two readings that are the
  kernel's own: `io::ErrorKind::NotFound` is `io.max` not being an
  interface file on this kernel (`CONFIG_BLK_DEV_THROTTLING`), and any
  other error is the kernel refusing that device. So the **three hosts
  get three sentences** where two of them shared one that named both
  causes and neither answer.
- The body moved into a free `prove_io_max_in`, so that every road out
  — the refusals and the proof alike — passes the probe's removal;
  `prove_io_max_through` is the seam a test reaches.
- **`Kernel` gains `mkdir`**, beside `write`, `proc_root` and `me`. A
  cgroup's interface files come from its parent's
  `cgroup.subtree_control`, so an empty temporary directory is a cgroup
  no kernel ever makes, and a fixture that handed one to the proof
  would be testing nothing. `Kernel::real()`'s is `fs::create_dir`.
- The module doc carries the rule as its own section, *A controller is
  read, never inferred from a file being there*, beside D259's *an
  `EBUSY` is read rather than guessed*, with the measured fixture and
  the explicit statement that **neither refusal changed class**.

**Five assertions over a fake tree, and the detector was proved to
fire.** `fake_io_base` writes the host shape into the tree the fake
kernel reads (which controllers the base hands down, whether the kernel
has blk-throttle, whether a cgroup is left holding a stray `io.max`);
`fake_cgroup_mkdir` populates a new cgroup the way a kernel does,
`io.pressure` included, which is the measured shape; `fake_io_write`
supplies the one thing no temporary directory will do — `ENOENT` for a
write naming a file cgroupfs never creates — and `EINVAL` for a device
it has no queue for.

1. `an_io_file_in_a_cgroup_handed_no_io_controller_proves_no_ceiling`
   — the known-bad fixture: `cgroup.controllers` without `io`, an
   `io.max` file there anyway. **Must refuse.**
2. `a_controller_list_that_will_not_read_is_a_refusal_naming_the_absence`
   — an absence is reported as an absence, not as a controller.
3. `a_withheld_controller_and_a_kernel_with_no_throttling_read_apart`
   — the two hosts the existence reading made one sentence of.
4. `a_device_the_kernel_refuses_is_the_kernels_refusal_and_not_the_controllers`
   — the controller list must not swallow the kernel's own refusal.
5. `a_base_that_really_hands_io_down_proves_the_ceiling` — the other
   direction, so a proof that refuses everything is not a fix.

**The red proof.** With the controller-list reading reverted in the
working tree and **nothing else changed** — the old `control.exists()`
branching restored verbatim from `origin/main` — Herd job
`j0000000069` ran `cargo test -p herd-runner --lib cgroup::tests`:
**20 passed, 3 failed**. Which assertion went red for which clause:

- test 1 failed at `expect_err`, panicking `a file is not a
  controller: ()` — **the old reading returned `Ok`**: a cgroup holding
  an `io.max` file with `io` absent from its controller list was
  called *proved*. That is the defect, demonstrated rather than argued.
- test 3 failed on `!why.contains("CONFIG_BLK_DEV_THROTTLING")` for the
  withheld-controller host: the old sentence read *"io.max does not
  exist, so this kernel has no per-cgroup IO throttling ..., or the io
  controller is not handed down ..."* — both causes, no answer.
- test 2 failed on `why.contains("cannot be read")`: the unreadable
  controller list came back as *"io.max does not exist"*.
- tests 4 and 5 stayed green under both readings, which is what they
  are for: they are the guard that the fix refuses no more than it
  should, not tests of this defect.

The fixed file was then restored and `cmp`-checked byte-identical to
the committed one before any gate ran.

## Not done, and why

- **`herd-runner/src/lib.rs` and `herd-runner/src/packstore.rs` were
  not touched**, as the brief's *Owns* required: both were changed by
  M12 P4 on a branch integrating at the same time, and a second editor
  there would have handed the merger a conflict for nothing.
- **No `docs/` change but this handoff**, and no `decisions.md` or
  `board.md` edit: the entry and the row are drafted below, in full.
- **The real kernel path is still unproved on this host**, and that is
  not a gap this task could close: our account gets no cgroup v2
  delegation, which is the standing twelve `HERD-SKIPPED` assertions. A
  skip is not a proof (D27), which is why the permanent proof is the
  fake tree above.
- **Nothing asserts that the probe's reading and `Ceiling::unenforced`
  agree.** `cannot_enforce("io")` already holds a reason whenever
  `prepare_base` could not hand `io` down, so there are now two
  readings of the same fact. They agree by construction — the probe is
  a child of the base the enable ran against — and a test that pinned
  them together would be worth having. It is not in this task's
  *Done when* and is left as a finding rather than smuggled in.

## Gates

Every Rust gate ran **inside a Herd job**, submitted with
`/mnt/ssd_980/herd/queue/bin/herdctl` (never bare: bare `herdctl` has
the consumer's production hub compiled in), `--label build
--memory-mib 6000`, after `./scripts/bench-preflight.sh`, on the clean
tree of `402a64a`. One job per gate. Verbatim output is in the agent's
report; the figures and ids are here.

| gate | job | exit | wall | `bytes_written` |
|---|---|---|---|---|
| `cargo build --workspace --all-targets` (cold) | `j0000000067` | 0 | 107.3 s | 90,137,663 |
| `cargo fmt --all --check` | `j0000000070` | 0 | 2.8 s | 3,402,568 |
| `cargo clippy --workspace --all-targets -- -D warnings` | `j0000000071` | 0 | 28.4 s | 89,821,892 |
| `./scripts/test.sh` | `j0000000072` | 0 | 318.3 s | 23,712,109,947 |

No job came back `stopped_by_cap`; every one of the four carries a
verdict and a receipt, so no resubmission was needed. The earlier
`j0000000064` is a **red build of my own code** — `E0308` on
`mkdir: fs::create_dir`, a generic `fn` item where a `for<'a> fn(&'a
Path)` was wanted — reported red, fixed in `402a64a`, and not re-run
locally for a greener answer.

**Skips: 14 `HERD-SKIPPED`, compared as a list and never as a count
(D187).** The standing twelve are present and **identical by name** to
`scripts/expected-skips/local.txt`; the two extra are `flood-fifty` and
`flood-two-fifty`, the load-gated flood benches, which are
`local-under-load.txt`'s own two and declined at load 9.18 against
their ceiling of 8. Nothing new and nothing missing.

Every one of the twelve is a cgroup-delegation skip, which is to say
they are the tests nearest this change. **None of the five new
assertions is among them**: all five appear in this run's `ok` lines,
because they run against a tree the test builds, which is the whole
reason that fixture exists.

The name extractor behind that comparison was itself proved on a
synthetic line before its silence was trusted: the first attempt
compared twelve copies of the string `HERD-SKIPPED` and would have
passed whatever the names were.

`#[ignore]` census **COMPLETE at 38**, 0 without a reason, as the
suite's own census line says. Counted independently anchored at
start-of-line (`grep -rc '^\s*#\[ignore'`, ugrep 15.2.0) and it agrees;
unanchored answers 51, which counts prose.

**`./scripts/check-docs.sh` is the last run**, on the tree with this
handoff committed. Its output is in the agent's report, not in the tree
it gates. An earlier run on `402a64a` said `docs ok`.

**The fmt gate ran while the preflight was refusing on load**, and that
is said rather than hidden: `cargo fmt --all --check` compiles nothing
— 2.8 s and 3.4 MB written, measured — so it was judged not to be a
heavy phase. Every phase that compiles waited for a green preflight;
the preflight refused four times during this task, at load 12.08,
11.64, 9.91 and 9.03 against a ceiling of 9.00, and each time the next
job waited for it.

## Risks and findings

1. **`origin/main` moved under this branch while it ran**: batch 23
   landed as `83bc78a`. `herd-runner/src/cgroup.rs` and
   `herd-runner/src/confine.rs` are **byte-identical** between
   `8af300e` and `83bc78a` (`git diff --stat` over both paths is
   empty), so there is no conflict in what this branch owns. The branch
   is still based on `8af300e` because that is what the brief named.
2. **The brief's premise holds, with one correction worth stating
   precisely.** The existence reading is real and it is gone, but on a
   **real cgroupfs** the old code could not have returned `Ok` with
   `io` undelegated by that route: cgroupfs creates no file, so the
   write would have met `ENOENT`. What the old code did on a real host
   was **misdiagnose** — one sentence for a withheld controller and for
   a kernel with no blk-throttle — and what it did on any tree whose
   `io.*` files are not controller-gated was **call the ceiling
   proved**. That second shape is exactly the one measured on
   `app.slice` (`io.pressure` present, `io` absent from
   `cgroup.controllers`), which is why the class and not the single
   call is what was fixed.
3. **The census of this defect class in `cgroup.rs` is complete and it
   was one site.** `control.exists()` at the old `:536` was the only
   existence-for-controller inference in the module; the other four
   `exists()`/`is_dir()` readings are a base directory's existence, a
   test fixture's marker and a test's own assertion, none of which
   stands in for a controller. Every controller question in the module
   already reads a list: `prepare_base` reads
   `base/cgroup.controllers`, `enable_for_children_through` reads it
   again, `is_enabled_for_children` reads `cgroup.subtree_control`.
   **`herd-runner/src/blockio.rs` is clear of the class too** and was
   checked because it is the other `io` reader: `io_weight_reach` reads
   the *contents* of `io.cost.qos` and `io.cost.model` and the active
   scheduler, never a file's presence. No edit was made there — it is
   outside *Owns* and needed none.
4. **A Herd job does not enter the requester's worktree, and
   `workflow.md` says it does.** Measured in job `j0000000063`: the
   job's working directory is
   `/mnt/ssd_980/herd/queue/runner-data/jobs/<id>/work`, an **empty**
   directory that is not a git repository, with `HOME` set to a
   per-job directory and no `cargo` on `PATH`. The *Execution goes
   through Herd* bullet reads *"a job gets no workspace — it `cd`s into
   the requester's worktree"*; the `cd` is the **requester's own**, in
   the command it submits, and nothing in the runner does it. Every
   gate here therefore ran `bash -c "cd <worktree> && ..."`. A board
   row for that sentence is not drafted — it is `workflow.md` prose and
   outside this task's *Owns* — but it cost this task one wasted job
   and it will cost the next agent one too.
5. **An ad-hoc command line declares no commit, so nothing checks that
   the gated tree is committed.** `herdctl run -- <argv>` recorded
   `declared_commit: null` and `ran_commit: null`, and the checkout's
   dirty-tree and remote-tracking refusals (`herdctl/src/checkout.rs`)
   never fired. The gates here were run on a tree checked clean with
   `git status --porcelain` immediately before each submission, by
   hand, because Herd did not do it. That is also what made the red
   proof cheap: the control ran against a dirty working tree, with no
   commit and no push.

## Drafted for `herd-merger`

**For `docs/decisions.md`** — append this entry, allocating the number
in place of `D-NEXT` (D148: numbers are identifiers and gaps stay). The
highest on `main` at the time of writing is **D260**, and three
branches were unlanded ahead of this one.

```markdown
## 2026-10-30 — D-NEXT A controller is read, never inferred from a file

`Ceiling::prove_io_max` is what stops a hard `--job-io-max` from being
announced on a host that cannot enforce it. Until today it decided
whether the `io` controller is handed down to a job's cgroup from
whether `<probe>/io.max` **existed**, and a file proves nothing about a
controller: PSI files are not controller-gated, and measured on this
project's host on 2026-10-30 `app.slice` holds `io.pressure` while `io`
is absent from its `cgroup.controllers`.

**The rule.** "Which controls can a job's cgroup really carry" is
answered by a controller list and by nothing else:
`cgroup.subtree_control` for what a cgroup hands to its children,
`cgroup.controllers` for what a cgroup has been handed. `prove_io_max`
now reads `<probe>/cgroup.controllers` before it writes anything, and
the write answers only the two readings that are the kernel's own — a
kernel with no blk-throttle, which is `ENOENT` on the interface file,
and a device the kernel refuses, which is any other error. The three
hosts therefore get three sentences where two of them used to share
one, and that shared sentence named both causes and neither answer.

**What it cost to find out, and that is the general shape.** This is
D259's rule — a refusal with several possible readings is measured down
to one, or says that it could not be — applied one layer down, to a
*reading* rather than to a refusal. The defect was reported after a
public correction that read one file, then "corrected" itself by
reading a different file, and was worth nothing either time until an
agent produced the fixture: `io.pressure` present with `io` missing
from the controller list.

**Neither refusal changed class.** An `io.max` is a ceiling an operator
*asked* for, so every way it cannot be enforced is a startup refusal,
before and after (`Confined::prepare`); `io.weight` is unasked-for and
stays absent and declared. What did change is a case that used to be
**no refusal at all**: a cgroup holding an `io.max` file with `io`
absent from its controller list was called proved, and an operator
would have been told a ceiling was enforced by a kernel never handed
the controller to enforce it with.

**The real kernel path is still unproved here and says so.** Our
account gets no cgroup v2 delegation, which is the standing twelve
`HERD-SKIPPED` assertions; a skip is not a proof (D27). So the
permanent proof is a fake tree, as D259's is: `Kernel` gains `mkdir`,
because a cgroup's interface files come from its parent's
`cgroup.subtree_control` and an empty temporary directory is a cgroup
no kernel ever makes, and the fixture's writer refuses a write naming a
file that is not there, which is what no temporary directory will do.

Reversible: one free function and one struct field in
`herd-runner/src/cgroup.rs`, held by five assertions against that tree,
three of which were watched to go red against the reverted reading —
including the one that returned `Ok` where the ceiling bounds nothing.
```

**For `docs/orchestration/board.md`**, section *Owed — agreed, not
started*, replacing the existing ORCH-273 row: state `done`, and the
*Recommendation* and *Evidence* cells extended. Eight cells, no
literal pipe anywhere in it.

```markdown
| ORCH-273 | **`Ceiling::prove_io_max` infers whether the `io` controller is handed down from `<probe>/io.max` *existing*, and a file proves nothing about a controller.** Measured 2026-10-30: `app.slice` on this host holds `io.pressure` while `io` is absent from its `cgroup.controllers`, so the existence reading can answer yes where the controller was never delegated | `cgroup-first-enable-race` handoff, 2026-10-30, the *Not done* section; split out of that branch's one drafted row by the orchestrator at batch 22's integration | herd-step | done | **Done 2026-10-30 on `orch-273-prove-io-max`** (`802e013`, `402a64a`). `prove_io_max` reads `<probe>/cgroup.controllers` before it writes anything, and the write is left to answer only the kernel's own two readings: `ENOENT` on the interface file is a kernel with no blk-throttle, any other error is the device refused. The three hosts now get three sentences where two shared one that named both causes and neither answer. `Kernel` gains `mkdir`, because a cgroup's interface files come from its parent's `cgroup.subtree_control` and an empty temporary directory is a cgroup no kernel ever makes. Five assertions over that fake tree, and the detector was proved to fire: with the reading reverted and nothing else changed, three of the five went red in Herd job `j0000000069`, the known-bad one by returning `Ok` — a cgroup holding an `io.max` file with `io` absent from its controller list was called proved. Neither refusal changed class: an asked-for ceiling that cannot be enforced is still a startup refusal | nothing | **D-NEXT** drafted in `history/2026-10-30-orch-273-prove-io-max.md`; `herd-runner/src/cgroup.rs` (`Ceiling::prove_io_max`, `prove_io_max_in`, `Kernel::mkdir`); **D259** for the rule one layer up; ORCH-272 for the same handoff's product half |
```

## Decisions needed

none

## Resume point

Nothing is in flight. For the merger: this branch is
`orch-273-prove-io-max` at `402a64a`, pushed, based on `8af300e`, and
it touches one product file nothing else in the batch touches. The one
command that would carry it further is the batch's own
`./scripts/gate.sh` at integration, which is the merger's and not an
agent's.

Whoever picks up the finding of *Risks and findings* 4 — the
`workflow.md` sentence about a job entering the requester's worktree —
needs no build: it is one prose bullet and job `j0000000063`'s
verdict is the evidence.

## Processes

- No long-lived process was started. Four Herd jobs carried the gates
  (`j0000000067`, `j0000000070`, `j0000000071`, `j0000000072`), one the
  red control (`j0000000069`), one the formatting write
  (`j0000000068`), one a probe of where a job runs (`j0000000063`), and
  one is the reported red build (`j0000000064`). Every one of them is
  the runner's child, not this agent's; nothing was signalled, and no
  PID was killed.
- Our hub (565152), our runner (607643) and the consumer's (3006543,
  3021002) were not touched.
- **Own build tree deleted once the branch was pushed:** `debug` with
  test targets, `16 G` at `/mnt/ssd_980/herd/build/orch-273`
  (`du -sh`, profile named before the number), removed with `rm -rf`
  after the push. `<worktree>/target/` holds only `test.sh`'s skip log
  and is removed with the worktree.
