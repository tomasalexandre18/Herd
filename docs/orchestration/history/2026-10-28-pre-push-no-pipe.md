# pre-push-no-pipe — handoff 2026-10-28

- **Item:** ORCH-NEXT (proposed row below; the merger allocates the id)
- **Agent:** herd-step
- **Branch / commit:** `pre-push-no-pipe` / `d305b3c` (the hook; this
  file is the commit after it), based on
  `origin/orch-111-stdin-independence` `20f1500`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`scripts/githooks/pre-push` no longer reads the tip commit's message
through a pipe. Three sites changed, plumbing only — the message text,
the flags and the patterns are byte-identical:

- the section loop, formerly line 291:
  `printf '%s\n' "$msg" | grep -qxF "## $s"` → `grep -qxF "## $s" <<<"$msg"`
- the subject, formerly line 272:
  `printf '%s\n' "$msg" | head -1` → `head -1 <<<"$msg"`
- the header fields, formerly line 295:
  `printf '%s\n' "$msg" | field_value "$f"` → `field_value "$f" <<<"$msg"`

A comment above them records the mechanism, the two conditions, and the
rates measured here, so that nobody turns them back into pipes.

**The section loop was the live defect.** Under the `set -uo pipefail` at
the top of the file, `grep -qxF` exits the moment it matches, `printf` is
killed by SIGPIPE on the bytes it has left, `pipefail` reports the
pipeline as failed, and the `||` branch records a section that *is*
present as missing — so the last machine in front of `main` refuses a
correct landing. It is the one of the three sites whose pipeline status is
read; `head -1` and `field_value` discard theirs, so they took the same
SIGPIPE to no effect. They were converted anyway, because reading their
status one day would bring the defect back with no diff to point at.

**The false refusal was reproduced on the base, before the edit**, with a
*correct* record — seven sections, four filled fields, no placeholder,
46-column subject — padded with per-branch evidence lines and verbatim
gate output, driven the way git drives the hook (the ref line on stdin,
arguments `origin <url>`, cwd at the repository root) against a scratch
repository with a local bare remote:

| record bytes | load1 | base hook | fixed hook |
| --- | --- | --- | --- |
| 3 091 (under 4 KiB) | at rest | 0 / 200 | — |
| 6 051 | at rest | 0 / 200 | 0 / 200 |
| 6 051 | 12–16 | 0 / 200 | 0 / 200 |
| 62 291 | at rest | 0 / 200 | 0 / 200 |
| 62 291 | 12–16 | **4 / 200** | 0 / 200 |
| 1 045 091 | at rest | **50 / 50** | 0 / 50 |
| 1 045 091 | 12–16 | not run | 0 / 50 |

Every false refusal named only sections lying in the first 4 KiB of the
message, which is the mechanism's own signature. The fixed hook refused
none of those 900 correct landings. A real `git push` through
`core.hooksPath` against the scratch bare remote: 1 MB record at rest,
base hook **0 / 5 accepted**, fixed hook **5 / 5 accepted and landed**;
62 291 bytes under load1 12–16, fixed hook **20 / 20 accepted**.

The hook still refuses everything it is supposed to refuse.
`check-orchestration.sh`'s own `pre-push` cases cover all nine required
refusals and were re-run after the change, at their own fixture size.
Because every one of those fixtures is under 4 KiB — which is why none of
them could ever have caught this defect — the same refusals were also
driven against the fixed hook at a **62 kB** record, the size at which it
used to fire: **13 passed, 0 failed**. A deletion, a non-fast-forward, a
remote tip this clone has never seen, a tip with no second parent, a
second parent with no receipt, a RED receipt, a merge whose tree differs
from the gated tree, a missing section, a missing field, an empty field, a
placeholder, a 93-column subject and a one-line message — each refused,
each naming its own reason.

**`exec </dev/null` was not added**, and the two assertions on the base
that forbid it — the case at the end of `check-orchestration.sh` and the
comment above the ref-list loop — are untouched. Git feeds the ref list
on fd 0; a redirected `pre-push` would iterate zero times and wave
through a deletion of `main`.

