#!/usr/bin/env bash
# Turns the suite's skipped assertions into something nobody can miss.
#
# M4's confinement tests touch the real kernel. A host without cgroup v2
# delegation — a GitHub Actions runner, for one — cannot make some of the
# assertions at all. Such an assertion is skipped with its reason rather than
# failed, because a suite that is red on a host that is merely different is a
# suite people learn to ignore. The risk is the opposite one: a green run that
# nobody reads as "this was not actually checked here".
#
# So every skip prints `HERD-SKIPPED: <test>: <why>`, scripts/test.sh prints
# the skip log it collected, and this script lifts those lines into a GitHub
# warning annotation and the run's summary. Usage:
#
#   ./scripts/report-skips.sh <test-output-file>
#
# This step runs with `if: always()`, so it also runs after a *red* suite —
# and a red suite's output is not a complete list of skips. PR #86's failing
# `check` job printed "nothing was skipped" from a run whose later test
# binaries never started, on a runner that reports 15 skips when green. So
# the output is first read for the marker scripts/test.sh prints last:
# a plain `test suite: PASSED` means the whole suite finished and the list
# below it is the whole list; anything else — `test suite: FAILED`, the
# marker's narrowed form, or no marker at all because the job was cancelled
# or never got that far — means the list is a floor at best. Then nothing is
# claimed that was not observed (D27).
#
# This script never fails the job: the suite's own step owns that verdict.

set -uo pipefail

output="${1:?usage: report-skips.sh <test-output-file>}"
if [ ! -f "$output" ]; then
  echo "report-skips.sh: $output does not exist" >&2
  exit 1
fi

if grep -q '^test suite: PASSED$' "$output"; then
  finished=yes
else
  finished=no
fi

# The same skip is printed on both stdout and stderr, and a test may run more
# than once in a suite, so identical lines collapse to one.
skips=$(grep -h '^HERD-SKIPPED: ' "$output" | sort -u)
count=0
if [ -n "$skips" ]; then
  count=$(printf '%s\n' "$skips" | wc -l | tr -d ' ')
fi

# Outside GitHub Actions there is no summary file; stdin is read either way,
# so that a producer piping into this never meets a closed pipe.
summary() {
  cat >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
}

if [ "$finished" = yes ] && [ -z "$skips" ]; then
  echo "Every assertion ran on this host: nothing was skipped."
  summary <<'MARKDOWN'
### Herd test suite
Every assertion ran on this host. Nothing skipped.
MARKDOWN
  exit 0
fi

if [ -z "$skips" ]; then
  echo "=============================================================="
  echo " SKIPS UNKNOWN on this host: this output is not a complete run of"
  echo " the suite — no plain \`test suite: PASSED\` from scripts/test.sh —"
  echo " and no HERD-SKIPPED line reached it either."
  echo " That is NOT the same as nothing having been skipped: a test binary"
  echo " that never ran records no skip of its own. See the suite's own step."
  echo "=============================================================="
  summary <<'MARKDOWN'
### Herd test suite: skips unknown on this host
This output is not a complete run of the suite, so it does **not** say
which assertions this host could make. An empty skip list from an
incomplete run means *unknown*, not *none* (D27). See the suite's own step.
MARKDOWN
  echo "::warning title=Herd skip list unknown::this is not a complete run of the suite, so the assertions this host could not make were not collected"
  exit 0
fi

if [ "$finished" = yes ]; then
  headline="$count assertion(s) COULD NOT RUN on this host and were skipped."
  caveat=" A green run does NOT mean these were checked."
else
  headline="AT LEAST $count assertion(s) COULD NOT RUN on this host and were skipped."
  caveat=" This output is not a complete run of the suite, so the list below is a floor and not a count."
fi

echo "=============================================================="
echo " $headline"
echo "$caveat"
echo "=============================================================="
printf '%s\n' "$skips"
echo "=============================================================="

{
  if [ "$finished" = yes ]; then
    echo "### Herd test suite: $count assertion(s) skipped on this host"
    echo
    echo "A green run does **not** mean these were checked. They need a"
    echo "capability this runner does not have."
  else
    echo "### Herd test suite: at least $count assertion(s) skipped on this host"
    echo
    echo "This is not a complete run of the suite, so this is a floor and"
    echo "not a count: a test binary that never ran recorded no skip."
  fi
  echo
  printf '%s\n' "$skips" | sed 's/^HERD-SKIPPED: /- /'
} | summary

# One annotation per skip, so each shows on the run rather than only in the log.
printf '%s\n' "$skips" | while IFS= read -r line; do
  message=${line#HERD-SKIPPED: }
  # Annotation messages are one line; newlines would end the command early.
  message=$(printf '%s' "$message" | tr '\n' ' ')
  echo "::warning title=Herd assertion skipped on this host::${message}"
done

# Still a success: the skip is information, not a failure.
exit 0
