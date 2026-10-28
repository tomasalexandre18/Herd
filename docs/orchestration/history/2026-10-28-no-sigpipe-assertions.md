# no-sigpipe-assertions — handoff 2026-10-28

- **Item:** the 25 latent pipe assertions (done); **ORCH-111 stays open**
  on a corrected diagnosis — its real cause is identified and proved, and
  its fix is outside this brief's paths
- **Agent:** herd-step
- **Branch / commit:** `no-sigpipe-assertions` / see the agent's report,
  based on origin/harden-the-gate `446e632`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here

## Done

### The 25 conversions

`scripts/check-orchestration.sh` runs under `set -uo pipefail`, where

    printf '%s' "$x" | grep -q <pattern> && ok "..." || bad "..."

is a race: `grep -q` exits the moment it matches, `printf` is killed by
SIGPIPE on the bytes it has left to write, and `pipefail` reports the
pipeline as failed — so the assertion declares a text missing that it has
just recognised, and prints that very text as its diagnostic.

All 25 sites now read their payload through a here-string, the form the
nine assertions added by the gate hardening already use. Counts, with the
brief's own command:

```text
$ grep -cE 'printf.*\|[[:space:]]*grep -q' scripts/check-orchestration.sh
26        # on 446e632, before
0         # after
$ grep -cE '\|[[:space:]]*grep ' scripts/check-orchestration.sh
0         # after: no pipe into grep of any kind is left
```

The 26 lines the pattern matched on the base are **25 code sites plus one
comment line** — the hardening's own note, whose text contains the
literal `printf | grep -q`. Converted, by line number on the base:

```text
 42  149  171  294  296  316  344  345  430  476  487  507  508
512  513  518  602  615  618  653  714  989 1161 1261 1262
```

plus one `grep -c` pipe, on line 171 beside a converted `grep -q`. That
one is not the race — `grep -c` reads to EOF and cannot exit early — but
it was the last pipe into `grep` in the file, and it sat in a line this
change rewrites anyway. Every site took a here-string; none refused one.

**The change is mechanical, and that is checkable.** A pristine base run
through the same converter differs from the committed file in 31 lines,
of which 30 are the two comment edits and one is that `grep -c`:

```text
$ diff base-converted.sh scripts/check-orchestration.sh \
    | grep -E '^[<>]' | grep -vE '^[<>] *(#|$)'
< ... [ "$(printf '%s' "$out" | grep -c 'number twice')" -ge 1 ] ...
> ... [ "$(grep -c 'number twice' <<<"$out")" -ge 1 ] ...
```

The 141 `ok`/`bad` message strings are byte-identical before and after
(`diff` of the two sorted lists is empty), and so is every grep flag and
pattern. The file keeps mode `100755`; the first commit lost it and was
amended.

### The mechanism, measured

The race needs **two** things, not one: output larger than the 4 KiB
stdio buffer `printf` writes a pipe in, so that it takes more than one
`write`, **and** a complete matching line inside the first of those
writes, so that `grep -q` can decide before the last one. Measured on
this host, load1 13–15, match on line 1, 200 runs per size:

```text
payload=945      pipe_fail=0/200    herestring_fail=0/200
payload=1950     pipe_fail=0/200    herestring_fail=0/200
payload=3960     pipe_fail=0/200    herestring_fail=0/200
payload=4161     pipe_fail=0/200    herestring_fail=0/200
payload=4965     pipe_fail=1/200    herestring_fail=0/200
payload=7980     pipe_fail=1/200    herestring_fail=0/200
payload=63992    pipe_fail=14/200   herestring_fail=0/200
payload=999982   pipe_fail=200/200  herestring_fail=0/200
```

The previous agent's reproduction fires here too, confirming their
figure: at rest `0/4000`, at load1 11.6–12.7 `1/4000` through the pipe
and `0/4000` through the here-string, on their 7883-byte payload.

