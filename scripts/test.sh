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
# Usage: ./scripts/test.sh [extra cargo test arguments]
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root" || exit 1

export HERD_SKIP_LOG="${HERD_SKIP_LOG:-$root/target/herd-skips.txt}"
mkdir -p "$(dirname "$HERD_SKIP_LOG")" || exit 1
: > "$HERD_SKIP_LOG"

cargo test --workspace "$@"
status=$?

# One line per distinct skip: a test may run in several binaries, and both
# streams of one test print the same line.
skips=$(sort -u "$HERD_SKIP_LOG" 2>/dev/null)

echo
if [ -z "$skips" ]; then
  echo "Every assertion ran on this host: nothing was skipped."
else
  count=$(printf '%s\n' "$skips" | wc -l | tr -d ' ')
  echo "=============================================================="
  echo " $count assertion(s) COULD NOT RUN on this host and were skipped."
  echo " A green run does NOT mean these were checked."
  echo "=============================================================="
  printf '%s\n' "$skips"
  echo "=============================================================="
  echo "Run them inside a delegated cgroup scope to make them real:"
  echo "  cargo test --workspace --no-run"
  echo "  systemd-run --user --scope -p Delegate=yes \\"
  echo "    target/debug/deps/confinement-<hash> --nocapture"
fi

if [ "$status" -eq 0 ]; then
  echo "test suite: PASSED"
else
  echo "test suite: FAILED (cargo test exited $status)" >&2
fi
# A skip is information, not a failure: the exit code is the suite's own.
exit "$status"
