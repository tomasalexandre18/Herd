#!/usr/bin/env bash
# The local gate: the one command a landing depends on.
#
# It runs, in order, everything `.github/workflows/ci.yml` runs, on the tree
# as it stands, and ends in one verdict line that names the commit it is
# about. Only `gate: GREEN` authorises a push to `main` (ORCH-001).
#
#   ./scripts/gate.sh                  run the gate
#   ./scripts/gate.sh --out FILE       write the log there
#   ./scripts/gate.sh --bless          adopt this run's skip list as the
#                                      expectation for this host. NOT a
#                                      gate run: it ends in
#                                      `gate: BLESSED` and writes no
#                                      receipt at all.
#   ./scripts/gate.sh --verify-receipt SHA
#                                      exit 0 iff a GREEN receipt exists
#                                      for exactly that sha
#
# Three rules decide its design, and each answers a way a local run can lie.
#
# 1. A count is not a signal; a list is (D27). A skipped assertion is
#    printed as skipped and never counted as passed, so this gate compares
#    the run's skips against a committed list of expected ones and is RED on
#    any skip that is not on it. It says nothing about the number: this host
#    skips 12 at rest and 14 under load, and neither figure means anything on
#    its own. The names do.
#
# 2. A load-dependent run is not a run, and the host's load at an instant
#    the GATE chose is not evidence about a skip. Two flood benches skip
#    themselves above a one-minute load of 8, and each prints the figure it
#    read at the moment it read it (`herd-hub/tests/fake_hub_flood.rs`). So
#    the load-conditional list is applied PER SKIP, out of the refusing
#    bench's own line, and never out of a reading the gate took itself. A
#    skip is excused only when BOTH hold: its NAME is on the
#    load-conditional delta list, and its REASON names this bench's load
#    ceiling with a figure above that ceiling and above this gate's own
#    threshold. Everything else stays red, including a name on the delta
#    list that skipped for some other reason, and one that claimed load
#    with a figure UNDER the ceiling — each of those is a defect in the
#    bench, and the gate is where it shows. The gate still reads the host
#    twice, at step 0 and after the suite, and still refuses to run at all
#    above a ceiling; those two figures are now INFORMATION, in the verdict
#    and in the receipt, and they decide nothing.
#
#    Why: two samples by the gate miss a burst that begins AND ends inside
#    the suite, which is the routine case on this host. Measured twice on
#    2026-10-29 on branches that cannot move a bench's reading — the host
#    went 1.68 -> 8.54 -> 11.60 -> 10.12 -> 3.30, the benches read 10.12 as
#    they ran and correctly declined, the gate sampled 1.70 and 3.30, and
#    called those two correct skips unexpected. Two whole suite runs lost to
#    a reason that was not in the diff. The reading that matters is the one
#    the bench actually made, at the moment it made it; nothing else is
#    evidence about why that skip exists.
#
# 3. A gate run on a tree that is not what gets pushed proves nothing. The
#    gate refuses a dirty tree, and writes a receipt named for the sha it
#    ran on. `--verify-receipt` is what a pre-push hook asks, and it reads
#    only the verdict and the sha — so a run that cannot stand behind its
#    own verdict writes NO receipt, rather than a receipt with a caveat in
#    it. A `--bless` run is exactly that run: it adopts this run's own skips
#    as the expectation, so its step 7 green-lights itself.
#
#    The receipt store is per worktree (`$HERD_GATE_RECEIPTS`, default
#    `target/herd-gate/`, under a gitignored directory), so a green gate
#    does NOT travel from one worktree to another. Whoever pushes gates the
#    tree they push.
#
# What it cannot cover, said plainly: a second machine, a pristine checkout,
# the release path (`.github/workflows/release.yml` builds the artefacts the
# consumer installs and nothing local does), GitHub's own drift in the
# actions it pins, and flakiness in either direction. A green gate is
# evidence from one host, not a proof.
set -uo pipefail

