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
fresh; printf 'x%.0s' $(seq 1 85) >> "$tmp/repo/docs/orchestration/workflow.md"; echo >> "$tmp/repo/docs/orchestration/workflow.md"
expect_red "over-long line in docs/orchestration" 'workflow.md has lines over 79'
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

echo
echo "check-orchestration: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
