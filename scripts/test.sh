#!/usr/bin/env bash
# The project's test command: the workspace suite, plus the list of
# assertions this host could not make.
#
# Why this exists. Some M4 assertions need a capability the host may not
# have — cgroup v2 delegation above all. Such an assertion is skipped with
# its reason rather than failed (D27), because a suite that is red on a host
# that is merely different is a suite people learn to ignore.
#
# But `cargo test` swallows the stdout of a *passing* test, so the
# `HERD-SKIPPED` line of a skip was invisible under `cargo test --workspace`:
# the suite read as all-green when some of it had not run at all. That
# defeats D27 entirely from a terminal.
#
# So each skip also appends itself to $HERD_SKIP_LOG, and this script prints
# what landed there. A skip is impossible to mistake for a pass here.
#
# And an empty skip log is not by itself good news. `cargo test` stops
# launching test binaries once one has failed, so a red run leaves the log
# holding only what the binaries that did run wrote: on this workstation a
# red run reported 0 skips where a complete run reports 12, and PR #86's red
# `check` job on GitHub Actions printed the same reassurance on a runner
# that reports 15 when green. Two things keep that honest, and both are
# needed:
#
#   * --no-fail-fast by default, so every test binary runs and the log is
#     complete even when one of them fails. The suite is a gate: all of its
#     failures at once is the useful answer, and a skip list collected from
#     a run that stopped early is worthless. It makes nothing less red —
#     the exit status is still cargo's own, and more failures are shown,
#     not fewer. Pass --no-fail-fast yourself and it is not added twice
#     (cargo rejects the flag given twice).
#   * the exit status, consulted below. --no-fail-fast does not make the
#     log provably complete: a compile error, a cancelled run, a binary
#     killed mid-flight or a test filter all leave it partial. So on a
#     non-zero status this script says what it actually knows — "unknown",
#     or "at least N" — and never that nothing was skipped. An empty log on
#     a run that did not finish means *unknown*, not *none*, and D27 is
#     precisely the rule that a thing which did not run is never reported
#     as one that passed.
#   * the caller's own arguments, for the same reason: `test.sh -p herd-proto`
#     finishes green having run a tenth of the suite, so it is told that its
#     empty skip log is about the tests it selected and nothing more.
#
# Usage: ./scripts/test.sh [extra cargo test arguments]
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root" || exit 1

export HERD_SKIP_LOG="${HERD_SKIP_LOG:-$root/target/herd-skips.txt}"
mkdir -p "$(dirname "$HERD_SKIP_LOG")" || exit 1
: > "$HERD_SKIP_LOG"

# --- the other kind of skip: `#[ignore]`, ORCH-202 -----------------------
#
# Everything above is about a skip a *running* test writes. There is a
# second kind, and until ORCH-202 the gate could not see it at all.
#
# A test carrying `#[ignore]` never runs, so it writes nothing to
# $HERD_SKIP_LOG — yet it is exactly an assertion nobody made. Measured in
# batch 9: a new `#[ignore]`d test was printed by cargo as `ignored, ...`
# while the gate's skip list stayed at the same twelve names with the same
# sha256. An `#[ignore]` was a silent exemption from D27, the one rule this
# project treats as non-negotiable.
#
# Cargo does say it, in its own words, on stdout:
#
#   test some_assertion ... ignored, the reason the attribute carries
#   test some_assertion ... ignored                      (no reason given)
#
# So that stream is captured here and turned into a list of its own.
#
# A list OF ITS OWN, deliberately, and never merged into the skip list:
#   HERD-SKIPPED: this host cannot check it.
#   HERD-IGNORED: nobody is checking it until someone acts.
# Those are different facts about coverage and a reader must be able to
# tell them apart. The two lists are compared separately by the gate.
#
# And a bare `#[ignore]` with no reason is refused rather than listed: an
# unchecked assertion whose line cannot say why it is unchecked is the
# worst of the two, so it gets its own marker,
# `HERD-IGNORED-UNEXPLAINED:`, which no expectation file may ever match.
# It is printed here and the gate is RED on it (gate.sh step 7). This
# script's own exit status stays cargo's, for the reason D179 gives: the
# marker below is what says whether the run was complete, and a non-zero
# exit here would make the gate call the *suite* incomplete and the skip
# list unknown — destroying the skip report over a question that is not
# about completeness at all.
cargo_output=${HERD_CARGO_OUTPUT:-$root/target/herd-cargo-output.txt}
ignored_log=${HERD_IGNORED_LOG:-$root/target/herd-ignored.txt}
mkdir -p "$(dirname "$cargo_output")" "$(dirname "$ignored_log")" || exit 1
: > "$cargo_output"
: > "$ignored_log"