# The gate is independent of the stdin it was handed, for two reasons that
# both end in a landing. Nothing below reads fd 0, so this costs nothing.
#
#  - Step 2 runs check-orchestration.sh, whose `bash -c` captures go wrong
#    when fd 0 is a socket (the note at the top of that script measures
#    it). It guards itself, and the gate runs no `bash -c` of its own, so
#    this is defence in depth: it guards every other child the gate starts,
#    present and future, so no landing can be derailed by its caller's fd 0.
#  - scripts/githooks/pre-push calls `gate.sh --verify-receipt` from inside
#    a `while read -r ...` loop over the ref list git feeds it on stdin. A
#    child that consumed any of that stdin would silently drop a ref from
#    the hook's checks. This makes that impossible.
#
# ORCH-111.
exec </dev/null

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root" || exit 1

receipts=${HERD_GATE_RECEIPTS:-$root/target/herd-gate}
expected=${HERD_GATE_EXPECTED:-$root/scripts/expected-skips/local.txt}
expected_load=${HERD_GATE_EXPECTED_UNDER_LOAD:-$root/scripts/expected-skips/local-under-load.txt}
load_note=${HERD_GATE_LOAD_NOTE:-8}
load_max=${HERD_GATE_LOAD_MAX:-24}
# A seam, so that "the load rose while the suite ran" is a case a test can
# run rather than a sentence in a comment: both readings come from here and
# nothing else in this file reads /proc/loadavg.
loadavg=${HERD_GATE_LOADAVG:-/proc/loadavg}
read_load1() { cut -d' ' -f1 "$loadavg" 2>/dev/null || echo 0; }

bless=0
out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --bless) bless=1; shift ;;
    --out) out=${2:?--out needs a path}; shift 2 ;;
    --verify-receipt)
      sha=${2:?--verify-receipt needs a sha}
      r="$receipts/$sha.receipt"
      if [ ! -f "$r" ]; then
        echo "gate: no receipt for $sha in $receipts" >&2
        exit 1
      fi
      if ! grep -q "^verdict: GREEN\$" "$r"; then
        echo "gate: the receipt for $sha is not GREEN" >&2
        exit 1
      fi
      if ! grep -q "^sha: $sha\$" "$r"; then
        echo "gate: $r does not name $sha" >&2
        exit 1
      fi
      echo "gate: GREEN receipt for $sha"
      exit 0
      ;;
    # The header block, counted rather than written down: a literal range
    # goes stale the first time a line is added above it, and a help text
    # that prints half a sentence is how nobody reads it twice.
    -h|--help) sed -n "2,$(($(grep -n '^set -uo pipefail' "$0" | cut -d: -f1) - 1))p" "$0"; exit 0 ;;
    *) echo "gate.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done

mkdir -p "$receipts" || exit 1
started=$(date -u +%Y-%m-%dT%H:%M:%SZ)
sha=$(git rev-parse HEAD 2>/dev/null || echo unknown)
[ -n "$out" ] || out="$receipts/gate-$(date -u +%Y%m%dT%H%M%SZ)-${sha:0:12}.log"
: > "$out" || exit 1

# Everything this script prints lands in one file, and that file is what a
# reader reads: a `tail` has lost evidence here before.
say() { printf '%s\n' "$*" | tee -a "$out"; }
rule() { say "--------------------------------------------------------------"; }

failed=""
note_lines=""
note() { note_lines="$note_lines$1"$'\n'; say "note: $1"; }

# run_step <number> <id> <name> <command...>
# The id is short and has no spaces: it is what the verdict line names, and
# a verdict a reader has to parse is not a verdict.
run_step() {
  n=$1; id=$2; name=$3; shift 3
  rule
  say "step $n: $name"
  say "  \$ $*"
  start=$(date +%s)
  "$@" >>"$out" 2>&1
  rc=$?
  took=$(( $(date +%s) - start ))
  if [ "$rc" -eq 0 ]; then
    say "step $n: $name — ok (${took}s)"
  else
    say "step $n: $name — FAILED (exit $rc, ${took}s)"
    failed="$failed $n:$id"
  fi
  return "$rc"
}

# A skip's identity is its assertion name: the text between the marker and
# the first colon. The reason that follows it carries a temporary path and a
# pid on every run, so it is kept in the expected list for the reader and is
# deliberately not what the gate compares.
skip_names() {
  grep -h '^HERD-SKIPPED: ' "$1" 2>/dev/null \
    | sed 's/^HERD-SKIPPED: //' | sed 's/:.*$//' | sort
}

