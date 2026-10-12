#!/usr/bin/env bash
# Enforces the docs discipline of CLAUDE.md invariant 8, so that it does not
# have to be enforced by hand in review.
set -uo pipefail

# Column counting below is per character, not per byte.
export LC_ALL=C.UTF-8

fail=0
note() { printf '%s\n' "$1" >&2; fail=1; }

# CLAUDE.md stays short enough to be read before every session.
limit=150
n=$(wc -l < CLAUDE.md)
[ "$n" -le "$limit" ] || note "CLAUDE.md is $n lines, limit $limit"

# status.md stays at most one page: it is state, not a journal.
limit=90
n=$(wc -l < docs/status.md)
[ "$n" -le "$limit" ] || note "docs/status.md is $n lines, limit $limit (it is state, not a journal)"

# Prose wraps at 79 columns. Table rows and fenced code are exempt.
# awk selects the lines; the width is measured by the shell, because awk
# implementations disagree on `length` for UTF-8 (mawk, the default awk on
# the CI runner, counts bytes, so `—` and `≤` would fail a wrapped line).
for f in CLAUDE.md README.md docs/*.md; do
  long=$(awk '
    /^```/ { fence = !fence; next }
    fence { next }
    /^\|/ { next }
    { printf "%d\t%s\n", FNR, $0 }
  ' "$f" | while IFS="$(printf '\t')" read -r lineno text; do
    [ "${#text}" -le 79 ] ||
      printf '    %s:%s (%d cols)\n' "$f" "$lineno" "${#text}"
  done)
  [ -z "$long" ] || note "$f has lines over 79 columns:
$long"
done

# Every must in the coverage table names a milestone. This is the property
# that keeps the plan honest; a must with no home is a must that gets lost.
orphans=$(awk -F'|' '
  /^\| E-[0-9]+ \| must \|/ {
    gsub(/^[ \t]+|[ \t]+$/, "", $5)
    if ($5 == "" || $5 == "-" || $5 == "—") {
      gsub(/^[ \t]+|[ \t]+$/, "", $2)
      print "    " $2 " is a must with no milestone"
    }
  }
' docs/coverage.md)
[ -z "$orphans" ] || note "docs/coverage.md leaves musts unplaced:
$orphans"

if [ "$fail" -eq 0 ]; then
  echo "docs ok"
else
  echo "docs check failed" >&2
fi
exit "$fail"
