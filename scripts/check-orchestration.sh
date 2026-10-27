#!/usr/bin/env bash
# Tests of the organisation's own controls: check-docs.sh must go red on
# each defect it claims to catch (a control only ever seen green is not
# known to work), and the two guard hooks must block what they claim and
# let legitimate inspection through. Runs on a scratch copy; the repository
# is never modified. No toolchain needed.
#
# Usage: ./scripts/check-orchestration.sh
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/herd-orch-check.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
pass=0; fail=0
ok()   { pass=$((pass+1)); printf 'ok    %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf 'FAIL  %s\n%s\n' "$1" "$2"; }

# A fresh copy of the files check-docs.sh reads.
fresh() {
  rm -rf "$tmp/repo"; mkdir -p "$tmp/repo"
  (cd "$root" && cp -r --parents CLAUDE.md README.md docs scripts .claude/agents .claude/hooks .claude/settings.json "$tmp/repo")
}
# expect_red <name> <grep pattern of the message> : check-docs.sh on $tmp/repo fails and says why
expect_red() {
  out=$(cd "$tmp/repo" && ./scripts/check-docs.sh 2>&1); rc=$?
  if [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -qE -- "$2"; then ok "$1"
  else bad "$1" "    exit=$rc, output:
$(printf '%s\n' "$out" | sed 's/^/    /' | head -8)"; fi
}
expect_green() {
  out=$(cd "$tmp/repo" && ./scripts/check-docs.sh 2>&1); rc=$?
  if [ "$rc" -eq 0 ]; then ok "$1"; else bad "$1" "$(printf '%s\n' "$out" | sed 's/^/    /' | head -8)"; fi
}

echo "== check-docs.sh goes green on the repository as it is"
fresh; expect_green "unmodified copy passes"

echo "== check-docs.sh goes red on each defect it claims to catch"
fresh; printf '\n## 2026-10-24 — D27 A duplicate of an old number\n\nBody.\n' >> "$tmp/repo/docs/decisions.md"
expect_red "duplicate decision number D27" 'number twice'
fresh; printf '\n## 2026-10-24 — D-NEXT A placeholder left behind\n\nBody.\n' >> "$tmp/repo/docs/decisions.md"
expect_red "D-NEXT placeholder" 'D-NEXT placeholder'
fresh; printf '\n## D999 No date on this header\n\nBody.\n' >> "$tmp/repo/docs/decisions.md"
expect_red "malformed decision header" 'malformed decision headers'
fresh; sed -i 's/^- \*\*The orchestrator\*\* (the main Claude session)/- **The orchestrator** works alone and/' "$tmp/repo/CLAUDE.md"
expect_red "CLAUDE.md saying the orchestrator works alone" 'works alone'
fresh; sed -i 's#docs/orchestration/board.md#docs/orchestration/boards.md#g' "$tmp/repo/CLAUDE.md"
expect_red "CLAUDE.md no longer pointing at the board" 'no longer points at docs/orchestration/board.md'
fresh; rm "$tmp/repo/.claude/agents/herd-merger.md"
expect_red "a missing role file" 'herd-merger.md is missing'
fresh; sed -i 's/^name: herd-step$/name: herd-steps/' "$tmp/repo/.claude/agents/herd-step.md"
expect_red "a role whose name does not match its file" 'name must be herd-step'
fresh; sed -i 's/^## Owed — /## Owing — /' "$tmp/repo/docs/orchestration/board.md"
expect_red "board sections out of shape" 'sections are'
fresh; printf '| ORCH-999 | a row with too few cells | origin | owner | owed |\n' >> "$tmp/repo/docs/orchestration/board.md"
expect_red "board row with missing cells" 'exactly eight cells'
fresh; printf '| ORCH-998 | item | origin | owner | pending | next | dep | ev |\n' >> "$tmp/repo/docs/orchestration/board.md"
expect_red "board row with an unknown state" 'unknown state'
fresh; printf '| ORCH-001 | a second ORCH-001 | origin | owner | owed | next | dep | ev |\n' >> "$tmp/repo/docs/orchestration/board.md"
expect_red "board id used twice" 'uses an id twice'
fresh; rm "$tmp/repo/docs/orchestration/board.md"
expect_red "board missing" 'board.md is missing'
fresh; printf '# late\n' > "$tmp/repo/docs/orchestration/history/undated-task.md"
expect_red "undated handoff" 'named YYYY-MM-DD'
# A brief kept under history/ must name the agent's own worktree (ORCH-031,
# V11). The rule has to be able to go red, and the read-only exemption has
# to be narrow enough that the "everything else is read-only" boilerplate
# every brief carries does not trigger it.
H="$tmp/repo/docs/orchestration/history"
fresh; printf 'Role: herd-step\nBase: origin/main at abc1234; branch x\nWrite only: docs/x.md; everything else is read-only\n' > "$H/2026-10-26-noworktree-brief.txt"
expect_red "a brief naming no worktree (the Write only boilerplate does not exempt it)" "own worktree as an absolute path"
fresh; printf 'Role: herd-step\nBase: origin/main at abc1234; branch x\nWorktree: /mnt/ssd_980/herd-project\n' > "$H/2026-10-26-checkout-brief.txt"
expect_red "a brief naming the orchestrator's checkout as its worktree" "orchestrator's checkout"
fresh; printf 'Role: herd-step\nBase: origin/main at abc1234; branch x\nWorktree: git worktree add -b x /scratch/x/wt origin/main\n' > "$H/2026-10-26-command-brief.txt"
expect_red "a brief whose only path is inside a git worktree add command" "own worktree as an absolute path"
fresh; printf 'Role: herd-step\nBase: origin/main at abc1234; branch x\nWorktree: /scratch/orch-031/wt (yours alone)\nWrite only: docs/x.md; everything else is read-only\n' > "$H/2026-10-26-named-brief.txt"
expect_green "a brief naming an absolute worktree of its own"
fresh; printf 'Role: herd-release\nBase: the repository at /scratch/x/wt, read-only.\nGates: none (read-only).\n' > "$H/2026-10-26-readonly-brief.txt"
expect_green "a read-only brief, which writes nothing in the repository"
fresh; printf '# a handoff, not a brief\n\nNo worktree named anywhere in it.\n' > "$H/2026-10-26-orch-031-brief-worktree.md"
expect_green "a handoff whose task name merely contains the word brief"
fresh; printf 'x%.0s' $(seq 1 85) >> "$tmp/repo/docs/orchestration/workflow.md"; echo >> "$tmp/repo/docs/orchestration/workflow.md"
expect_red "over-long line in docs/orchestration" 'workflow.md has lines over 79'
# The 79 columns are characters, not bytes. check-docs.sh counts them inside
# awk, where `length` is bytes on the CI runner's mawk; it corrects for that
# by subtracting the UTF-8 continuation bytes. Nothing above would notice if
# that correction were dropped -- an 85-character ASCII line is 85 bytes
# either way -- so the property gets its own cases, one on each side of the
# limit, with a 3-byte and a 4-byte character.
em=$(printf '\342\200\224')        # U+2014 EM DASH, 3 bytes, 1 column
goat=$(printf '\360\237\220\220')  # U+1F410 GOAT, 4 bytes, 1 column
W="$tmp/repo/docs/orchestration/workflow.md"
fresh; { printf "$em%.0s" $(seq 1 79); echo; } >> "$W"
expect_green "a 79-character line of 3-byte characters (237 bytes) is within the limit"
fresh; { printf "$em%.0s" $(seq 1 80); echo; } >> "$W"
expect_red "an 80-character line of 3-byte characters is over the limit" 'workflow.md has lines over 79'
fresh; { printf "$goat%.0s" $(seq 1 79); echo; } >> "$W"
expect_green "a 79-character line of 4-byte characters (316 bytes) is within the limit"
fresh; chmod -x "$tmp/repo/.claude/hooks/orchestrator-guard.sh"
expect_red "hook not executable" 'not executable'
fresh; printf '{ not json' > "$tmp/repo/.claude/settings.json"
expect_red "settings.json invalid" 'not valid JSON'

echo "== the number is an identifier: the historical gaps stay silent, a new gap is noted, nothing is rewritten"
fresh; before=$(sha256sum "$tmp/repo/docs/decisions.md")
out=$(cd "$tmp/repo" && ./scripts/check-docs.sh 2>&1)
printf '%s' "$out" | grep -qE 'note: D143 ' && bad "D143 is a known gap and must not be noted" "$out" || ok "known gaps D143, D154 not noted"
[ "$before" = "$(sha256sum "$tmp/repo/docs/decisions.md")" ] && ok "decisions.md unchanged by the check" || bad "decisions.md was modified by the check" ""

echo "== two branches allocating the same number collide at integration, and the check says so"
# Fake corpus: main has D1..D3; branch A and branch B both wrote D-NEXT; the
# merger allocates A as D4, then B must see D4 taken.
# Without -e, a failed cd would leave every relative path below pointing at the
# real repository, so each step that decides where we are is fatal.
mkdir -p "$tmp/corpus" && cd "$tmp/corpus" || { echo "FATAL: cannot enter $tmp/corpus; touching nothing" >&2; exit 1; }
git init -q . && cp -r "$root/scripts" . && mkdir -p docs/orchestration/history .claude/agents .claude/hooks && cp "$root/CLAUDE.md" "$root/README.md" . && cp -r "$root/docs/orchestration" docs/ && cp "$root/.claude/agents/"*.md .claude/agents/ && cp "$root/.claude/hooks/"*.sh .claude/hooks/ && cp "$root/.claude/settings.json" .claude/ && cp "$root/docs/coverage.md" "$root/docs/status.md" docs/ \
  || { echo "FATAL: corpus setup failed" >&2; exit 1; }
printf '# Decisions\n\n## 2026-10-01 — D1 One\n\nBody.\n\n## 2026-10-02 — D2 Two\n\nBody.\n\n## 2026-10-03 — D3 Three\n\nBody.\n' > docs/decisions.md
git add -A >/dev/null && git -c user.name=t -c user.email=t@t commit -qm base
git checkout -qb a && printf '\n## 2026-10-04 — D-NEXT From A\n\nBody.\n' >> docs/decisions.md && git -c user.name=t -c user.email=t@t commit -qam a
git checkout -q master 2>/dev/null || git checkout -q main; git checkout -qb b && printf '\n## 2026-10-04 — D-NEXT From B\n\nBody.\n' >> docs/decisions.md && git -c user.name=t -c user.email=t@t commit -qam b
# merger integrates A: allocate D4
git checkout -q a && sed -i 's/— D-NEXT From A/— D4 From A/' docs/decisions.md && git -c user.name=t -c user.email=t@t commit -qam "allocate D4"
out=$(./scripts/check-docs.sh 2>&1); [ $? -eq 0 ] && ok "branch A allocated D4 and passes" || bad "branch A should pass after allocation" "$out"
# a careless integrator allocates D4 on B too, then merges A: the duplicate is caught
git checkout -q b && sed -i 's/— D-NEXT From B/— D4 From B/' docs/decisions.md && git -c user.name=t -c user.email=t@t commit -qam "allocate D4 again"
# a textual merge of a into b conflicts on the file; this is what resolving it by keeping both entries produces
printf '# Decisions\n\n## 2026-10-01 — D1 One\n\nBody.\n\n## 2026-10-02 — D2 Two\n\nBody.\n\n## 2026-10-03 — D3 Three\n\nBody.\n\n## 2026-10-04 — D4 From A\n\nBody.\n\n## 2026-10-04 — D4 From B\n\nBody.\n' > docs/decisions.md
out=$(./scripts/check-docs.sh 2>&1); printf '%s' "$out" | grep -q 'D4' && [ "$(printf '%s' "$out" | grep -c 'number twice')" -ge 1 ] && ok "the collision D4/D4 is rejected with the number named" || bad "collision not rejected" "$out"
# the right move: B takes D5, the gap list stays empty
sed -i 's/— D4 From B/— D5 From B/' docs/decisions.md
out=$(./scripts/check-docs.sh 2>&1); [ $? -eq 0 ] && ok "B reallocated to D5 passes" || bad "D5 should pass" "$out"
cd "$root" || exit 1

echo "== orchestrator-guard.sh: blocks the main session's writes, lets agents and inspection through"
G=.claude/hooks/orchestrator-guard.sh; R=$root
hook() { # <expected exit> <json>
  printf '%s' "$2" | "$G" >/dev/null 2>&1; rc=$?
  [ "$rc" -eq "$1" ] && ok "guard exit $1: $(printf '%s' "$2" | sed "s|$R|<repo>|g" | cut -c1-90)" || bad "guard expected $1 got $rc" "    $(printf '%s' "$2" | sed "s|$R|<repo>|g" | cut -c1-120)"
}
hook 2 "{\"tool_name\":\"Edit\",\"cwd\":\"$R\",\"tool_input\":{\"file_path\":\"$R/docs/status.md\"}}"
hook 2 "{\"tool_name\":\"Write\",\"cwd\":\"$R\",\"tool_input\":{\"file_path\":\"docs/status.md\"}}"
hook 0 "{\"tool_name\":\"Edit\",\"cwd\":\"$R\",\"agent_id\":\"abc\",\"tool_input\":{\"file_path\":\"$R/docs/status.md\"}}"
hook 0 "{\"tool_name\":\"Edit\",\"cwd\":\"$R\",\"agent_type\":\"herd-step\",\"tool_input\":{\"file_path\":\"$R/docs/status.md\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"agent_type\":\"herd-merger\",\"tool_input\":{\"command\":\"git merge --no-ff x\"}}"
hook 0 "{\"tool_name\":\"Write\",\"cwd\":\"$R\",\"tool_input\":{\"file_path\":\"/tmp/claude-1002/x/brief.md\"}}"
hook 0 "{\"tool_name\":\"Edit\",\"cwd\":\"$R\",\"tool_input\":{\"file_path\":\"$R/.claude/worktrees/agent-1/docs/x.md\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"cd x && git commit -m hi\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"gh pr merge 70 --squash\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git merge --no-ff feature\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"sed -i 's/a/b/' docs/status.md\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"cat > docs/status.md <<EOF\\nx\\nEOF\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git tag -a v1 -m x\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"gh release create v1\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git push origin main\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"echo x > $R/CLAUDE.md\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git log --oneline -5; git diff origin/main --stat\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"gh pr view 70; gh pr checks 70\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"cat > /tmp/claude-1002/x/brief.md <<EOF\\nx\\nEOF\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"grep -n 'git commit' docs/decisions.md | head\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git worktree add -b t /tmp/claude-1002/x/wt origin/main\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"./scripts/check-docs.sh; ls $R/docs\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"agent_id\":\"a1\",\"tool_input\":{\"command\":\"git commit -m hi\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"agent_type\":\"herd-step\",\"tool_input\":{\"command\":\"git commit -m hi\"}}"
hook 0 "{\"tool_name\":\"Edit\",\"cwd\":\"$R\",\"agent_type\":\"herd-merger\",\"tool_input\":{\"file_path\":\"$R/docs/status.md\"}}"
hook 0 "not json at all"
touch "$G.DISABLED.tmp"; mv "$G.DISABLED.tmp" .claude/hooks/DISABLED
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git commit -m hi\"}}"
rm -f .claude/hooks/DISABLED
echo "== the holes the V7 review found (ORCH-004), now closed"
# A ".." segment must not let a path pose as an agent worktree, and must not
# hide the repository prefix behind another directory.
hook 2 "{\"tool_name\":\"Edit\",\"cwd\":\"$R\",\"tool_input\":{\"file_path\":\"$R/.claude/worktrees/../../docs/x.md\"}}"
hook 2 "{\"tool_name\":\"Write\",\"cwd\":\"$R\",\"tool_input\":{\"file_path\":\"/tmp/..$R/docs/x.md\"}}"
hook 2 "{\"tool_name\":\"Edit\",\"cwd\":\"$R/docs\",\"tool_input\":{\"file_path\":\"../CLAUDE.md\"}}"
# ... while a real agent worktree still passes, "." and ".." included.
hook 0 "{\"tool_name\":\"Edit\",\"cwd\":\"$R\",\"tool_input\":{\"file_path\":\"$R/.claude/worktrees/a-1/./docs/x.md\"}}"
hook 0 "{\"tool_name\":\"Edit\",\"cwd\":\"$R\",\"tool_input\":{\"file_path\":\"$R/.claude/worktrees/a-1/../a-2/docs/x.md\"}}"
# A relative redirection while the shell stands in the checkout.
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"echo x > a.md\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"cd docs && echo x > a.md\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"/tmp/claude-1002/x\",\"tool_input\":{\"command\":\"cd $R/docs && echo x > a.md\"}}"
# Git verbs that rewrite a checkout without committing anything.
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git reset --hard\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git checkout -- docs/status.md\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git stash\"}}"
hook 2 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git clean -fd\"}}"
# An agent is still free to do all of it.
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"agent_id\":\"a1\",\"tool_input\":{\"command\":\"cd docs && echo x > a.md\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"agent_type\":\"herd-merger\",\"tool_input\":{\"command\":\"git reset --hard\"}}"
# The orchestrator's own reading and scratchpad writing stay possible.
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git log -1 > /tmp/claude-1002/x/out\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"./scripts/check-docs.sh 2>&1 | tail -5\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"wc -l docs/status.md >/dev/null\"}}"
# The guard needs perl: without it it fails open, so its absence is a failure
# of this host, not a pass (V7, finding 7).
command -v perl >/dev/null 2>&1 && ok "perl is present, the guard can parse" \
  || bad "no perl: orchestrator-guard.sh fails open on this host" ""
# The guard's logging switches must not sit in the tree: .log-raw writes whole
# hook payloads to disk (V7, finding 5).
if [ -e .claude/hooks/.log-to ] || [ -e .claude/hooks/.log-raw ]; then
  bad "a logging switch is live in .claude/hooks" "    remove .log-to / .log-raw"
else ok "no logging switch left in .claude/hooks"; fi
git check-ignore -q .claude/hooks/.log-to && git check-ignore -q .claude/hooks/.log-raw \
  && ok "the logging switches are gitignored" \
  || bad "the logging switches are not gitignored (invariant 7)" ""

echo "== known bypasses, documented in docs/orchestration/permissions.md (these pass on purpose)"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"git -c a=b commit -m hi\"}}"
hook 0 "{\"tool_name\":\"Bash\",\"cwd\":\"$R\",\"tool_input\":{\"command\":\"/usr/bin/git commit -m hi\"}}"

echo "== release-guard.sh"
RG=.claude/hooks/release-guard.sh
rg() { printf '{"tool_input":{"command":"%s"}}' "$2" | "$RG" >/dev/null 2>&1; rc=$?; [ "$rc" -eq "$1" ] && ok "release exit $1: $2" || bad "release expected $1 got $rc" "    $2"; }
rg 2 "git tag -a v0.4.0 -m x"; rg 2 "git push origin v0.4.0"; rg 2 "git push --tags"
rg 2 "gh release create v0.4.0"; rg 2 "gh workflow run release.yml"; rg 2 "docker load < hub.tar"; rg 2 "sudo systemctl restart herd-runner"
rg 0 "git tag -l"; rg 0 "git tag --list 'v*'"; rg 0 "git tag -n9 v0.3.7"; rg 0 "git tag"; rg 2 "git tag -l; git tag -a v1 -m x"; rg 2 "git tag v0.4.0"
rg 0 "git push origin cut-v0.4.0"; rg 0 "gh run list --workflow release.yml"; rg 0 "git log v0.3.7..origin/main --oneline"

echo "== the skip report never claims more than it saw (ORCH-071, D27)"
# A fake `cargo` on PATH stands in for the suite: it records its arguments,
# writes the skip lines it was given to $HERD_SKIP_LOG, and exits with the
# status it was given. So each combination of (the suite finished, something
# was skipped) is reproducible in a second and with no toolchain — including
# the one that lied: red, with an empty skip log because the binary carrying
# the skips never ran.
mkdir -p "$tmp/bin"
cat > "$tmp/bin/cargo" <<'FAKE'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$FAKE_CARGO_ARGV"
[ -n "${FAKE_CARGO_SKIPS:-}" ] && printf '%s\n' "$FAKE_CARGO_SKIPS" >> "$HERD_SKIP_LOG"
exit "${FAKE_CARGO_STATUS:-0}"
FAKE
chmod +x "$tmp/bin/cargo"
export FAKE_CARGO_ARGV="$tmp/cargo-argv"
S1='HERD-SKIPPED: ceiling_is_a_cgroup: no cgroup v2 delegation on this host'
S2='HERD-SKIPPED: io_max_is_written: the workspace is on ZFS'

# run_test_sh <cargo exit status> <skip lines, possibly empty> [args...]
run_test_sh() {
  local st=$1 sk=$2; shift 2
  out=$(cd "$tmp/repo" && PATH="$tmp/bin:$PATH" FAKE_CARGO_STATUS="$st" \
    FAKE_CARGO_SKIPS="$sk" ./scripts/test.sh "$@" 2>&1); rc=$?
}
# verdict <name> <expected exit> <must appear> [<must not appear>]
verdict() {
  local name=$1 want=$2 must=$3 never=${4:-} why=''
  [ "$rc" -eq "$want" ] || why="    exit=$rc, expected $want"
  printf '%s' "$out" | grep -qF -- "$must" || why="$why
    missing from the report: $must"
  if [ -n "$never" ] && printf '%s' "$out" | grep -qF -- "$never"; then
    why="$why
    said what it must not say: $never"
  fi
  if [ -z "$why" ]; then ok "$name"; else bad "$name" "$why
$(printf '%s\n' "$out" | sed 's/^/    /' | tail -14)"; fi
}

fresh
# The defect: a red run whose skip log is empty because most of the suite
# never started. "Unknown" is the only honest answer; the reassurance is a
# lie, and the failure stays a failure.
run_test_sh 101 ''
verdict "test.sh: red with an empty skip log reports it unknown" 101 \
  "SKIPS UNKNOWN on this host" "nothing was skipped"
# A red run that did collect skips knows a floor, not a count.
run_test_sh 101 "$S1
$S2"
verdict "test.sh: red with skips reports them as a floor" 101 \
  "AT LEAST 2 assertion(s) COULD NOT RUN" "nothing was skipped"
printf '%s' "$out" | grep -qF -- "$S2" && ok "test.sh: the floor lists the skips it has" \
  || bad "test.sh: the floor must still list the skips" "$out"
# And the green cases keep their exact, unqualified wording.
run_test_sh 0 ''
verdict "test.sh: green with an empty skip log says nothing was skipped" 0 \
  "Every assertion ran on this host: nothing was skipped." "UNKNOWN"
run_test_sh 0 "$S1
$S2"
verdict "test.sh: green with skips reports the real count" 0 \
  " 2 assertion(s) COULD NOT RUN on this host and were skipped." "AT LEAST"
# report-skips.sh decides completeness on the marker test.sh prints last, so
# the two must agree on its wording; they are read by CI one after the other.
verdict "test.sh: a finished run prints the marker report-skips.sh reads" 0 \
  "test suite: PASSED"
run_test_sh 101 ''
verdict "test.sh: an unfinished run prints the failed marker" 101 \
  "test suite: FAILED (cargo test exited 101)"

# A run narrowed by its own arguments ran a part of the suite, so its empty
# skip log is not evidence that every assertion ran.
run_test_sh 0 '' -p herd-proto
verdict "test.sh: a narrowed green run does not speak for the whole suite" 0 \
  "narrowed by its own arguments" "Every assertion ran on this host"
run_test_sh 0 "$S1" -p herd-runner
verdict "test.sh: a narrowed green run says its count may be short" 0 \
  "there may be more"
# ... and its verdict marker says so too, so that a capture of it cannot be
# read back as a whole run by report-skips.sh.
if printf '%s\n' "$out" | grep -qF -- "test suite: PASSED (a part of the suite" \
  && ! printf '%s\n' "$out" | grep -qxF -- "test suite: PASSED"; then
  ok "test.sh: a narrowed run's marker is not a plain PASSED"
else
  bad "test.sh: a narrowed run must not print the whole-suite marker" \
    "    $(printf '%s\n' "$out" | tail -1)"
fi
# --no-fail-fast by default, so a failing binary does not truncate the log —
# and never twice, which cargo rejects outright.
run_test_sh 0 ''
[ "$(grep -cx -- '--no-fail-fast' "$FAKE_CARGO_ARGV")" = 1 ] \
  && ok "test.sh: --no-fail-fast is passed by default" \
  || bad "test.sh: --no-fail-fast missing" "    $(tr '\n' ' ' < "$FAKE_CARGO_ARGV")"
run_test_sh 0 '' --no-fail-fast
[ "$(grep -cx -- '--no-fail-fast' "$FAKE_CARGO_ARGV")" = 1 ] \
  && ok "test.sh: the caller's own --no-fail-fast is not doubled" \
  || bad "test.sh: --no-fail-fast doubled, cargo would refuse" "    $(tr '\n' ' ' < "$FAKE_CARGO_ARGV")"

# report-skips.sh reads a captured run in CI, where it runs with if: always()
# and so also sees red ones. It decides on the marker test.sh prints last.
# run_report <file contents>
run_report() {
  printf '%s\n' "$1" > "$tmp/test-output.txt"
  out=$(cd "$tmp/repo" && ./scripts/report-skips.sh "$tmp/test-output.txt" 2>&1); rc=$?
}
run_report "running 3 tests
test suite: FAILED (cargo test exited 101)"
verdict "report-skips.sh: a red run with no skip line reports it unknown" 0 \
  "SKIPS UNKNOWN on this host" "nothing was skipped"
run_report "$S1
$S2
test suite: FAILED (cargo test exited 101)"
verdict "report-skips.sh: a red run with skips reports them as a floor" 0 \
  "AT LEAST 2 assertion(s)" "nothing was skipped"
run_report "running 3 tests
test suite: PASSED"
verdict "report-skips.sh: a finished run with no skips says nothing was skipped" 0 \
  "Every assertion ran on this host: nothing was skipped." "UNKNOWN"
run_report "$S1
$S2
test suite: PASSED"
verdict "report-skips.sh: a finished run reports the real count" 0 \
  " 2 assertion(s) COULD NOT RUN on this host and were skipped." "AT LEAST"
# A capture of a narrowed run is not a whole run either, however green.
run_report "running 5 tests
test suite: PASSED (a part of the suite: narrowed by its arguments)"
verdict "report-skips.sh: a narrowed run is not read as a whole one" 0 \
  "SKIPS UNKNOWN on this host" "nothing was skipped"
# No marker at all — a cancelled or killed job — is not a finished run either.
run_report "error: linking with cc failed"
verdict "report-skips.sh: an output with no verdict marker is not a pass" 0 \
  "SKIPS UNKNOWN on this host" "nothing was skipped"


echo "== ops/agents: the launcher gives a session what it needs (ORCH-068)"
# The live launcher on the host and the shipped one drifted: the shipped copy
# set no GH_CONFIG_DIR, so a session started by it had no GitHub login at all
# — no `gh pr create`, no `gh pr merge`, and no `git push`, because `origin`
# is https and the credential helper is `gh auth git-credential`. Nothing in
# the repository noticed. These assertions are what notices.
#
# The real state root is never touched: the scripts are copied and the one
# hardcoded root line is rewritten to a scratch directory, exactly as the
# check-docs.sh cases above run on a scratch copy.
E="$tmp/ops/agents"; mkdir -p "$E"
cp "$root/ops/agents/env.sh" "$root/ops/agents/session.sh" "$E/"
sandroot=$tmp/stateroot
sed -i "s|^HERD_STATE_ROOT=.*|HERD_STATE_ROOT=$sandroot|" "$E/env.sh"
grep -qxF "HERD_STATE_ROOT=$sandroot" "$E/env.sh" \
  && ok "env.sh keeps its state root on one rewritable line" \
  || bad "env.sh: the hardcoded HERD_STATE_ROOT line was not found" \
         "    expected a line 'HERD_STATE_ROOT=/...' in ops/agents/env.sh"

# Source it from a hostile environment: someone else's HOME, chatOnS markers,
# an ambient API key — all of which a real relapse would carry in.
dump=$(env -i PATH="$PATH" HOME=/wrong/home CHATONS_ROOT=/x HC=1 \
  ANTHROPIC_API_KEY=sk-not-a-real-key ANTHROPIC_BASE_URL=http://elsewhere \
  bash -c '
    . "$1" || exit 9
    for v in HERD_STATE_ROOT HERD_CLAUDE_ROOT HOME CLAUDE_CONFIG_DIR \
             XDG_CONFIG_HOME XDG_RUNTIME_DIR TMPDIR TEMP CARGO_HOME \
             HISTFILE GH_CONFIG_DIR PATH ANTHROPIC_API_KEY \
             ANTHROPIC_BASE_URL CHATONS_ROOT HC; do
      printf "%s=%s\n" "$v" "${!v-<unset>}"
    done' _ "$E/env.sh" 2>&1)
envis() { # <NAME=value> : the sourced environment has exactly this
  printf '%s\n' "$dump" | grep -qxF "$1" && ok "env.sh: $1" \
    || bad "env.sh: expected $1" "$(printf '%s\n' "$dump" | sed 's/^/    /')"
}
envval() { printf '%s\n' "$dump" | sed -n "s/^$1=//p"; }
envis "HOME=$sandroot/claude/home"
envis "CLAUDE_CONFIG_DIR=$sandroot/claude/config"
envis "XDG_CONFIG_HOME=$sandroot/claude/home/.config"
envis "XDG_RUNTIME_DIR=$sandroot/claude/run"
envis "TMPDIR=$sandroot/claude/tmp"
envis "TEMP=$sandroot/claude/tmp"
envis "HISTFILE=$sandroot/claude/home/.bash_history"
# Under the Claude root, not beside it: the registry in use on this host is
# at <root>/claude/cargo and the shipped file once said <root>/cargo.
envis "CARGO_HOME=$sandroot/claude/cargo"
envis 'ANTHROPIC_API_KEY=<unset>'
envis 'ANTHROPIC_BASE_URL=<unset>'
envis 'CHATONS_ROOT=<unset>'
envis 'HC=<unset>'
for d in home config run tmp cargo; do
  [ -d "$sandroot/claude/$d" ] && ok "env.sh creates $d" \
    || bad "env.sh did not create $sandroot/claude/$d" ""
done

# GH_CONFIG_DIR is the whole point of ORCH-068. It must be set, absolute, a
# `gh` directory, and deliberately OUTSIDE the state root: the login belongs
# to the host user and is shared like the Claude Code binary, not copied in.
gh_dir=$(envval GH_CONFIG_DIR)
case $gh_dir in
  "$sandroot"|"$sandroot"/*) bad "GH_CONFIG_DIR is inside the state root" "    $gh_dir" ;;
  /*/gh) ok "GH_CONFIG_DIR is an absolute gh directory outside the state root" ;;
  *) bad "GH_CONFIG_DIR must be an absolute path ending in /gh" "    got '$gh_dir'; a session without it cannot open or merge a pull request, nor push to an https remote" ;;
esac
# An operator may point it somewhere else without editing the file.
alt=$(env -i PATH="$PATH" HOME=/wrong/home HERD_GH_CONFIG_DIR=/opt/ghconf \
  bash -c '. "$1" >/dev/null 2>&1; printf "%s" "${GH_CONFIG_DIR-<unset>}"' _ "$E/env.sh")
[ "$alt" = /opt/ghconf ] && ok "HERD_GH_CONFIG_DIR overrides the default" \
  || bad "HERD_GH_CONFIG_DIR ignored" "    got '$alt'"

# PATH: the four directories in front, in order, and what was inherited still
# behind them, or this file stops being sourceable anywhere but this host.
path_val=$(envval PATH)
case $path_val in
  "$sandroot/claude/tools/dtach/bin:/run/wrappers/bin:/etc/profiles/per-user/"*"/bin:/run/current-system/sw/bin:"*)
    ok "PATH puts dtach, the wrappers, the per-user profile and sw/bin in front" ;;
  *) bad "PATH prefix is wrong" "    $path_val" ;;