## Not done, and why

- **No `check-orchestration.sh` case freezes the new shape.** The brief
  set the expected count at 256 before and after, so adding a case was
  out of scope; the text is proposed under *Decisions needed*. Until it
  is added, nothing mechanical stops the pipes coming back — the comment
  in the file is all that stands there.
- Lines formerly 273 (`sed '1d' | grep -c`) and 301
  (`grep -oE | tr`) are **left byte-identical**. Judged inert and
  verified: `grep -c` counts every line and `grep -oE` prints every
  match, so both read to EOF, neither can exit early, and no SIGPIPE
  is possible. No other early-exiting reader exists in the file —
  `columns()`'s awk reads to EOF, and there is no `grep -m`, `sed q` or
  `read` in any pipeline.
- No board row, no `docs/decisions.md` entry, no PR, no merge, no tag.

## Gates

`check-orchestration.sh` on the unmodified base tree, before any edit —
the figure the brief asked to compare against:

```
check-orchestration: 256 passed, 0 failed
```

`./scripts/check-orchestration.sh` after the change, standalone:

```
check-orchestration: 256 passed, 0 failed
```

The last run is `./scripts/gate.sh`, on the committed tree, after this
file was committed and before the push. Its log goes to `gate.log` under
this agent's scratchpad:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/repro/gate.log
```

and its receipt to the worktree's `target/herd-gate/`; the verdict line
is in the agent's report, not here, because a run made before this file
existed gated a different tree.

Skips: the suite's own, named by `gate.sh` against its expected list, in
the report. No `HERD-SKIPPED` line was counted as a pass (D27).

## Risks and findings

1. **The realistic size did not fire through the whole hook.** At 6 051
   bytes — a five-branch record of this project's own shape — the base
   hook false-refused 0 / 200 at rest and 0 / 200 at load1 12–16. The
   bare construct at the same payload, in isolation, false-failed
   1 / 200 under the same load. So at the size a landing record is
   likely to be, the defect is real but rare; at 62 kB it is 4 / 200 and
   at 1 MB it is certain. **For today's record sizes the change is
   closer to prophylactic than to a repair**, and that is how it should
   be reported. What makes it worth landing anyway: the failure mode is
   a refusal of a correct landing by the only machine in front of
   `main`, its diagnostic names a section it has just matched, and there
   is no retry that distinguishes it from a real defect in the record.
2. **A rate measured here is not the rate on the merger's machine.** The
   race depends on scheduling, so a loaded host at landing time is where
   it fires. The figures above are this host on 2026-10-28.
3. **Nothing mechanical holds the new shape.** See *Not done*.
4. The 100755 mode survived: `git ls-files -s` reports `100755` before
   and after, and `git diff` shows no mode line. The file was rewritten
   with `cat new > file`, which keeps the inode and the mode, not with
   `awk` plus `mv`.

## Decisions needed

1. **Freeze the shape in `check-orchestration.sh`?** Recommendation: yes,
   one case, which the brief's 256/256 expectation kept out of this
   branch. Tested as written against both versions of the file — 3 hits
   on the base, 0 on the fixed tree:

   ```sh
   # Under `pipefail` a reader that can exit early kills its writer with
   # SIGPIPE, the pipeline reports failure, and a `grep -q` fed by a
   # `printf` declares absent the text it has just matched. In pre-push
   # that refused a CORRECT landing: 4/200 at a 62 291-byte record under
   # load1 12-16, 50/50 at 1 045 091 bytes at rest, 2026-10-28. Every
   # fixture in this file is under 4 KiB, so no case can catch the
   # symptom -- freeze the shape instead.
   if grep -qE \
     'printf[^|]*\|[^|]*(grep -q|grep -m|head( |$)|sed [^|]*q|field_value)' \
     "$root/scripts/githooks/pre-push"; then
     bad "pre-push reads the commit message through here-strings, not a pipe" \
       "    a reader above can exit before printf finishes; under pipefail
       that records a section that is present as missing"
   else
     ok "pre-push reads the commit message through here-strings, not a pipe"
   fi
   ```

   Fallback: no case, and the comment in `pre-push` carries the rule.
2. **A decision entry?** Recommendation: yes, one, headed `D-NEXT` for the
   merger to number. Proposed text:

   > **A reader that can exit before its writer finishes never sits
   > downstream of a writer in this project's shell.** Under
   > `set -o pipefail` a `grep -q`, `grep -m`, `head`, `sed q` or `read`
   > in a pipeline is killed-writer territory: it exits on the first
   > match, the writer takes SIGPIPE on the bytes it has left, and
   > `pipefail` reports the pipeline as failed — so a control declares
   > absent the very text it has just recognised. Two conditions, both
   > met by any payload over the 4 KiB stdio buffer with a matching line
   > in its first write. Measured in `scripts/githooks/pre-push` on
   > 2026-10-28: a correct 62 291-byte landing record was falsely
   > refused 4 times in 200 at load1 12–16 and a 1 045 091-byte one 50
   > times in 50 at rest, each refusal naming a section present in the
   > message. Every such read is a here-string, which is a temporary
   > file: no reader can exit early and there is no writer to kill.
   > `check-orchestration.sh` already held the rule for its own
   > assertions; it now holds for the push guard, and for any script
   > added later.

   Fallback: no entry, and the comment in `pre-push` plus the one in
   `check-orchestration.sh` are the record.
3. **Board row.** Proposed, for the merger to number and paste; the cells
   are id, what happened, source, owner, state, outcome, depends,
   evidence:

   ```
   | ORCH-nnn | **`scripts/githooks/pre-push` read the tip commit's message through a pipe into an early-exiting `grep -qxF`, so under `pipefail` it recorded a section that is present as missing and refused a correct landing.** The three reads of the message are here-strings now; the section loop was the live one, `head -1` and `field_value` discarded their pipeline status and were inert. | measured and fixed by `herd-step` on branch `pre-push-no-pipe`, 2026-10-28 | herd-step | done | Base hook: 4/200 false refusals at a 62 291-byte correct record under load1 12-16, 50/50 at 1 045 091 bytes at rest, 0/5 real pushes accepted. Fixed hook: 0/900 across five sizes at rest and under load, 25/25 real pushes accepted, and the 13 refusals it owes all still red at 62 kB. **At 6 051 bytes — a realistic record — it is 0/200 either way, so for today's sizes this is prophylactic rather than a repair.** **Left open:** nothing mechanical freezes the shape; the case text is in the handoff | — | `history/2026-10-28-pre-push-no-pipe.md`; `scripts/githooks/pre-push`; `check-orchestration.sh` 256 passed / 0 failed before and after |
   ```

## Resume point

Nothing is owed on this branch. Whoever continues:
`herd-merger` takes `pre-push-no-pipe` into the batch, allocates the
decision and the board row from *Decisions needed*, and — this is the
one that matters for the method — **the first local landing will be the
first landing record this hook has ever read at full size**, so it is
also the first real test of this fix.

## Processes

Started and ended by this task, all killed by PID, never by name:

- 6 CPU spinners pinned to CPUs 10–11 (`taskset`), PIDs 2309507–2309512
- 14 CPU spinners, PIDs 2338546–2338559
- 14 CPU spinners, PIDs 2352211–2352224
- 14 CPU spinners, PIDs 2437651–2437664

Each set was started for one measurement and killed at its end; `ps` after
each confirms none survived. `./scripts/bench-preflight.sh` was run before
each loaded phase and before the gate; it refused once, on a load1 of 9.00
left over from a previous set, and the phase waited rather than starting.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/pre-push-no-pipe`, deleted once
the branch was pushed. The worktree's `target/herd-gate/` holds the gate
receipt and is removed with the worktree.