### What these 25 sites actually read

Every site was instrumented with the byte size of its payload and the
file run once. The largest payload any of them reads is **3692 bytes**,
below the threshold — so **not one of the 25 can fire on today's tree**.
Max bytes per site, over all 27 instrumented statements:

```text
3692 3551 3551 3551 3551 3273 3273 3273 2893 1373  976  976  976  904
 777  581  513  513  513  380  370  340  296  115   96   25    7
```

This change is therefore **prophylactic**, and the margin is thin: the
largest site is the `gate_case` helper reading a stubbed `gate.sh` run,
at 90% of the threshold, which a few more lines of gate output would
cross.

## Not done, and why

- **ORCH-111 is not closed and not fixed.** The brief's diagnosis does
  not hold for it. Its real cause is identified, proved and reported
  below; the fix changes what seven `env -i` invocations do, which is a
  behaviour change this brief forbids, so it is proposed and not made.
- **`scripts/check-docs.sh` not edited**, as the brief required. Two
  sites found, reported below.
- **`scripts/githooks/pre-push` not edited.** It carries the same defect
  and is outside the paths this brief allows, so it is a finding.
- **The `| head` pipelines in `check-orchestration.sh` were left alone.**
  All 8 remaining sit inside a `$(...)` whose exit status is discarded,
  so SIGPIPE there cannot change an assertion. Audited one by one.

## Gates

`./scripts/gate.sh` on the base `446e632`, before any edit, verbatim:

```text
gate: GREEN sha=446e632f26cfff1b605893c77402d0de16c64d6d steps=0-8:ok host=servernixos load1=0.66 skips=at-rest
```

with `step 2: check-orchestration.sh (ci job: docs) — ok (19s)`,
`step 6: test suite — ok (238s)`, and step 7 reading `observed skip
lines: 12 … the skip list matches the expectation exactly` and `the
at-rest list applies on its own: load1 0.66 at the start and 2.94 after
the suite, neither above 8`.

`check-orchestration.sh` on the base, directly, under load1 13.22:

```text
check-orchestration: 253 passed, 0 failed
```

and the same on this branch after the conversion, which adds no case:

```text
check-orchestration: 253 passed, 0 failed
```

The loaded repetition — the evidence this task owes — is under *Risks
and findings*, because what it shows is not what the brief predicted.

**The last run, not quoted here:** `./scripts/gate.sh` on the committed
tree, whose log and receipt land under `<worktree>/target/herd-gate/` as
`gate-<ts>-<sha>.log` and `<sha>.receipt`; its verbatim output is in the
agent's report.

**Skips:** 12 at rest, the `scripts/expected-skips/local.txt` list
matched exactly by the base gate run; no `HERD-SKIPPED` line from either
check script. D27's two extra names above load1 8, `flood-fifty` and
`flood-two-fifty`, did not apply: the gate ran at load1 0.66.

## Risks and findings

### 1. ORCH-111 is NOT this defect — and its real cause is now proved

The brief asked me to verify before believing it. It does not hold. But
the row is not a mystery any more either.

**The SIGPIPE theory is refuted for this site.** It is
`grep -qF ":$PATH"` reading `$path_val`, one line of 615 bytes with no
trailing newline. 20 000 runs of the exact old idiom with the exact
payload, at load1 13.2 rising to 15.6:

```text
ORCH-111 payload: bytes=615 lines=1 trailing_newline=no
  pipe idiom:   0/20000 false failures
  here-string:  0/20000
```

Two independent structural reasons: 615 bytes is one `write`, so
`printf` is finished before `grep` has anything to match; and the
payload is a single line with no terminator, so `grep -q` cannot decide
until EOF and has no early exit to take. The brief's "long enough to be
split across writes" is out by a factor of about seven.

**What ORCH-111 actually is.** The red samples are a **true** failure,
not a false one — the text really was absent — and the cause is
deterministic:

