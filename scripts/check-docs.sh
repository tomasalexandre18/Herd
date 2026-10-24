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
for f in CLAUDE.md README.md docs/*.md docs/orchestration/*.md \
         docs/orchestration/*/*.md; do
  [ -f "$f" ] || continue
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

# --- Orchestration (docs/orchestration, CLAUDE.md roles, decisions numbers) ---
# Added 2026-10-24 by the orchestration remediation. Every check here names
# what is wrong and where; none of them rewrites a file.

# Decision headers: `## <date> — D<n> <title>`, numbers unique, no D-NEXT
# placeholder left behind (herd-merger allocates it at integration), gaps
# listed so a reader can tell a held number from a lost one.
dec=docs/decisions.md
bad=$(grep -nE '^## ' "$dec" | grep -vE '^[0-9]+:## [0-9]{4}-[0-9]{2}-[0-9]{2} — D([0-9]+|-NEXT) [^ ]')
[ -z "$bad" ] || note "$dec has malformed decision headers (want: ## YYYY-MM-DD — D<n> <title>):
$bad"
next=$(grep -nE '^## [0-9-]+ — D-NEXT ' "$dec")
[ -z "$next" ] || note "$dec still has a D-NEXT placeholder; herd-merger allocates the number at integration:
$next"
dups=$(grep -oE '^## [0-9-]+ — D[0-9]+' "$dec" | sed 's/.*— D//' | sort -n | uniq -d)
[ -z "$dups" ] || note "$dec uses a decision number twice (a number is an identifier, D148; append with the next free one, never renumber):
$(printf '    D%s\n' $dups)"
known_gaps="143 154"
max=$(grep -oE '^## [0-9-]+ — D[0-9]+' "$dec" | sed 's/.*— D//' | sort -n | tail -1)
if [ -n "$max" ]; then
  gaps=$(seq 1 "$max" | grep -vxF -f <(grep -oE '^## [0-9-]+ — D[0-9]+' "$dec" | sed 's/.*— D//') | tr '\n' ' ')
  for g in $gaps; do
    case " $known_gaps " in *" $g "*) ;; *) echo "note: D$g is a gap in $dec (held by an open PR, or lost); gaps stay (D148)";; esac
  done
fi

# CLAUDE.md names the multi-agent organisation and nothing it points at is
# missing. "works alone" is the 2026-10-13 wording the remediation replaced.
! grep -qiE 'works alone' CLAUDE.md || note "CLAUDE.md says the orchestrator works alone; the contract is multi-agent (docs/orchestration/workflow.md)"
for p in docs/orchestration/board.md docs/orchestration/workflow.md .claude/agents; do
  grep -qF "$p" CLAUDE.md || note "CLAUDE.md no longer points at $p"
  [ -e "$p" ] || note "$p is named in CLAUDE.md and does not exist"
done
for a in planner step merger release reviewer; do
  f=.claude/agents/herd-$a.md
  if [ ! -f "$f" ]; then note "$f is missing (role herd-$a)"; continue; fi
  grep -qE "^name: herd-$a$" "$f" || note "$f: frontmatter name must be herd-$a"
  grep -qE '^description: .' "$f" || note "$f: frontmatter description is missing"
done
for h in .claude/hooks/*.sh; do [ -x "$h" ] || note "$h is not executable"; done
if command -v perl >/dev/null 2>&1 && [ -f .claude/settings.json ]; then
  perl -MJSON::PP -0777 -ne 'JSON::PP->new->decode($_)' .claude/settings.json 2>/dev/null || note ".claude/settings.json is not valid JSON"
fi

# The board: six sections in order, every row an ORCH id with eight cells
# and a known state. A row that drops a cell loses its owner or its origin.
board=docs/orchestration/board.md
if [ -f "$board" ]; then
  want="Waiting Owed Open Active Blocked Next"
  got=$(grep -oE '^## (Waiting|Owed|Open|Active|Blocked|Next)' "$board" | sed 's/^## //' | tr '\n' ' ' | sed 's/ $//')
  [ "$got" = "$want" ] || note "$board sections are '$got', want '$want' in that order"
  rows=$(grep -nE '^\| ORCH-' "$board" | awk -F'|' 'NF != 10 { print "    line " $0 }' | cut -c1-60)
  [ -z "$rows" ] || note "$board rows must have exactly eight cells:
$rows"
  states=$(grep -E '^\| ORCH-' "$board" | awk -F'|' '{ gsub(/^[ \t]+|[ \t]+$/, "", $6); if ($6 !~ /^(waiting|owed|open|active|blocked|done|refused)$/) print "    " $2 "state " $6 }')
  [ -z "$states" ] || note "$board rows with an unknown state (waiting owed open active blocked done refused):
$states"
  ids=$(grep -oE '^\| ORCH-[0-9]+' "$board" | sort | uniq -d)
  [ -z "$ids" ] || note "$board uses an id twice: $ids"
else
  note "$board is missing: it is the one list of open work (CLAUDE.md, invariant 8)"
fi

# Handoffs are dated, one per task.
for f in docs/orchestration/history/*.md; do
  case "$(basename "$f")" in README.md|[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]-*.md) ;; *) note "$f: a handoff is named YYYY-MM-DD-<task>.md";; esac
done

if [ "$fail" -eq 0 ]; then
  echo "docs ok"
else
  echo "docs check failed" >&2
fi
exit "$fail"
