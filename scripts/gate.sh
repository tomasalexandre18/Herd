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
#                                      expectation for this host
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
# 2. A load-dependent run is not a run. Two flood benches skip themselves
#    above a one-minute load of 8, so the gate reads load1 first, allows the
#    load-conditional list above that threshold and says so, and refuses to
#    run at all above a ceiling.
#
# 3. A gate run on a tree that is not what gets pushed proves nothing. The
#    gate refuses a dirty tree, and writes a receipt named for the sha it
#    ran on. `--verify-receipt` is what a pre-push hook asks.
#
# What it cannot cover, said plainly: a second machine, a pristine checkout,
# the release path (`.github/workflows/release.yml` builds the artefacts the
# consumer installs and nothing local does), GitHub's own drift in the
# actions it pins, and flakiness in either direction. A green gate is
# evidence from one host, not a proof.
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root" || exit 1

receipts=${HERD_GATE_RECEIPTS:-$root/target/herd-gate}
expected=${HERD_GATE_EXPECTED:-$root/scripts/expected-skips/local.txt}
expected_load=${HERD_GATE_EXPECTED_UNDER_LOAD:-$root/scripts/expected-skips/local-under-load.txt}
load_note=${HERD_GATE_LOAD_NOTE:-8}
load_max=${HERD_GATE_LOAD_MAX:-24}

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
    -h|--help) sed -n '2,33p' "$0"; exit 0 ;;
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
load1=$(cut -d' ' -f1 /proc/loadavg 2>/dev/null || echo 0)
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
under_load=0
if awk "BEGIN{exit !($load1 > $load_note)}"; then
  under_load=1
  say "  load1 $load1 is above $load_note: the flood benches skip themselves"
  say "  at this load, so the load-conditional expected list applies."
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
skip_names "$expected" > "$allowed"
if [ "$under_load" -eq 1 ] && [ -f "$expected_load" ]; then
  say "  adding the load-conditional list: $expected_load"
  skip_names "$expected_load" >> "$allowed"
fi
sort -o "$allowed" "$allowed"
seen="$receipts/seen-skips-${sha:0:12}.txt"
skip_names "$observed" > "$seen"

say "  expected list: $expected"
unexpected=$(comm -13 "$allowed" "$seen")
absent=$(comm -23 "$allowed" "$seen")
if [ -n "$unexpected" ]; then
  say "  SKIPS THAT ARE NOT ON THE EXPECTED LIST — the gate is red on these,"
  say "  because an assertion nobody expected to be skipped was not checked:"
  printf '%s\n' "$unexpected" | sed 's/^/    + /' | tee -a "$out"
  failed="$failed 7:unexpected-skip"
fi
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
load1_end=$(cut -d' ' -f1 /proc/loadavg 2>/dev/null || echo 0)
say "step 8: the verdict and the receipt"
say "  started $started, finished $finished"
say "  load1 $load1 at the start, $load1_end at the end"
if [ -n "$failed" ]; then
  say "  steps that failed:$failed"
  rule
  say "gate: RED sha=$sha failed=$(echo $failed | tr ' ' ',')"
  say "gate: NOT GREEN — this sha must not be pushed to main."
  say "log: $out"
  exit 1
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
say "gate: GREEN sha=$sha steps=0-8:ok host=$host load1=$load1"
say "log: $out"
exit 0