1. The test captures its dump with
   `env -i PATH="$PATH" HOME=/wrong/home … bash -c '. env.sh; …'`.
2. `env -i` strips **`__ETC_PROFILE_DONE`**, which this host's login
   environment sets to 1.
3. The child `bash -c` reads `/etc/bashrc`, which says:
   `if [ -z "$__ETC_PROFILE_DONE" ]; then . /etc/profile; fi`.
4. `/etc/profile` and `/etc/set-environment` therefore run **inside the
   child**, and rebuild `PATH` from the child's own `HOME=/wrong/home`
   with `USER` and `XDG_STATE_HOME` empty — both consequences of
   `env -i`. **The `PATH` the test handed the child is discarded.**
5. `env.sh` then correctly keeps that inherited `PATH` behind its four,
   and the assertion correctly reports that the caller's literal `$PATH`
   is not in there.

Byte-identical reproduction, against the tail recorded in every red
sample since 2026-10-27:

```text
$ env -i PATH="$PATH" HOME=/wrong/home bash -c 'printf "%s\n" "$PATH"'
/wrong/home/.local/bin:/run/wrappers/bin:/wrong/home/.nix-profile/bin:/nix/profile/bin:/wrong/home/.local/state/nix/profile/bin:/etc/profiles/per-user//bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin

recorded in every ORCH-111 red sample:
/wrong/home/.local/bin:/run/wrappers/bin:/wrong/home/.nix-profile/bin:/nix/profile/bin:/wrong/home/.local/state/nix/profile/bin:/etc/profiles/per-user//bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin
```

**Two controls, both ways.** Each independently restores the supplied
`PATH`, which is what pins the chain in step 3–4:

```text
$ env -i PATH="$PATH" HOME=/wrong/home NOSYSBASHRC=1 bash -c '…'
  __ETC_BASHRC_SOURCED=unset
  PATH=<the caller's PATH, unchanged>
$ env -i PATH="$PATH" HOME=/wrong/home __ETC_PROFILE_DONE=1 bash -c '…'
  PATH=<the caller's PATH, unchanged>
```

and without either, the child reports `__ETC_BASHRC_SOURCED=1` and
`__ETC_PROFILE_DONE=1`, neither of which `env -i` could have passed it.

**It is not one case: it is 21 of 253.** When it fires, the PATH
assertion is the only one that *names* it; the other 20 are the `gate:`
cases, and they fail for the same reason. The gate section hands its
subshell a stub directory in front —
`env -i PATH="$gatedir/bin:/usr/bin:/bin:$PATH"` — and the rebuild
throws `$gatedir/bin` away, so the stubbed `nix` and `cargo` are never
found and the gate goes red on its own steps:

```text
FAIL  gate: the at-rest verdict must say so
gate: RED sha=62576981a4ee… failed=3:nix-flake-check,4:cargo-fmt,5:cargo-clippy,6:test-suite skips=at-rest
```

That is exactly the signature three agents recorded as "one PATH failure
followed by the fifteen gate cases" and read as collateral. It is not
collateral; it is the same defect wearing a different mask. There are
**seven** `env -i` invocations in the file and all seven are exposed.

Measured on the gate section's own `PATH`, with a stub `nix` in
`$gatedir/bin`:

```text
as the gate section writes it:
  nix resolves to: /run/current-system/sw/bin/nix
  PATH starts: /tmp/.local/bin:/run/wrappers/bin:/tmp/.nix-profile/bin:/nix
with __ETC_PROFILE_DONE=1 added:
  nix resolves to: <gatedir>/bin/nix
  PATH starts: <gatedir>/bin:/usr/bin:/bin:…
```

So when this fires the gate section is not testing its stubs at all — it
runs the **real** `nix` and `cargo` inside a check script whose header
says "No toolchain needed". That is worse than a red: on a run where the
real toolchain happened to answer the way the stub would, those 20 cases
would have passed while testing nothing they claim to test.

