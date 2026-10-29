#!/usr/bin/env bash
# Enforces the docs discipline of CLAUDE.md invariant 8, so that it does not
# have to be enforced by hand in review.
#
# Adding a rule here: check-orchestration.sh runs this whole script 38 times,
# each over a full copy of the real repository, because a control only ever
# seen green is not known to work and each case needs its own fresh corpus.
# The cost of any rule that is linear in the corpus is therefore multiplied
# by 38 in the `docs` CI job, which has a 5-minute cap. One such rule already
# took that job past the cap (2026-10-27; see the 79-column check below).
# Measure a new whole-corpus rule against the suite, not against one run.
set -uo pipefail

# A stable locale for grep, sed and sort below. The column count has its own
# locale (LC_ALL=C, see the 79-column check) and does not rely on this one.
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
#
# The width is counted inside awk, in one pass per file. It used to be
# measured by the shell, one `read` per line, because awk implementations
# disagree on `length` for UTF-8: mawk, the default awk on the CI runner,
# counts bytes, so a wrapped line holding `—` or `≤` looked over-long. That
# shell loop was the whole cost of this script -- 5.9 s of 5.9 s over a
# 37k-line corpus -- and check-orchestration.sh paid it 25 times over, which
# took the `docs` job to 5m12s against its 5-minute cap (PR #100, 2026-10-27).
#
# Counting it here instead needs no assumption about awk's locale support:
# under LC_ALL=C every awk works on bytes, and the character count of valid
# UTF-8 is its byte count minus its continuation bytes (0x80-0xBF), since
# every encoded character contributes exactly one leading byte. Verified to
# give byte-identical findings to the shell loop under gawk and under mawk,
# over a corpus seeded with 150 violations built from `—`, `≤`, `é` and a
# 4-byte emoji. The multibyte property has its own cases in
# check-orchestration.sh, so a relapse to byte counting goes red.
#
# A fence opens at any indent and closes only at the indent it opened on,
# and a table row is exempt at any indent (ORCH-117). Both anchors were at
# column 1 until 2026-10-27, so a fenced block or a table nested inside a
# list item was width-checked like prose: three fences and one table, on
# that one day, in handoffs whose evidence has to stay verbatim. Closing
# only at the opening indent is what keeps a nested fence -- the shape a
# handoff quoting a fenced block produces -- from closing the outer one
# early; a plain toggle at any indent would. The indent is counted in
# characters, so a fence opened with a tab and closed with spaces does not
# pair; no such fence exists in the corpus, and it is a known limit, not a
# guard. What this does not change: a stray unbalanced fence at column 1
# already swallows the rest of its file, before and after this change, so
# the column-1 anchor was never what protected against that. All 92 files
# are balanced today; an END check for an unbalanced fence is ORCH-122.
for f in CLAUDE.md README.md docs/*.md docs/orchestration/*.md \
         docs/orchestration/*/*.md; do
  [ -f "$f" ] || continue
  long=$(LC_ALL=C awk '
    /^[[:space:]]*```/ {
      ind = match($0, /[^ \t]/) - 1
      if (!fence) { fence = 1; find = ind }
      else if (ind == find) { fence = 0 }
      next
    }
    fence { next }
    /^[[:space:]]*\|/ { next }
    {
      line = $0
      cols = length(line) - gsub(/[\200-\277]/, "", line)
      if (cols > 79) printf "    %s:%d (%d cols)\n", FILENAME, FNR, cols
    }
  ' "$f")
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
# D143 and D154 were raced for by parallel branches and never taken.
# D170 was held by PR #67, which was closed unmerged as obsolete, so the
# number is spent and the gap is permanent (decisions.md, D148: a number
# is an identifier, gaps stay). Listing it here accepts the gap; it does
# not renumber anything. A note nobody can clear is a note nobody reads.
known_gaps="143 154 170"
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

# herd-merger's handoff carries the landing record: the merge commit message
# that replaces the pull-request body as the audit trail of a push to `main`
# (2026-10-28, templates/landing.md). The commit message is not in the tree
# a gate reads, so the handoff copy is what can be checked here -- its seven
# sections, and the four header fields filled with real values rather than
# the template's placeholders. A gate verdict with no sha, or a skip count
# with no list, is the failure this exists to catch (D27, invariant 5).
#
# Cost: deliberately per-file, not whole-corpus. The date in the filename is
# compared as a string first, so every handoff written under the
# pull-request method is skipped without being opened, and what is left is
# read by one awk for the whole set -- one landing per batch. check-docs.sh
# is run 38 times by check-orchestration.sh against the `docs` job's
# 5-minute cap (see the note at the top of this file).
# The constant is a floor, and it moved once: the method landed on
# 2026-10-28 and a merge handoff of that same date was already on `main`,
# written under the pull-request method and not rewritable (history is
# append-only, and 68 handoffs carry the old field names). The first
# landing under the method is dated 2026-10-29 or later -- it comes after
# the last pull request, which is what carried this rule -- so that is
# the floor. Moving it is one line, and the rule is a no-op until a
# landing is dated on or after it.
landing_from=2026-10-29
landings=""
for f in docs/orchestration/history/*-merge-*.md; do
  [ -f "$f" ] || continue
  b=${f##*/}      # not basename: one fork per file, paid 38 times
  [ "${b%%-merge-*}" \< "$landing_from" ] && continue
  landings="$landings $f"
done
if [ -n "$landings" ]; then
  bad_landings=$(LC_ALL=C awk '
    BEGIN {
      nh = split("What changed and why|Branches in this batch|Evidence|Left out|Decisions allocated|Board rows|CI run", head, "|")
      hex = "[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]"
      run = "[0-9][0-9][0-9][0-9][0-9][0-9]"
    }
    function want(c, s) { if (!c) miss = miss (miss ? ", " : "") s }
    function flush() {
      if (file == "") return
      miss = ""
      for (i = 1; i <= nh; i++) want(index(seen, "|" i "|"), "section \"## " head[i] "\"")
      want(batch, "a filled **Batch:** line")
      want(gate,  "a filled **Gate:** line naming the sha it ran on")
      want(skips, "a filled **Skips:** line")
      want(list,  "the skips named (HERD-SKIPPED lines, or \"no skip\")")
      want(ci,    "a filled **CI run:** line with the run id")
      if (miss != "") printf "    %s: %s\n", file, miss
    }
    FNR == 1 { flush(); seen = ""; batch = gate = skips = list = ci = 0; file = FILENAME }
    { for (i = 1; i <= nh; i++) if ($0 ~ "^## " head[i] "[ \t]*$") seen = seen "|" i "|" }
    /HERD-SKIPPED/ || /no skip/ { list = 1 }
    /\*\*Batch:\*\*/  { if ($0 !~ /</) batch = 1 }
    /\*\*Gate:\*\*/   { if ($0 !~ /</ && $0 ~ hex) gate = 1 }
    /\*\*Skips:\*\*/  { if ($0 !~ /</) skips = 1 }
    /\*\*CI run:\*\*/ { if ($0 !~ /</ && ($0 ~ run || $0 ~ /none/)) ci = 1 }
    END { flush() }
  ' $landings)
  [ -z "$bad_landings" ] || note "a landing on or after $landing_from must carry the filled landing record of docs/orchestration/templates/landing.md:
$bad_landings"
fi

# A brief kept under history/ names the agent's own worktree. The guard
# exempts agents by design (docs/orchestration/permissions.md "Live
# verification"), so nothing but the brief keeps one out of the
# orchestrator's checkout: in V11 a brief that named no worktree let its
# agent edit and stage docs/status.md there (ORCH-031). A brief is a file
# named <task>-brief.<ext>, as templates/brief.md says; a handoff whose task
# name merely contains "brief" is not one. The worktree is an
# absolute path written right after the word "worktree"; `git worktree add`
# does not count, because that names the command, not the destination. A
# brief whose agent writes nothing in the repository says read-only on its
# Base line instead -- "everything else is read-only" on the Write only
# line is a different statement and does not exempt it.
checkout=/mnt/ssd_980/herd-project
for f in docs/orchestration/history/*-brief.* docs/orchestration/history/*/*-brief.*; do
  [ -f "$f" ] || continue
  named=$(grep -oE '[Ww]orktree[^/]{0,4}/[^ ,;:)`"]+' "$f" | sed -E 's|^[^/]*||; s|[.`]+$||')
  if [ -z "$named" ]; then
    grep -E '^[^A-Za-z]*Base[^A-Za-z]' "$f" | grep -qi 'read-only' ||
      note "$f: a brief must name the agent's own worktree as an absolute path, or say read-only on its Base line (ORCH-031, V11)"
  elif printf '%s\n' "$named" | grep -qxF "$checkout"; then
    note "$f: the worktree it names is $checkout, the orchestrator's checkout; an agent works in its own worktree (ORCH-031, V11)"
  fi
done

# --- A release's notes are a file in the tree, and not a paste (ORCH-162) --
# v0.4.0 was published carrying none of the notes written for it, both
# operator cautions included. `release.yml` assembles its body out of
# fragments it generates plus the standing notice it quotes, and the human
# half was a person remembering to paste it -- the person briefed for the
# tag, who was not briefed for the paste. The cautions were put on the page
# by hand afterwards, because the permission system refuses that write to
# every agent.
#
# So the notes became a file, `deploy/release-notes/<version>.md`, and this
# is the half that makes it a guard rather than a second convention: the
# notes for the version the workspace carries must exist before a tag can be
# pushed. A bump that forgets them goes red here, on a branch, instead of
# after a page is public.
#
# Cost: four greps over two small files, flat in the corpus. This is not one
# of the whole-corpus rules the note at the top of this file warns about, and
# the 38 runs in check-orchestration.sh pay it 38 times at that price.
notes_dir=deploy/release-notes

# The version the way `flake.nix` reads it (`workspace.package.version`),
# which is the same figure the release's file names and the image tag come
# from -- the first bare `version = "..."` in Cargo.toml. Every other
# `version` in that file sits inside a dependency table, braced and not at
# the start of a line, so the anchor is what keeps them out.
version=$(sed -n 's/^version = "\([^"]*\)".*/\1/p' Cargo.toml | head -1)
notes="$notes_dir/$version.md"
if [ -z "$version" ]; then
  note "Cargo.toml has no workspace.package.version, so the release-notes rule cannot name the file it wants (ORCH-162)"
elif [ ! -f "$notes" ]; then
  note "$notes does not exist: the workspace is at $version and a release's notes are a file in the tree, not a paste into the page (ORCH-162). Write it, or put the version back."
elif ! grep -q '[^[:space:]]' "$notes"; then
  note "$notes is empty: an empty notes file publishes an empty half, which is the defect this rule exists to catch (ORCH-162)."
fi
# Only the workspace's version is required, and nothing above iterates over
# $notes_dir. Every other file in it is a release that has already shipped,
# and keeping those is the point -- that history is why the notes live in the
# tree and not in somebody's clipboard.

# The other half of the same rule: the file has to reach the page. These
# assertions read the real workflow and never a copy of its logic -- a test
# that re-implements what it checks is how two green tests once covered a
# dead feature. What they cannot do is run it: the release factory is
# GitHub's, by design.
wf=.github/workflows/release.yml
if [ ! -f "$wf" ]; then
  note "$wf is missing: it is what puts a release's notes on its page"
else
  grep -q "$notes_dir/" "$wf" ||
    note "$wf never reads $notes_dir/: the notes file would exist and reach no page (ORCH-162)"

  # Absent means no half, as it already does for the dev preamble: the piece
  # is truncated to empty before it is filled, so a run with no notes file --
  # a rehearsal with an empty tag input, a tag replayed from a later commit --
  # publishes without the half instead of failing. The body is never allowed
  # to cost a tag its binaries (D162; three tags once published nothing).
  grep -qE '^[[:space:]]*:[[:space:]]+>[[:space:]]*body-notes\.md[[:space:]]*$' "$wf" ||
    note "$wf must truncate body-notes.md to empty before it fills it, so an absent notes file publishes no half instead of failing the release (ORCH-162)"

  # Every assembly of the body, in order: the half the workflow generates,
  # then the notes, then the standing notice. There is more than one `cat`,
  # because the half promising the image archive is added only once the
  # archive is attached and the later `cat` replaces what the earlier one
  # published -- so a piece left out of any one of them is a piece the
  # release loses. Each is checked, and not just the two that exist today.
  asm=$(grep -nE '^[[:space:]]*cat .*>[[:space:]]*release-body\.md[[:space:]]*$' "$wf")
  if [ -z "$asm" ]; then
    note "$wf assembles no release-body.md any more, so the release-notes rule cannot tell where the notes go (ORCH-162)"
  else
    wrong=$(printf '%s\n' "$asm" \
      | grep -vE 'body-head\.md.*body-notes\.md.*body-tail\.md' \
      | sed 's/^/    /')
    [ -z "$wrong" ] || note "$wf assembles a release body without the notes between the generated half and the standing notice (ORCH-162):
$wrong"
  fi
fi

if [ "$fail" -eq 0 ]; then
  echo "docs ok"
else
  echo "docs check failed" >&2
fi
exit "$fail"