# Arguments of the caller's own can narrow what runs — a package, a test
# name filter — so a run that carries any is not evidence about the whole
# suite, and its empty skip log is not evidence that every assertion ran.
narrowed=no
[ "$#" -gt 0 ] && narrowed=yes

# `cargo test` errors out if --no-fail-fast is given twice, so look for the
# caller's own. Arguments after `--` belong to libtest, not to cargo.
own_flag=no
for arg in "$@"; do
  case "$arg" in
    --) break ;;
    --no-fail-fast) own_flag=yes ;;
  esac
done

# The binaries the `real_herd` tests drive are built by the tests themselves
# (`real_herd::binaries`, `capabilities::binaries`), because `cargo test` does
# not build *another* package's binary for this package's integration test and
# a stale `herd-runner` under test is worse than no test. That build is not
# idempotent with `cargo test`'s own: `cargo build -p herd-hub -p herd-runner
# -p herdctl` resolves a narrower feature set than `cargo test --workspace`
# does — dev-dependencies unify features across the workspace — so it has real
# work to do on a tree that was just built. Measured here on 2026-10-27: right
# after `cargo build --workspace --all-targets` it still recompiled rustix,
# linux-raw-sys and the herd-hub binary, 9.7 s at every core it could take,
# *inside* a test process whose hub and runner were already running against a
# 30 s PATIENCE, with every other test thread of that binary queued behind it
# on the build directory's lock. Hence ORCH-107: timeouts under load that were
# the suite's own build, not the product.
#
# Running the identical command here, before any test starts, makes it a
# 0.06 s no-op in the tests. It weakens nothing: the tests still run it and
# still fail if it fails — they simply no longer compile while timing a hub.
# The profile has to match, or the two builds invalidate each other.
#
# It is advisory, and failing it is not an exit. The tests own this build:
# they run it themselves and fail on it with the real diagnostic, and the
# suite's own exit status and skip report are what this script exists to
# report honestly (D27, D179). Exiting here on a failed prebuild would print
# neither, which is the one thing the skip report must never do.
prebuild=(cargo build --offline -p herd-hub -p herd-runner -p herdctl)
for arg in "$@"; do
  case "$arg" in
    --) break ;;
    --release) prebuild+=(--release) ;;
  esac
done
echo "Building the binaries the real_herd tests drive, so that they do not:"
echo "  ${prebuild[*]}"
"${prebuild[@]}" || echo "  (it did not build; the tests will build it and say why)"
echo

# cargo's stdout goes through `tee`, because the `ignored` lines are on it
# and nothing else in this project would ever see them. stderr is left
# exactly where it was: the parse below needs stdout only, and merging the
# two would reorder a compile diagnostic against the test report for every
# reader of a direct run. $PIPESTATUS[0] is cargo's own status; `tee`'s is
# not the suite's verdict.
if [ "$own_flag" = no ]; then
  cargo test --workspace --no-fail-fast "$@" | tee "$cargo_output"
else
  cargo test --workspace "$@" | tee "$cargo_output"
fi
status=${PIPESTATUS[0]}

# One line per distinct ignored assertion. `test <name> ... ignored` and
# `test <name> ... ignored, <reason>` are libtest's two forms; the name is
# taken up to the FIRST ` ... ignored` that ends the line or is followed by
# a comma, so a reason that itself contains the words is still read as the
# reason. A Rust test path carries no spaces, and a doc-test name
# (`src/lib.rs - Foo::bar (line 10)`) carries no `...`, so the first match
# is the marker.
ignored_lines() {
  awk '
    /^test .+ \.\.\. ignored(,.*)?$/ {
      name = $0
      sub(/^test /, "", name)
      sub(/ \.\.\. ignored(,.*)?$/, "", name)
      rest = substr($0, length(name) + 6)   # past "test " and the name
      reason = substr(rest, length(" ... ignored") + 1)
      sub(/^,[ \t]*/, "", reason)
      sub(/[ \t]+$/, "", reason)
      if (reason == "") print "HERD-IGNORED-UNEXPLAINED: " name
      else print "HERD-IGNORED: " name ": " reason
      next
    }
  ' "$1" | sort -u
}
ignored_lines "$cargo_output" > "$ignored_log"
ignored=$(grep '^HERD-IGNORED: ' "$ignored_log" 2>/dev/null)
unexplained=$(grep '^HERD-IGNORED-UNEXPLAINED: ' "$ignored_log" 2>/dev/null)
ignored_count=0
unexplained_count=0
[ -z "$ignored" ] || ignored_count=$(printf '%s\n' "$ignored" | wc -l | tr -d ' ')
[ -z "$unexplained" ] || unexplained_count=$(printf '%s\n' "$unexplained" | wc -l | tr -d ' ')