**The fix, proposed and not made:** pass `__ETC_PROFILE_DONE=1` through
each `env -i` — one token per site, no assertion's message or meaning
changed, and all 21 cases fixed at once. `NOSYSBASHRC=1` works equally
well; `__ETC_PROFILE_DONE=1` is the better one, because it says *the
profile has already been applied* rather than *suppress the system
rc file*, and because the test's whole point is to start from a
**known** environment rather than whatever a profile would build.

The PATH assertion is also fragile beyond this: it asserts that the
caller's `$PATH` survives into a shell the test deliberately sterilised,
which is not the property it cares about. `env.sh` keeping *what it
inherited* behind its four is, and a sentinel entry the test itself puts
on the inner `PATH` would assert that correctly whatever the child reads.

**What is still not pinned: what arms it.** While armed, the rebuild is
deterministic — 300/300 captures, and in six different calling contexts
— and the full script read `232 passed, 21 failed` five runs in a row.
While not armed, the child reports `__ETC_BASHRC_SOURCED=no`, nothing is
rebuilt, and the script reads `253 passed, 0 failed` every time. The
state flipped once during this task, between 18:55 and 19:30, with no
edit to either tree in between. So the mechanism is settled but its
switch is not, and that is precisely the intermittency three agents
sampled honestly and disagreed about (7 red in 14, 5 in 11, 0 in 5).

What was ruled out: the kind of `stdin` (inherited, `/dev/null`, a pipe,
a regular file, closed — all identical, and the harness gives a tool
call `/dev/null` anyway); `BASH_ENV`, which `env -i` strips and which
reads unset in the child; the calling context (direct, one wrapper,
`bash -c`, `bash file`, a subshell, under another `env -i`); and the
launching `$PATH`, which was byte-identical across the flip. One
hypothesis left unmeasured: bash reads `SYS_BASHRC` for a
non-interactive shell when it believes it was started by a remote shell
daemon, which it decides by looking at whether `stdin` is a **socket**.
Testing that needs a listening socket, which this host's permission
system refuses (*Expose Local Services*) and invariant 3 would question
anyway, so it was not run and is not claimed.

**None of that blocks the fix.** `__ETC_PROFILE_DONE=1` makes the child
skip `/etc/profile` whatever armed it, which is why the recommendation is
the fix and not more sampling.

### 2. The loaded repetition, and what it does and does not show

Twelve `nice -n 19` spinners took load1 from 0.3 to 16.4. Interleaved
runs of the base and this branch, same load, same machine, alternating
so neither side got a quieter window — 12 pairs, plus 5 earlier base
runs and 2 direct ones:

```text
after  (this branch): 12 runs, load1 13.8-16.2, 12 x 253 passed, 0 failed
before (446e632):     12 runs, load1 13.4-16.4, 11 x 253 passed, 0 failed
                                                 1 x 252 passed, 1 failed
before (446e632):      5 runs, load1 13.9-15.1,  5 x 232 passed, 21-22 failed
```

The honest reading, fixed in advance by the
payload measurement above: **the before/after counts must come out the
same**, because no site in this file reads more than 3692 bytes and the
race needs over 4096. A difference would have meant my measurement was
wrong, not that the change had worked.

So this change cannot be credited with any red turning green, and is
not claimed to. The one base failure is finding 3, and it is not the
race. Load is also not the variable in finding 1: the five
`232 passed, 21 failed` runs and the later all-green runs were made at
the same load1 of 14–15, on trees whose `scripts/` are byte-identical
(`git diff 446e632 HEAD -- scripts/` shows only this conversion).

### 3. A second intermittent failure on the base, and it is not the race

One of the twelve loaded base runs came back `252 passed, 1 failed`, on
a case the other eleven passed and that the branch passed twelve times:

```text
--- before run 4, load1=14.86
FAIL  board sections out of shape
    exit=1, output:
    note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
```