esac
printf '%s' "$path_val" | grep -qF ":$PATH" \
  && ok "PATH keeps what it inherited behind the four" \
  || bad "PATH dropped the inherited entries; the guard below needs coreutils" "    $path_val"

# The guard still refuses a state path that escapes the root, and says which.
# A second copy with one path moved out: assigning CARGO_HOME in the caller
# would prove nothing, since env.sh assigns it unconditionally.
cp "$E/env.sh" "$tmp/env-escape.sh"
sed -i 's|^export CARGO_HOME=.*|export CARGO_HOME=/var/empty/herd-escaped|' "$tmp/env-escape.sh"
out=$(env -i PATH="$PATH" HOME=/wrong/home \
  bash -c '. "$1" || exit 7' _ "$tmp/env-escape.sh" 2>&1); rc=$?
[ "$rc" -eq 7 ] && printf '%s' "$out" | grep -q 'state path outside' \
  && ok "the guard refuses a state path outside the root" \
  || bad "the guard let a path outside the root through" "    exit=$rc $out"

# session.sh: the same facts, from the launcher a person actually types.
mkdir -p "$tmp/fakebin"
cat > "$tmp/fakebin/claude" <<'FAKE'
#!/usr/bin/env bash
echo "FAKE-CLAUDE-RAN $*"
FAKE
chmod +x "$tmp/fakebin/claude"
printf '{}\n' > "$sandroot/claude/config/.credentials.json"
mkdir -p "$tmp/gh-empty" "$tmp/gh-logged-in"
printf 'github.com:\n    user: someone\n' > "$tmp/gh-logged-in/hosts.yml"
runsession() { # <GH_CONFIG_DIR>
  out=$(env -i PATH="$PATH" HOME=/wrong/home HERD_GH_CONFIG_DIR="$1" \
    HERD_CLAUDE_BIN="$tmp/fakebin/claude" \
    bash "$E/session.sh" --version </dev/null 2>&1); rc=$?
}
runsession "$tmp/gh-logged-in"
{ [ "$rc" -eq 0 ] && printf '%s' "$out" | grep -q 'FAKE-CLAUDE-RAN --version' \
  && ! printf '%s' "$out" | grep -q 'hosts.yml'; } \
  && ok "session.sh starts the session and says nothing when gh is logged in" \
  || bad "session.sh misbehaved with a logged-in gh" "    exit=$rc $out"
runsession "$tmp/gh-empty"
{ printf '%s' "$out" | grep -q 'cannot open or merge a pull request' \
  && printf '%s' "$out" | grep -q 'FAKE-CLAUDE-RAN'; } \
  && ok "session.sh warns, and still starts, when gh has no login" \
  || bad "session.sh did not warn about a gh directory with no hosts.yml" "    exit=$rc $out"
mv "$sandroot/claude/config/.credentials.json" "$tmp/creds-aside"
runsession "$tmp/gh-logged-in"
{ [ "$rc" -eq 1 ] && printf '%s' "$out" | grep -q 'no .credentials.json'; } \
  && ok "session.sh refuses without credentials when no terminal can log in" \
  || bad "session.sh should refuse without credentials and no tty" "    exit=$rc $out"
mv "$tmp/creds-aside" "$sandroot/claude/config/.credentials.json"
# Nothing in this section may name a token; only paths belong in the repo.
grep -qE 'oauth_token|ghp_|github_pat_' "$root/ops/agents/env.sh" "$root/ops/agents/session.sh" \
  && bad "a credential appears in ops/agents (invariant 7)" "" \
  || ok "ops/agents names paths only, never a token (invariant 7)"
echo
echo "check-orchestration: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