# One line per distinct skip: a test may run in several binaries, and both
# streams of one test print the same line.
skips=$(sort -u "$HERD_SKIP_LOG" 2>/dev/null)
count=0
if [ -n "$skips" ]; then
  count=$(printf '%s\n' "$skips" | wc -l | tr -d ' ')
fi

bar="=============================================================="
rule() { echo "$bar"; }
how_to_make_them_real() {
  rule
  echo "Run them inside a delegated cgroup scope to make them real:"
  echo "  cargo test --workspace --no-run"
  echo "  systemd-run --user --scope -p Delegate=yes \\"
  echo "    target/debug/deps/confinement-<hash> --nocapture"
}

echo
if [ "$status" -eq 0 ]; then
  # cargo finished: every binary it launched ran to the end, so the log
  # holds every skip of every test this run selected.
  if [ -z "$skips" ] && [ "$narrowed" = no ]; then
    echo "Every assertion ran on this host: nothing was skipped."
  elif [ -z "$skips" ]; then
    echo "No assertion was skipped on this host by the tests this run"
    echo "selected — but it was narrowed by its own arguments ($*), so it"
    echo "says nothing about the rest of the suite."
  else
    rule
    echo " $count assertion(s) COULD NOT RUN on this host and were skipped."
    echo " A green run does NOT mean these were checked."
    [ "$narrowed" = no ] || echo " This run was narrowed by its arguments ($*): there may be more."
    rule
    printf '%s\n' "$skips"
    how_to_make_them_real
    rule
  fi
elif [ -z "$skips" ]; then
  rule
  echo " SKIPS UNKNOWN on this host: the suite did not finish (cargo test"
  echo " exited $status) and nothing reached the skip log."
  echo " That is NOT the same as nothing having been skipped: a binary that"
  echo " never ran, or never compiled, records no skip of its own."
  echo " Fix the failure and rerun before quoting a skip count for this host."
  rule
else
  rule
  echo " AT LEAST $count assertion(s) COULD NOT RUN on this host and were"
  echo " skipped, and the suite did not finish (cargo test exited $status),"
  echo " so this list is a floor and not a count."
  rule
  printf '%s\n' "$skips"
  how_to_make_them_real
  rule
fi

# --- and the compile-time ones, in their own block (ORCH-202) ------------
# Printed apart from the skips above and never folded into them: these did
# not run because of the tree, not because of the host, and a reader who
# cannot tell the two apart cannot tell "this host cannot check it" from
# "nobody is checking it".
if [ "$unexplained_count" -gt 0 ]; then
  rule
  echo " REFUSED: $unexplained_count test(s) carry #[ignore] WITH NO REASON."
  echo " An assertion nobody makes must at least say why nobody makes it."
  echo " Give each one a reason — #[ignore = \"why, and what would undo it\"]"
  echo " — so that it can be listed. The gate is RED on every line below"
  echo " (scripts/gate.sh step 7) and no expectation file can match them."
  rule
  printf '%s\n' "$unexplained"
  rule
fi
if [ "$ignored_count" -gt 0 ]; then
  rule
  echo " $ignored_count assertion(s) DID NOT RUN AT ALL: they carry #[ignore]."
  echo " This is not 'this host could not' — it is 'nobody is checking this"
  echo " until someone acts'. A green run does NOT mean these were checked."
  [ "$narrowed" = no ] || echo " This run was narrowed by its arguments ($*): there may be more."
  rule
  printf '%s\n' "$ignored"
  rule
  echo "Run one on purpose with --ignored, naming its test target:"
  echo "  cargo test -p <package> --test <target> -- --ignored"
  rule
fi

# The census marker, for the same reason as `test suite: PASSED` below: a
# run that did not finish did not print every binary's `ignored` lines
# either, so its list is a floor. A compile error is the case that makes
# this matter — the binary never ran, so it never announced what it was
# not checking.
if [ "$status" -eq 0 ] && [ "$narrowed" = no ]; then
  echo "ignored census: COMPLETE ($ignored_count ignored, $unexplained_count without a reason)"
else
  echo "ignored census: INCOMPLETE — a floor and not a count ($ignored_count ignored, $unexplained_count without a reason)"
fi

# The last line is a marker scripts/report-skips.sh reads back out of a
# captured run, so `test suite: PASSED` on its own means the whole suite
# finished and nothing else does.
if [ "$status" -eq 0 ] && [ "$narrowed" = no ]; then
  echo "test suite: PASSED"
elif [ "$status" -eq 0 ]; then
  echo "test suite: PASSED (a part of the suite: narrowed by its arguments)"
else
  echo "test suite: FAILED (cargo test exited $status)" >&2
fi
# A skip is information, not a failure: the exit code is the suite's own.
exit "$status"