`check-docs.sh` exited 1 — so something went red — while printing only
the pre-existing D170 note and **not** the `sections are …` message the
case plants a defect to provoke. **It is not the SIGPIPE race**, and the
diagnostic is what rules it out: the race's signature is the expected
text being *present* in the printed output, and here it is absent. Nor
can it be, by the sizes above.

The reading most consistent with it is that `fresh()` — which copies the
whole repository 32 times per run — came back incomplete under load, so
the `sed -i` that plants the defect had nothing to edit and `check-docs`
read a partial tree. That is not confirmed; one sample in twelve is a
lead, not a diagnosis. Worth its own row, with this handoff as the
sample, because it is a second way for the `docs` CI job to go red for
nothing.

### 4. The same defect in `scripts/githooks/pre-push`, line 291

`set -uo pipefail` at line 110, and:

    printf '%s\n' "$msg" | grep -qxF "## $s" || missing="$missing
      missing section: ## $s"

`$msg` is the tip commit's whole message and the status **is** consumed.
A landing record over 4 KiB with a required `## Section` header early in
it can be reported missing although it is there, and the guard then
refuses a good push. It fails closed, which is the safe direction, but it
is a false red on the last machine between an agent and `main`. Outside
this brief's paths, so not edited. Its `| head -1` at line 272 is inside
a status-discarded `$(...)` and is harmless.

### 5. `scripts/check-docs.sh`: two sites, neither able to fire today

Looked at and not edited, as the brief required. Under its
`set -uo pipefail`:

- line 247,
  `grep -E '^[^A-Za-z]*Base[^A-Za-z]' "$f" | grep -qi 'read-only' ||`
  — a pipe between two greps, the second with `-q`, status consumed;
- line 249, `printf '%s\n' "$named" | grep -qxF "$checkout"`.

Both payloads are a handful of lines from one brief, far below 4 KiB, so
neither can fire at today's sizes. Its other pipes (`grep -v`, `sort`,
`uniq`, `wc`, `seq | grep -vxF`) read to EOF and cannot exit early.

### 6. The two hooks are safe only because they lack `pipefail`

`.claude/hooks/orchestrator-guard.sh` has 10 instances of the shape and
`.claude/hooks/release-guard.sh` 4, but both run under `set -u` with no
`pipefail`, so the pipeline's status is `grep`'s and SIGPIPE on `printf`
is invisible. That is load-bearing and undocumented: adding `pipefail`
to either file would create 14 races at once, and in the guard the
failure direction is **fail-open** — a pipeline reported failed reads as
"pattern not found", which *allows* the write the guard exists to deny.
Worth a comment in both files; outside this brief's paths.

### 7. The other shell files

`scripts/image-smoke.sh`, `scripts/release-smoke.sh` and
`deploy/wsl2-check.sh` have the shape but run under `set -eu` or
`set -u` with no `pipefail`: inert, like the hooks. `gate.sh`,
`test.sh`, `report-skips.sh` and `bench-preflight.sh` set `pipefail` and
have no early-exit reader whose status is consumed.

### 8. Two corrections to the brief's arithmetic

- "the hardening already converted nine of its own" — it converted
  nothing. It **added** nine new assertions already written as
  here-strings. The base and `origin/main` carry the **same 25 code
  sites**: the sorted lists of their site texts are identical. The
  base's count of 26 is `origin/main`'s 25 plus the hardening's comment
  line, which the `grep -c` pattern matches on its own text.
- `grep -cE "printf.*\|[[:space:]]*grep -q"` is not a count of sites. It
  counts lines, it counts comments that quote the idiom, and a line
  holding two sites counts once.

## Decisions needed