refuse() {
  rule
  say "gate: REFUSED sha=$sha reason=$1"
  say "gate: NOT GREEN — this sha must not be pushed to main."
  say "log: $out"
  exit 3
}

# --- step 0: the tree, the sha and the host -------------------------------
rule
say "step 0: the tree, the sha and the host"
host=$(hostname)
cores=$(nproc 2>/dev/null || echo ?)
memavail=$(awk '/^MemAvailable:/ {print $2 " kB"}' /proc/meminfo 2>/dev/null)
load1=$(read_load1)
branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)
say "  started:      $started"
say "  sha:          $sha"
say "  branch:       $branch"
say "  host:         $host"
say "  nproc:        $cores"
say "  MemAvailable: $memavail"
say "  load1:        $load1"
say "  gate.sh:      $(git rev-parse --short HEAD 2>/dev/null) tree"

dirty=$(git status --porcelain 2>/dev/null)
if [ -n "$dirty" ]; then
  say "  the working tree is not clean:"
  printf '%s\n' "$dirty" | sed 's/^/    /' | tee -a "$out"
  refuse "the working tree is not clean, so the gate would not be about $sha"
fi
say "  the working tree is clean: this gate is about $sha and nothing else"

if awk "BEGIN{exit !($load1 > $load_max)}"; then
  refuse "load1 $load1 is above the ceiling $load_max; retry when the host is quieter"
fi
# The host's own two readings, kept because a reader wants to know the gate
# ran on a busy host and the receipt should record it. They no longer decide
# which expected list applies: step 7 asks the bench that refused.
host_busy=0
host_trigger=""
note_host_load() { # <label> <value>
  host_busy=1
  [ -z "$host_trigger" ] || host_trigger="$host_trigger,"
  host_trigger="$host_trigger$1=$2>$load_note"
}
if awk "BEGIN{exit !($load1 > $load_note)}"; then
  note_host_load load1-start "$load1"
  say "  load1 $load1 is above $load_note: the flood benches may skip"
  say "  themselves at this load. Whether the load-conditional list applies"
  say "  is decided at step 7, out of the skip line of whichever bench"
  say "  refused, and not out of this figure."
fi
say "step 0: the tree, the sha and the host — ok"

# --- steps 1..6: what ci.yml runs ----------------------------------------
run_step 1 check-docs "check-docs.sh (ci job: docs)" ./scripts/check-docs.sh
run_step 2 check-orchestration \
  "check-orchestration.sh (ci job: docs)" ./scripts/check-orchestration.sh
run_step 3 nix-flake-check "nix flake check (ci job: check)" \
  nix flake check --no-build
run_step 4 cargo-fmt "cargo fmt (ci job: check)" \
  nix develop --command cargo fmt --all --check
run_step 5 cargo-clippy "cargo clippy (ci job: check)" \
  nix develop --command cargo clippy --workspace --all-targets -- -D warnings

suite="$receipts/test-output-${sha:0:12}.txt"
rule
say "step 6: test suite (ci job: check)"
say "  \$ nix develop --command ./scripts/test.sh   (output: $suite)"
start=$(date +%s)
nix develop --command ./scripts/test.sh >"$suite" 2>&1
suite_rc=$?
took=$(( $(date +%s) - start ))
cat "$suite" >> "$out"
if [ "$suite_rc" -eq 0 ]; then
  say "step 6: test suite — ok (${took}s)"
else
  say "step 6: test suite — FAILED (exit $suite_rc, ${took}s)"
  failed="$failed 6:test-suite"
fi
# D27 and D179: a run that did not finish is not a run. `test.sh` prints
# `test suite: PASSED` only when cargo test exited 0, and report-skips.sh
# prints "AT LEAST n" for an incomplete run. Never call either a pass.
if ! grep -q '^test suite: PASSED$' "$suite"; then
  say "step 6: the suite did not print 'test suite: PASSED'; this is not a"
  say "        complete run and cannot be reported as one (D27, D179)."
  case " $failed " in *" 6:test-suite "*) ;; *) failed="$failed 6:test-suite-incomplete";; esac
