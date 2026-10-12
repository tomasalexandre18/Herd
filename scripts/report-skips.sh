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
# So every skip prints `HERD-SKIPPED: <test>: <why>`, CI runs the suite with
# --nocapture, and this script lifts those lines into a GitHub warning
# annotation and the run's summary. Usage:
#
#   ./scripts/report-skips.sh <test-output-file>

set -uo pipefail

output="${1:?usage: report-skips.sh <test-output-file>}"
if [ ! -f "$output" ]; then
  echo "report-skips.sh: $output does not exist" >&2
  exit 1
fi

# The same skip is printed on both stdout and stderr, and a test may run more
# than once in a suite, so identical lines collapse to one.
skips=$(grep -h '^HERD-SKIPPED: ' "$output" | sort -u)

if [ -z "$skips" ]; then
  echo "Every assertion ran on this host: nothing was skipped."
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    echo "### Herd test suite" >> "$GITHUB_STEP_SUMMARY"
    echo "Every assertion ran on this host. Nothing skipped." \
      >> "$GITHUB_STEP_SUMMARY"
  fi
  exit 0
fi

count=$(printf '%s\n' "$skips" | wc -l | tr -d ' ')
echo "=============================================================="
echo " $count assertion(s) COULD NOT RUN on this host and were skipped."
echo " A green run does NOT mean these were checked."
echo "=============================================================="
printf '%s\n' "$skips"
echo "=============================================================="

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    echo "### Herd test suite: $count assertion(s) skipped on this host"
    echo
    echo "A green run does **not** mean these were checked. They need a"
    echo "capability this runner does not have."
    echo
    printf '%s\n' "$skips" | sed 's/^HERD-SKIPPED: /- /'
  } >> "$GITHUB_STEP_SUMMARY"
fi

# One annotation per skip, so each shows on the run rather than only in the log.
printf '%s\n' "$skips" | while IFS= read -r line; do
  message=${line#HERD-SKIPPED: }
  # Annotation messages are one line; newlines would end the command early.
  message=$(printf '%s' "$message" | tr '\n' ' ')
  echo "::warning title=Herd assertion skipped on this host::${message}"
done

# Still a success: the skip is information, not a failure.
exit 0