1. **ORCH-111's row: keep it open, re-specified.** Recommendation: leave
   the state `open`, replace its theory with the proved mechanism, and
   name the one-token fix. Proposed row text:

   > `check-orchestration.sh` hands each of its seven `env -i`
   > subshells a `PATH`, and this host's `bash` throws it away: `env -i`
   > strips `__ETC_PROFILE_DONE`, so the child reads `/etc/bashrc`,
   > which re-sources `/etc/profile`, which rebuilds `PATH` from the
   > child's own `HOME=/wrong/home` with `USER` empty. Proved
   > 2026-10-28 with a byte-identical reproduction of the recorded
   > output and two controls (`NOSYSBASHRC=1` and
   > `__ETC_PROFILE_DONE=1` each restore the supplied `PATH`). It costs
   > 21 of 253 assertions when it fires: the PATH case names it, and the
   > 20 `gate:` cases fail because the same rebuild drops the stub
   > `bin` directory, so the stubbed `nix` and `cargo` are never found.
   > The SIGPIPE theory is refuted for this site: 0 false failures in
   > 20 000 runs of the old idiom with its real 615-byte single-line
   > payload at load1 13–15, and the race needs over 4 KiB. Fix: pass
   > `__ETC_PROFILE_DONE=1` through every `env -i`.

   Fallback: close it and open a new row for the mechanism — worse,
   because the samples and the three agents' counts belong with it, and
   nothing is fixed yet.

2. **A new row for finding 4**, `pre-push` line 291 — the only one of
   these that can refuse a good push today. Recommendation: open it,
   state `open`, fixed by the same one-line conversion.

3. **A row for finding 3**, the `board sections out of shape` case that
   went red once in twelve loaded runs with the planted defect
   undetected. Recommendation: open it with this handoff as its one
   sample and `fresh()`'s 32 whole-repo copies as the lead, and do not
   merge it into ORCH-111 — the two have different signatures and
   nothing yet connects them.

4. **A decision entry for the mechanism**, so nobody rediscovers the
   4 KiB threshold. Recommendation one `D-NEXT`:

   > ## 2026-10-28 — D-NEXT An assertion reads its payload through a
   > here-string, never a pipe
   >
   > In a script that sets `pipefail`, piping `printf` into `grep -q` is
   > a race and not a style: `grep -q` exits on its match, `printf` dies
   > of SIGPIPE on what is left, and the pipeline reports the failure of
   > a text that matched. It fires when the payload exceeds the 4 KiB
   > stdio buffer and a matching line sits in the first write — measured
   > on this host at load1 13–15: 0/200 at 4161 bytes, 1/200 at 4965,
   > 14/200 at 64 kB, 200/200 at 1 MB; a here-string 0/200 at every
   > size. Every assertion in `scripts/check-orchestration.sh` was
   > converted, where the largest payload is 3692 bytes and none could
   > fire yet. `check-docs.sh:247,249` and
   > `scripts/githooks/pre-push:291` still carry it; the two
   > `.claude/hooks` guards carry it inertly, having no `pipefail` —
   > adding one there would make 14 races that fail **open**.

   The merger allocates the number; this branch writes nothing to
   `docs/decisions.md`.

## Resume point

`herd-merger` takes this branch in the batch; nothing here depends on
another branch. Then, as a separate task on ORCH-111:

    sed -i 's/env -i PATH=/env -i __ETC_PROFILE_DONE=1 PATH=/g' \
      scripts/check-orchestration.sh    # all 7 sites, then re-run

and expect `253 passed, 0 failed` from inside a wrapper script, which is
where it reads `232 passed, 21 failed` today.

## Processes

Twelve `nice -n 19` spinners started to raise load1 from 0.3 to 15.6,
killed by the PIDs they were started with; no `pkill`, nothing killed by
name. PIDs are in the agent's report. `CARGO_TARGET_DIR` was
`/mnt/ssd_980/herd/build/no-sigpipe-assertions`, outside the worktree,
removed once the branch was pushed. Two extra `git worktree`s were added
read-only under the scratchpad for the before/after comparison and
removed at the end.