fi

# --- step 7: the skips, by list and never by count -----------------------
rule
say "step 7: the skips this host could not make (ci job: check, annotation)"
./scripts/report-skips.sh "$suite" >>"$out" 2>&1
skips_rc=$?
[ "$skips_rc" -eq 0 ] || failed="$failed 7:report-skips"
observed="$receipts/skips-${sha:0:12}.txt"
grep -h '^HERD-SKIPPED: ' "$suite" 2>/dev/null | sort -u > "$observed"
say "  observed skip lines: $(wc -l < "$observed" | tr -d ' ') (the count is"
say "  not the signal; the list is)"
say "  observed list:        $observed"

# The load's second reading, taken here because this is the first moment
# after the suite. It is printed, carried in the verdict and written into
# the receipt, and it decides nothing: a burst that began and ended inside
# the suite has decayed away by now, and the benches that declined because
# of it are the only witnesses left. Step 8 prints and records this same
# figure; the load is not read a third time.
load1_end=$(read_load1)
if awk "BEGIN{exit !($load1_end > $load_note)}"; then
  if [ "$host_busy" -eq 0 ]; then
    say "  load1 rose from $load1 to $load1_end, above $load_note. The figure"
    say "  is information: which list applies is decided below, by name and"
    say "  by reason, out of the skip lines themselves."
  fi
  note_host_load load1-end "$load1_end"
fi

if [ "$bless" -eq 1 ]; then
  if grep -q '^test suite: PASSED$' "$suite"; then
    mkdir -p "$(dirname "$expected")"
    { echo "# The assertions this host cannot make, as a list and not a count."
      echo "# Produced by scripts/gate.sh --bless on $started"
      echo "# host=$host nproc=$cores load1=$load1 sha=$sha"
      echo "# The gate compares the assertion NAME of each line (the text"
      echo "# between the marker and the first colon). The reason that"
      echo "# follows is kept for the reader and is not compared: it carries"
      echo "# a temporary path and a pid that change on every run."
      cat "$observed"
    } > "$expected"
    say "  --bless: wrote $(wc -l < "$observed" | tr -d ' ') lines to $expected"
  else
    say "  --bless refused: the suite did not pass, so its skip list is not"
    say "  an expectation (D27)."
    failed="$failed 7:bless-on-a-red-suite"
  fi
fi

if [ ! -f "$expected" ]; then
  refuse "no expected skip list at $expected; produce one with --bless after a green suite"
fi

allowed="$receipts/allowed-skips-${sha:0:12}.txt"
skip_names "$expected" | sort -u > "$allowed"
seen="$receipts/seen-skips-${sha:0:12}.txt"
skip_names "$observed" > "$seen"
say "  expected list: $expected"

# The load-conditional list is a DELTA over the at-rest list and never a
# whole list, and it is read here for its NAMES only: those are the only
# names the excuse below may ever reach. Any other unexpected name is red,
# whatever its reason claims.
load_delta="$receipts/load-delta-${sha:0:12}.txt"
: > "$load_delta"
if [ -f "$expected_load" ]; then
  skip_names "$expected_load" | sort -u > "$load_delta"
  say "  load-conditional delta list: $expected_load"
  say "  a DELTA over the at-rest list and never a whole list; its"
  say "  $(wc -l < "$load_delta" | tr -d ' ') name(s) are the only ones a bench's own reading can excuse"
  declined=$(comm -12 "$load_delta" "$seen")
  ran=$(comm -23 "$load_delta" "$seen")
  if [ -n "$ran" ]; then
    say "  load-gated benches that RAN, and whose assertions were made:"
    printf '%s\n' "$ran" | sed 's/^/    ran      /' | tee -a "$out"
  fi
  if [ -n "$declined" ]; then
    say "  load-gated benches that DECLINED, and must now say why:"
    printf '%s\n' "$declined" | sed 's/^/    declined /' | tee -a "$out"
  fi
else
  say "  there is no load-conditional delta list, so a bench that declined"
  say "  for load would be red: $expected_load"
fi

# load_excuse <skip line>
# The refusing bench's own words, or nothing. The reason has to name the
# host's one-minute load AND this bench's ceiling, and the figure it gives
# has to be above that ceiling and above this gate's $load_note. The two
# phrasings are the two the bench prints — at the flood's start and
# mid-flood (herd-hub/tests/fake_hub_flood.rs). Prints "<figure> <ceiling>"
# and exits 0 when the line carries that evidence. A reason that merely
# claims load, or claims it with a figure under the ceiling, is a
# contradiction inside the bench: exit 1, and the gate stays red on it.
load_excuse() {
  printf '%s\n' "$1" | awk -v note="$load_note" '
    {
      i = index($0, ":"); if (i == 0) exit 1
      rest = substr($0, i + 1)            # past "HERD-SKIPPED"
      j = index(rest, ":"); if (j == 0) exit 1
      reason = substr(rest, j + 1)        # past the assertion name
      if (match(reason, /over this bench.s ceiling of [0-9]+(\.[0-9]+)?/) == 0) exit 1
      ceiling = substr(reason, RSTART, RLENGTH); sub(/^.*ceiling of /, "", ceiling)
      if (match(reason, /one-minute load is Some\([0-9]+(\.[0-9]+)?\)/) > 0) {
        figure = substr(reason, RSTART, RLENGTH)
        sub(/^.*Some\(/, "", figure); sub(/\)$/, "", figure)
      } else if (match(reason, /one-minute load reached [0-9]+(\.[0-9]+)?/) > 0) {
        figure = substr(reason, RSTART, RLENGTH); sub(/^.*reached /, "", figure)
      } else {
        exit 1
      }
      if (figure + 0 <= ceiling + 0) exit 1
      if (figure + 0 <= note + 0) exit 1
      print figure " " ceiling
      exit 0
    }'
}

unexpected=$(comm -13 "$allowed" "$seen")
absent=$(comm -23 "$allowed" "$seen")

# Every name that is not on the at-rest list is asked twice: is it a name
# the delta list may excuse at all, and does its own line give the load
# ceiling as the reason with a figure above it. Both, or red.
excused=""
unexpected_left=""
if [ -n "$unexpected" ]; then
  say "  each skip that is not on the at-rest list, asked by name and then by"
  say "  reason whether the load-conditional delta excuses it:"
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    if ! grep -qxF -- "$name" "$load_delta"; then
      unexpected_left="$unexpected_left$name"$'\n'
      say "    red      $name: not on the at-rest list, and not a name the"
      say "             load-conditional delta list may ever excuse"
      continue
    fi
    lines=$(grep -F "HERD-SKIPPED: $name:" "$observed")
    figures=""
    carries=1
    if [ -z "$lines" ]; then
      carries=0
    else
      while IFS= read -r line; do
        [ -n "$line" ] || continue
        if evidence=$(load_excuse "$line"); then
          read -r figure ceiling <<EOFEV
$evidence
EOFEV
          [ -z "$figures" ] || figures="$figures+"
          figures="$figures$figure>$ceiling"
        else
          carries=0
          break
        fi
      done <<EOFLINES
$lines
EOFLINES
    fi
    if [ "$carries" -eq 1 ]; then
      [ -z "$excused" ] || excused="$excused,"
      excused="$excused$name=$figures"
      say "    excused  $name: the bench itself read $figures, the ceiling"
      say "             being the one its own skip line names, and declined"
    else
      unexpected_left="$unexpected_left$name"$'\n'
      say "    red      $name: on the delta list, but its own line does not"
      say "             give the load ceiling as the reason with a figure"
      say "             above it, so nothing says why it went unchecked:"
      if [ -z "$lines" ]; then
        say "             (no skip line of its own to read a reason out of)"
      else
        printf '%s\n' "$lines" | sed 's/^/             /' | tee -a "$out"
      fi
    fi
  done <<EOFNAMES
$unexpected
EOFNAMES
fi
if [ -n "$unexpected_left" ]; then
  say "  SKIPS THAT ARE NOT ON THE EXPECTED LIST — the gate is red on these,"
  say "  because an assertion nobody expected to be skipped was not checked:"
  printf '%s' "$unexpected_left" | sed 's/^/    + /' | tee -a "$out"
  failed="$failed 7:unexpected-skip"
fi
unexpected=$unexpected_left
if [ -n "$absent" ]; then
  note "an expected skip did not appear; the host may have gained a capability, or a test was renamed or removed:"
  printf '%s\n' "$absent" | sed 's/^/    - /' | tee -a "$out"
fi
if [ -z "$unexpected" ] && [ -z "$absent" ]; then
  say "  the skip list matches the expectation exactly."
fi
case " $failed " in
  *" 7:unexpected-skip "*|*" 7:report-skips "*|*" 7:bless-on-a-red-suite "*)
    say "step 7: the skips — FAILED" ;;
  *) say "step 7: the skips — ok" ;;
esac

# --- step 8: one verdict, and a receipt named for the sha -----------------
rule
finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)
say "step 8: the verdict and the receipt"
say "  started $started, finished $finished"
say "  load1 $load1 at the start, $load1_end after the suite"
# Which list applied, and what justified it. An excuse names the bench, the
# figure IT read and the ceiling ITS own line gave, because that is the
# reading the decision rests on. A busy host where no bench declined gets
# the at-rest list and says so with its figures beside it, so the one line
# still answers "why this list" on its own.
if [ -n "$excused" ]; then
  skips_field="skips=under-load($excused)"
elif [ "$host_busy" -eq 1 ]; then
  skips_field="skips=at-rest(host-busy:$host_trigger,no-bench-declined-for-load)"
else
  skips_field="skips=at-rest"
fi
if [ -n "$failed" ]; then
  say "  steps that failed:$failed"
  rule
  say "gate: RED sha=$sha failed=$(echo $failed | tr ' ' ',') $skips_field"
  say "gate: NOT GREEN — this sha must not be pushed to main."
  say "log: $out"
  exit 1
fi

# A blessing run stops one step short of a receipt, and that is the whole of
# the rule. It adopted this run's own skip list as the expectation at step 7,
# so its step 7 green-lit itself and its verdict is not evidence about this
# sha. Nothing a receipt could carry would say so where it is read:
# `--verify-receipt` reads the verdict line, the sha and nothing else, and
# `pre-push`'s check 1b asks only that — so a receipt from a blessing run
# would be taken as the gate for a landing's second parent. The alternative
# considered and rejected was a `blessed: yes` field that `--verify-receipt`
# refuses: it leaves an object that exists and must be disbelieved, and
# every reader of the store then has to know the rule. Writing nothing needs
# no rule.
if [ "$bless" -eq 1 ]; then
  rule
  say "gate: BLESSED sha=$sha steps=0-8:ok host=$host load1=$load1 $skips_field"
  say "gate: NOT A GATE RUN — a blessing run green-lights its own step 7, so"
  say "      no receipt was written and this verdict authorises no push."
  say "      Re-run without --bless to gate $sha."
  say "log: $out"
  exit 0
fi

receipt="$receipts/$sha.receipt"
{
  echo "verdict: GREEN"
  echo "sha: $sha"
  echo "branch: $branch"
  echo "host: $host"
  echo "nproc: $cores"
  echo "load1-start: $load1"
  echo "load1-end: $load1_end"
  echo "started: $started"
  echo "finished: $finished"
  echo "log: $out"
  echo "suite-output: $suite"
  echo "expected-skips: $expected"
  echo "load-excused-skips: ${excused:-none}"
  echo "skips-observed: $(wc -l < "$observed" | tr -d ' ')"
  echo "skip-list-sha256: $(sort "$seen" | sha256sum | cut -d' ' -f1)"
  echo "steps: 0..8 all ok"
} > "$receipt"
say "  receipt: $receipt"
if [ -n "$note_lines" ]; then
  say "  notes stand, and none of them is a failure:"
  printf '%s' "$note_lines" | sed 's/^/    /' | tee -a "$out"
fi
rule
say "gate: GREEN sha=$sha steps=0-8:ok host=$host load1=$load1 load1-end=$load1_end $skips_field"
say "log: $out"
exit 0
