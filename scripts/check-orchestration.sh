#!/usr/bin/env bash
# Tests of the organisation's own controls: check-docs.sh must go red on
# each defect it claims to catch (a control only ever seen green is not
# known to work), the two guard hooks must block what they claim and let
# legitimate inspection through, scripts/gate.sh -- the local gate a
# landing depends on -- must go red on every way a local run can lie, and
# scripts/githooks/pre-push -- the last machine between an agent and `main`
# now that no pull request stands in the way -- must refuse every bad push
# and slow down no good one. Runs on a scratch copy; the repository is never
# modified. No toolchain needed: the gate section stubs nix, cargo and the
# suite, and the pre-push section stubs only the receipts, so each script's
# own logic is what is under test and not the toolchain.
#
# The pre-push section drives real `git push` against a local bare remote in
# $tmp: no network, no credential, nothing outside $tmp is written.
#
# Cost: each expect_red/expect_green runs check-docs.sh over a fresh full
# copy of the real repository -- 32 whole-corpus runs as this file stands.
# That multiplier is the constraint on anyone adding an all-docs rule to
# check-docs.sh: a rule linear in the corpus is paid 32 times here, and the
# `docs` CI job is capped at 5 minutes. Adding a case here costs one more
# run of every rule. The fixtures are deliberately not trimmed: a control
# measured on a toy corpus is not known to work on the real one.
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
# Fenced code and table rows are exempt, and each exemption has its own
# cases below because nothing above told today's anchors from loosened
# ones: a herd-reviewer pass (ORCH-117, 2026-10-27) loosened each in turn
# and this harness stayed byte-identical to its control over six runs.
# Only outright removal of an exemption was caught, incidentally, by the
# 85-character case. The five cases pin both directions: the exemption
# exists at all (3, 4), it applies at any indent (1, 2 -- the defect
# ORCH-117 names, red before the anchor moved), and it does not close a
# fence early (5), which is the regression a naive `fence = !fence` at
# any indent causes. Known limit, no case: a fence opened with a tab and
# closed with spaces does not pair, because the indent is counted in
# characters; no such fence exists in the corpus.
wide=$(printf 'y%.0s' $(seq 1 100))
fresh; printf '\n1. An item with its evidence:\n\n   ```text\n   %s\n   ```\n' "$wide" >> "$W"
expect_green "an indented fenced block exempts its over-long content"
fresh; printf '\n2. An item with its table:\n\n   | %s | %s |\n' "$wide" "$wide" >> "$W"
expect_green "an indented table row is exempt, as one at column 1 is"
fresh; { printf '\n```text\n'; printf 'z%.0s' $(seq 1 1600); printf '\n```\n'; } >> "$W"
expect_green "a fence at column 1 exempts a 1600-character line"
fresh; printf '\n| %s | %s |\n' "$wide" "$wide" >> "$W"
expect_green "a table row at column 1 is exempt over 79 columns"
# Exactly one indented fence marker inside the outer block, deliberately
# unpaired: a plain toggle survives an even number of them, so an even
# fixture would pass under the naive patch too and assert nothing.
fresh; printf '\n```markdown\n   ```text\n%s\n```\n' "$wide" >> "$W"
expect_green 'an indented ``` inside a fence at column 1 does not close it'
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

echo "== gate.sh: the local gate, red on each way a local run can lie"
# A gate only ever seen green is not known to work. Every case below runs the
# real scripts/gate.sh in a scratch repository whose steps are stubs, so the
# gate's own logic — not the toolchain — is what is under test.
gatedir="$tmp/gate"
gate_fresh() {
  rm -rf "$gatedir" "$tmp/gate-receipts"
  mkdir -p "$gatedir/scripts/expected-skips" "$gatedir/bin" "$tmp/gate-receipts"
  cp "$root/scripts/gate.sh" "$gatedir/scripts/gate.sh"
  for s in check-docs.sh check-orchestration.sh; do
    printf '#!/usr/bin/env bash\necho "%s stub ok"\nexit "${HERD_STUB_DOCS:-0}"\n' "$s" \
      > "$gatedir/scripts/$s"
  done
  cat > "$gatedir/scripts/report-skips.sh" <<'EOS'
#!/usr/bin/env bash
grep -h '^HERD-SKIPPED: ' "$1" | sort -u
exit 0
EOS
  cat > "$gatedir/scripts/test.sh" <<'EOS'
#!/usr/bin/env bash
[ -n "${HERD_STUB_SKIPS:-}" ] && cat "$HERD_STUB_SKIPS"
if [ "${HERD_STUB_SUITE:-pass}" != pass ]; then
  echo "test suite: FAILED (cargo test exited 101)"; exit 101
fi
[ "${HERD_STUB_NOPASS:-0}" = 1 ] || echo "test suite: PASSED"
exit 0
EOS
  # A `nix` that is not nix: `flake check` answers, and `develop --command`
  # runs what it is given. The gate must reach its steps through it.
  cat > "$gatedir/bin/nix" <<'EOS'
#!/usr/bin/env bash
case "$1" in
  flake) exit "${HERD_STUB_FLAKE:-0}" ;;
  develop) shift; [ "${1:-}" = --command ] && shift; exec "$@" ;;
esac
exit 0
EOS
  cat > "$gatedir/bin/cargo" <<'EOS'
#!/usr/bin/env bash
case "$1" in
  fmt) exit "${HERD_STUB_FMT:-0}" ;;
  clippy) exit "${HERD_STUB_CLIPPY:-0}" ;;
esac
exit 0
EOS
  chmod +x "$gatedir/scripts/"*.sh "$gatedir/bin/"*
  (cd "$gatedir" && git init -q . \
    && git add -A >/dev/null \
    && git -c user.name=t -c user.email=t@t commit -qm base) >/dev/null
  # The skip list the stub suite prints, and the expectation it is compared
  # with, both live OUTSIDE the repository: a case that edited them inside it
  # would be testing the dirty-tree refusal by accident.
  printf 'HERD-SKIPPED: a_needs_cgroup_delegation: no delegated subtree here\nHERD-SKIPPED: b_needs_bfq: no device offers bfq\n' > "$tmp/gate-skips"
  printf '# expectation\nHERD-SKIPPED: a_needs_cgroup_delegation: no delegated subtree here\nHERD-SKIPPED: b_needs_bfq: no device offers bfq\n' > "$tmp/gate-expected"
  rm -f "$tmp/gate-expected-load"
}
# gate_run <extra env...> -- <gate args...>
gate_run() {
  env -i PATH="$gatedir/bin:/usr/bin:/bin:$PATH" HOME="$tmp" \
    HERD_GATE_RECEIPTS="$tmp/gate-receipts" \
    HERD_GATE_EXPECTED="$tmp/gate-expected" \
    HERD_GATE_EXPECTED_UNDER_LOAD="$tmp/gate-expected-load" \
    HERD_STUB_SKIPS="$tmp/gate-skips" \
    "$@" bash -c "cd '$gatedir' && ./scripts/gate.sh ${GATE_ARGS:-}" 2>&1
}
# gate_case <name> <expected exit> <grep pattern> <env...>
gate_case() {
  name=$1; want=$2; pat=$3; shift 3
  out=$(gate_run "$@"); rc=$?
  if [ "$rc" -eq "$want" ] && printf '%s' "$out" | grep -qE -- "$pat"; then
    ok "gate: $name"
  else
    bad "gate: $name" "    want exit $want matching /$pat/, got exit $rc:
$(printf '%s\n' "$out" | tail -12 | sed 's/^/    /')"
  fi
}

gate_fresh
gate_case "a clean tree with the expected skip list is GREEN" 0 '^gate: GREEN sha=[0-9a-f]{40} '
# The verdict names the sha it is about: an authorisation that does not say
# which commit it covers is not an authorisation (ORCH-001).
out=$(gate_run); sha=$(cd "$gatedir" && git rev-parse HEAD)
printf '%s' "$out" | grep -q "gate: GREEN sha=$sha " \
  && ok "gate: the GREEN verdict names the exact sha" \
  || bad "gate: the verdict must name the sha" "$(printf '%s\n' "$out" | tail -4)"
printf '%s' "$out" | grep -qE 'skips=|steps=0-8:ok' \
  && ok "gate: the verdict is one line and says the steps all ran" \
  || bad "gate: verdict shape" "$(printf '%s\n' "$out" | tail -4)"
# One output file, and the verdict is its last line.
log=$(printf '%s' "$out" | sed -n 's/^log: //p' | tail -1)
[ -f "$log" ] && [ "$(tail -2 "$log" | head -1 | cut -c1-11)" = "gate: GREEN" ] \
  && ok "gate: one log file whose end carries the verdict" \
  || bad "gate: the log must end in the verdict" "    log=$log"
# Every ci.yml step is named in the log, mapped to its CI job.
for s in "step 1: check-docs" "step 2: check-orchestration" "step 3: nix flake check" \
         "step 4: cargo fmt" "step 5: cargo clippy" "step 6: test suite" "step 7: the skips"; do
  grep -qF "$s" "$log" || bad "gate: the log does not name '$s'" ""
done
grep -qF "step 1: check-docs.sh (ci job: docs)" "$log" \
  && grep -qF "step 5: cargo clippy (ci job: check)" "$log" \
  && ok "gate: each step names the ci.yml job it stands for" \
  || bad "gate: the steps do not map onto ci.yml's jobs" ""

# --- red paths ------------------------------------------------------------
gate_fresh
gate_case "a red step makes the gate RED and names it" 1 '^gate: RED .*5:cargo-clippy' \
  HERD_STUB_CLIPPY=1
gate_fresh
gate_case "a red docs step makes the gate RED" 1 '^gate: RED .*1:check-docs' \
  HERD_STUB_DOCS=1
gate_fresh
gate_case "a red suite makes the gate RED" 1 '^gate: RED .*6:test-suite' \
  HERD_STUB_SUITE=red
gate_fresh
echo "an uncommitted line" >> "$gatedir/scripts/test.sh"
gate_case "a dirty tree is refused before anything runs" 3 'working tree is not clean' 
gate_fresh
printf 'HERD-SKIPPED: a_needs_cgroup_delegation: no delegated subtree here\nHERD-SKIPPED: b_needs_bfq: no device offers bfq\nHERD-SKIPPED: c_nobody_expected_this: a new reason\n' > "$tmp/gate-skips"
gate_case "a skip that is not on the expected list is RED" 1 '7:unexpected-skip'
out=$(gate_run)
printf '%s' "$out" | grep -qF '+ c_nobody_expected_this' \
  && ok "gate: the unexpected skip is named, not counted" \
  || bad "gate: the unexpected skip must be named" "$(printf '%s\n' "$out" | tail -8)"
# The same run, with the load-conditional list allowing that skip above the
# load threshold: the gate is GREEN and says which list it applied. This is
# how the flood benches skipping themselves above load1=8 stays legible
# without the gate ever reading a count.
printf 'HERD-SKIPPED: c_nobody_expected_this: a new reason\n' > "$tmp/gate-expected-load"
gate_case "the load-conditional list allows a load skip above the threshold" 0 \
  'load-conditional list' HERD_GATE_LOAD_NOTE=0
gate_case "and below the threshold the same skip is still RED" 1 '7:unexpected-skip' \
  HERD_GATE_LOAD_NOTE=1000
# The count moving on its own is not a signal and must not be one.
gate_fresh
printf 'HERD-SKIPPED: a_needs_cgroup_delegation: no delegated subtree here\n' > "$tmp/gate-skips"
gate_case "an expected skip that did not appear is a note, not a failure" 0 \
  'an expected skip did not appear'
out=$(gate_run); printf '%s' "$out" | grep -qE 'gate: GREEN' \
  && ok "gate: a smaller skip list does not turn the gate red (list, not count)" \
  || bad "gate: a smaller list must stay green with a note" "$(printf '%s\n' "$out" | tail -6)"

# D27 and D179: a run that did not finish is not a run.
gate_fresh
gate_case "a suite that never printed PASSED cannot be reported as one" 1 \
  '6:test-suite-incomplete' HERD_STUB_NOPASS=1
gate_fresh
GATE_ARGS=--bless
gate_case "--bless on a suite that did not pass is refused" 1 \
  'bless refused' HERD_STUB_SUITE=red
gate_fresh
rm -f "$tmp/gate-expected"
gate_case "--bless after a passing suite adopts the list and is GREEN" 0 \
  'wrote 2 lines'
grep -q '^HERD-SKIPPED: a_needs_cgroup_delegation' "$tmp/gate-expected" \
  && grep -q '^# host=' "$tmp/gate-expected" \
  && ok "gate: --bless records the host and the load it measured on" \
  || bad "gate: the blessed list must say where it came from" ""
GATE_ARGS=""
gate_fresh
rm -f "$tmp/gate-expected"
gate_case "no expected list for this host: refused, not assumed" 3 \
  'no expected skip list'
gate_fresh
gate_case "a load above the ceiling is refused with a retry" 3 \
  'above the ceiling' HERD_GATE_LOAD_MAX=-1

# --- the receipt: a gate run on another sha proves nothing ----------------
gate_fresh
out=$(gate_run); sha=$(cd "$gatedir" && git rev-parse HEAD)
[ -f "$tmp/gate-receipts/$sha.receipt" ] \
  && ok "gate: a GREEN run writes a receipt named for its sha" \
  || bad "gate: no receipt after a green run" "$(ls "$tmp/gate-receipts")"
verify() { # <expected exit> <sha> <label>
  env -i PATH="$gatedir/bin:/usr/bin:/bin:$PATH" HOME="$tmp" \
    HERD_GATE_RECEIPTS="$tmp/gate-receipts" \
    bash -c "cd '$gatedir' && ./scripts/gate.sh --verify-receipt '$2'" >/dev/null 2>&1
  rc=$?
  [ "$rc" -eq "$1" ] && ok "gate: $3" || bad "gate: $3 (exit $rc, wanted $1)" ""
}
verify 0 "$sha" "the receipt for the sha that was gated verifies"
other=0000000000000000000000000000000000000000
verify 1 "$other" "a receipt for another sha does not satisfy the gate"
# A receipt file that names a different sha than its own name is refused too:
# renaming a green receipt must not launder another commit.
cp "$tmp/gate-receipts/$sha.receipt" "$tmp/gate-receipts/$other.receipt"
verify 1 "$other" "a green receipt renamed onto another sha is refused"
rm -f "$tmp/gate-receipts/$other.receipt"
# A red run leaves no receipt at all.
gate_fresh
gate_run HERD_STUB_CLIPPY=1 >/dev/null
sha=$(cd "$gatedir" && git rev-parse HEAD)
[ ! -f "$tmp/gate-receipts/$sha.receipt" ] \
  && ok "gate: a RED run writes no receipt" \
  || bad "gate: a red run must not leave a receipt" ""
rm -rf "$gatedir" "$tmp/gate-receipts" "$tmp/gate-skips" "$tmp/gate-expected" "$tmp/gate-expected-load"

echo "== orchestrator-guard.sh: a global option must not move the verb out of reach"
# `git -C <dir> merge` and `git -C <dir> push` passed the guard until
# 2026-10-28, because every git rule in it matched the verb in first
# position. The idiom is about to be documented in workflow.md and
# herd-merger.md for driving another worktree, so it is the bypass certain
# to be used by accident. The two known bypasses asserted above
# (`git -c a=b commit`, `/usr/bin/git commit`) are deliberate and stay
# exactly as they are: closing one of those is a decision, not a side
# effect of this fix.
OG="$root/.claude/hooks/orchestrator-guard.sh"
og() { # <expected exit> <command, JSON-escaped>
  printf '{"tool_name":"Bash","cwd":"%s","tool_input":{"command":"%s"}}' \
    "$root" "$2" | "$OG" >/dev/null 2>&1; rc=$?
  [ "$rc" -eq "$1" ] && ok "guard exit $1: $2" \
    || bad "guard expected $1 got $rc" "    $2"
}
og 2 'git -C /x merge origin/main'
og 2 'git -C /x push origin main'
og 2 'git -C /x commit -m hi'
og 2 'git --git-dir=/x/.git --work-tree=/x commit -m hi'
og 2 'git --git-dir /x/.git push origin main'
og 2 'git --work-tree=/x merge origin/main'
og 2 'git -C \"/a b\" push origin main'
og 2 'cd /tmp && git -C /x push origin main'
# Measured, and honest about it: `-c key=val` anywhere in the option list
# still bypasses, because `-c` is the deliberate bypass and stripping it
# here would close it by the back door. Nobody types this by accident.
og 0 'git -C /x -c a=b rebase origin/main'
# The orchestrator reads constantly, and a guard that blocks reading is a
# guard that gets switched off. Inspection through -C stays free.
og 0 'git -C /x fetch origin'
og 0 'git -C /x status --short'
og 0 'git -C /x log --oneline -5'
og 0 'git -C /x diff origin/main --stat'
og 0 'git --no-pager log -1'
og 0 'git --git-dir=/x/.git log --oneline -3'
# Unchanged on purpose: the two documented bypasses keep their verdict.
og 0 'git -c a=b commit -m hi'
og 0 '/usr/bin/git commit -m hi'
# The relative-redirection rule resolves its target now, instead of denying
# every relative target while the shell stands in the checkout. A write that
# lands in the checkout is still denied; one that lands outside is the
# orchestrator writing its own notes and was denied twice in one session.
og 2 'echo x > a.md'
og 2 'cd docs && echo x > a.md'
og 2 'echo x > ./docs/status.md'
og 0 'echo x > ../../notes.md'
og 0 'echo x > ~/notes.md'
og 0 'echo x > /tmp/claude-1002/x/notes.md'
# ... and it still fails CLOSED where the target cannot be judged.
og 2 'echo x > ~someone/notes.md'
og 2 'tee -a report.md'

echo "== pre-push: the last machine between an agent and main"
# With pull requests and required status checks gone (2026-10-28), nothing
# server-side refuses a bad push to `main`: `enforce_admins` is false and
# the push credential is a repository admin, so even allow_force_pushes:
# false does not hold against it. This hook is the only mechanism left.
# A guard only ever seen passing is not known to work, so every case below
# drives the REAL hook and the REAL scripts/gate.sh in a scratch repository
# with a local bare remote. Only the receipts are stubs: the receipt rule
# lives in gate.sh and the hook must be seen asking it, never
# re-implementing it.
pdir="$tmp/push"; prem="$tmp/push-remote.git"; prec="$tmp/push-receipts"
ptrace="$tmp/push-trace"
pp_record() { # <file>: the landing record, filled, seven sections
  cat > "$1" <<'EOR'
Land batch 9: add the push guard and its cases

- **Batch:** 9, landed 2026-10-28 by herd-merger
- **Gate:** green — ./scripts/gate.sh on the sha pushed; receipt kept
- **Skips:** 12, each named under Evidence and printed as a skip (D27)
- **CI run:** 40600000001 — recorded, not waited on

## What changed and why

Milestone M4. main refuses a push that no gate blessed.

## Branches in this batch

method-push-guard, ORCH-125, docs/orchestration/history/x.md

## Evidence

gate: GREEN steps=0-8:ok, on the sha pushed.

## Left out

The branch protection change, which is a separate act.

## Decisions allocated

D185, the push guard is the last machine before main.

## Board rows

ORCH-125 done.

## CI run

40600000001, recorded, not waited on.
EOR
}
pp_fresh() {
  rm -rf "${pdir:?}" "${prem:?}" "${prec:?}" "${ptrace:?}"
  mkdir -p "$pdir/scripts/githooks" "$prec"
  cp "$root/scripts/gate.sh" "$pdir/scripts/gate.sh"
  cp "$root/scripts/githooks/pre-push" "$pdir/scripts/githooks/pre-push"
  chmod +x "$pdir/scripts/gate.sh" "$pdir/scripts/githooks/pre-push"
  git init -q --bare "$prem" >/dev/null 2>&1
  (cd "$pdir" && git init -q . \
    && git config core.hooksPath scripts/githooks \
    && git config user.name t && git config user.email t@t \
    && git remote add origin "$prem" \
    && echo base > file.txt && git add -A \
    && git commit -qm "base" && git branch -M main) >/dev/null 2>&1
}
# A gate.sh that records what it was asked before delegating to the real
# one: two properties are then measured rather than believed -- which sha
# the hook asks about, and that it does not consult the gate at all for a
# ref that is not main.
pp_trace_gate() {
  mv "$pdir/scripts/gate.sh" "$pdir/scripts/gate-real.sh"
  { echo '#!/usr/bin/env bash'
    printf 'printf "%%s\\n" "$*" >> %s\n' "$ptrace"
    printf 'exec %s/scripts/gate-real.sh "$@"\n' "$pdir"
  } > "$pdir/scripts/gate.sh"
  chmod +x "$pdir/scripts/gate.sh"
}
pp_receipt() { # <sha the file is named for> [verdict] [sha written inside]
  printf 'verdict: %s\nsha: %s\nsteps: 0..8 all ok\n' \
    "${2:-GREEN}" "${3:-$1}" > "$prec/$1.receipt"
}
pp_commit() { # <message file>: a new commit straight on main, prints its sha
  (cd "$pdir" && date -u +%s%N > stamp.txt && git add -A \
    && git commit -q -F "$1" && git rev-parse HEAD) 2>/dev/null
}
# A real landing, built the way herd-merger builds one: an integration
# branch carries the work and is gated AS ITSELF, then it is merged onto
# main with --no-ff and the landing record as the merge message. Prints
# "<tip> <gated>".
#
# This is the shape the first version of this hook got wrong, and the
# reason the mistake was invisible: the gate runs on the integration tip,
# so the merge commit's sha does not exist until afterwards and can never
# carry a receipt. A hook that asks about the tip refuses every correct
# landing. Hence pp_landing, and hence the cases below that push a receipt
# for the tip and for the first parent and expect red.
#
# With a second argument, main first gains a commit of its own, so the
# merge's tree is NOT the gated tree -- the only thing that keeps the
# receipt honest.
pp_landing() { # <record file> [content of an unmerged commit on main]
  (cd "$pdir" \
    && git checkout -q -B integration main \
    && date -u +%s%N > work.txt && git add -A \
    && git commit -q -m "do the work" \
    && git checkout -q main \
    && { [ -z "${2:-}" ] || { printf '%s\n' "$2" > unmerged.txt \
         && git add -A && git commit -q -m "a commit the gate never saw"; }; } \
    && git merge -q --no-ff -F "$1" integration \
    && printf '%s %s\n' "$(git rev-parse HEAD)" "$(git rev-parse HEAD^2)") \
    2>/dev/null
}
pp_push() { # <git push args...>: output in $out, exit in $pp_rc
  out=$(cd "$pdir" && HERD_GATE_RECEIPTS="$prec" git push "$@" 2>&1); pp_rc=$?
}
pp_case() { # <name> <expected exit> <grep pattern> <git push args...>
  name=$1; want=$2; pat=$3; shift 3
  pp_push "$@"
  if [ "$pp_rc" -eq "$want" ] && printf '%s' "$out" | grep -qE -- "$pat"; then
    ok "pre-push: $name"
  else
    bad "pre-push: $name" "    want exit $want matching /$pat/, got exit $pp_rc:
$(printf '%s\n' "$out" | sed 's/^/    /' | head -10)"
  fi
}

pp_record "$tmp/push-good.md"
MAIN="HEAD:refs/heads/main"

# --- a landing is a --no-ff merge, and the gated object is its second parent
pp_fresh
sha=$(pp_commit "$tmp/push-good.md"); pp_receipt "$sha"
pp_case "a tip on main with no second parent is refused" 1 \
  'has no second parent' origin "$MAIN"

pp_fresh
land=$(pp_landing "$tmp/push-good.md"); tip=${land%% *}; gsha=${land##* }
pp_case "a landing with no receipt at all is refused" 1 \
  "no green gate receipt for the gated commit \($gsha\)" origin "$MAIN"
# A receipt for the merge commit itself: this is exactly what the first
# version of this hook asked for, and it must stay red.
pp_receipt "$tip"
pp_case "a receipt for the merge commit itself does not authorise it" 1 \
  'no green gate receipt for the gated commit' origin "$MAIN"
rm -f "${prec:?}/$tip.receipt"
first=$(cd "$pdir" && git rev-parse "$tip^1")
pp_receipt "$first"
pp_case "a receipt for the first parent does not authorise the landing" 1 \
  'no green gate receipt for the gated commit' origin "$MAIN"
rm -f "${prec:?}/$first.receipt"
pp_receipt "$gsha" RED
pp_case "a RED receipt for the gated commit is refused" 1 \
  'is not GREEN' origin "$MAIN"
# A green receipt renamed onto the gated sha: the file's own `sha:` field
# disagrees with its name, and gate.sh is what notices.
pp_receipt "$gsha" GREEN 0000000000000000000000000000000000000000
pp_case "a GREEN receipt renamed onto the gated sha is refused" 1 \
  'does not name' origin "$MAIN"

# The tree is what keeps the receipt honest: a merger must not be able to
# gate one tree and push another.
pp_fresh
land=$(pp_landing "$tmp/push-good.md" "a line no gate ever saw")
tip=${land%% *}; gsha=${land##* }
pp_receipt "$gsha"
pp_case "a merge that changed the tree is refused though the receipt verifies" 1 \
  'the merge changed the tree' origin "$MAIN"

# --- the landing record: the gap nothing else can close -------------------
# check-orchestration.sh copies the corpus without .git, so no docs rule can
# ever read a commit message. A merger who fills the handoff and merges with
# a one-line message is stopped here or nowhere. Each case below is a
# correct landing in every other respect: gated second parent, identical
# tree, only the record is wrong.
printf 'Land batch 9: a one-line merge commit\n' > "$tmp/push-oneline.md"
grep -v '^## Left out$' "$tmp/push-good.md" > "$tmp/push-nosection.md"
sed 's/^- \*\*CI run:\*\*.*/- **CI run:** <id> — recorded, not waited on/' \
  "$tmp/push-good.md" > "$tmp/push-placeholder.md"
sed '/^- \*\*Skips:\*\*/d' "$tmp/push-good.md" > "$tmp/push-nofield.md"
sed '1s/.*/Land batch 9: a subject deliberately longer than the seventy-two columns the contract allows/' \
  "$tmp/push-good.md" > "$tmp/push-longsubject.md"
pp_record_case() { # <name> <grep pattern> <record file>
  pp_fresh
  land=$(pp_landing "$3"); gsha=${land##* }; pp_receipt "$gsha"
  pp_case "$1" 1 "$2" origin "$MAIN"
}
pp_record_case "a one-line merge message is refused on a correct landing" \
  'has a one-line message' "$tmp/push-oneline.md"
pp_record_case "a merge message missing one required section is refused" \
  'missing section: ## Left out' "$tmp/push-nosection.md"
pp_record_case "a header field still carrying its placeholder is refused" \
  'CI run:\*\* still carries a placeholder: <id>' "$tmp/push-placeholder.md"
pp_record_case "a missing header field is refused and named" \
  'missing or empty header field: - \*\*Skips:\*\*' "$tmp/push-nofield.md"
pp_record_case "a subject over 72 characters is refused" \
  'characters, over 72' "$tmp/push-longsubject.md"

# --- the green path, and it is the one that proves the rest mean something -
pp_fresh
pp_trace_gate
land=$(pp_landing "$tmp/push-good.md"); tip=${land%% *}; gsha=${land##* }
pp_receipt "$gsha"
pp_case "a --no-ff landing, gated second parent, identical tree, filled record" 0 \
  "pre-push: main <- .* \(merge of ${gsha:0:12}\)" origin "$MAIN"
[ "$(git --git-dir="$prem" rev-parse refs/heads/main 2>/dev/null)" = "$tip" ] \
  && ok "pre-push: the landing reached the remote" \
  || bad "pre-push: the landing did not reach the remote" ""
grep -qF -- "--verify-receipt $gsha" "$ptrace" \
  && ! grep -qF -- "--verify-receipt $tip" "$ptrace" \
  && ok "pre-push: the gate is asked about the second parent, not the tip" \
  || bad "pre-push: wrong sha asked of the gate" "    $(cat "$ptrace" 2>/dev/null)"

# The sha on stdin, not HEAD at some other moment: two landings exist, HEAD
# is the second, and the FIRST is what gets pushed. The gate must be asked
# about the first landing's gated commit and not the second's.
pp_fresh
pp_trace_gate
l1=$(pp_landing "$tmp/push-good.md"); tip1=${l1%% *}; g1=${l1##* }
l2=$(pp_landing "$tmp/push-good.md"); tip2=${l2%% *}; g2=${l2##* }
pp_receipt "$g1"; pp_receipt "$g2"
pp_push origin "$tip1:refs/heads/main"
[ "$pp_rc" -eq 0 ] && grep -qF -- "--verify-receipt $g1" "$ptrace" \
  && ! grep -qF -- "--verify-receipt $g2" "$ptrace" \
  && ok "pre-push: the sha on stdin decides, not HEAD" \
  || bad "pre-push: HEAD must not decide" "    exit $pp_rc, tip2=$tip2
    $(cat "$ptrace" 2>/dev/null)"

# --- every other ref goes through untouched, and stays cheap --------------
pp_fresh
pp_trace_gate
sha=$(pp_commit "$tmp/push-oneline.md")
pp_push -q origin "HEAD:refs/heads/method-task-x"
[ "$pp_rc" -eq 0 ] && [ -z "$out" ] \
  && ok "pre-push: a task branch with no receipt and a one-line message is silent" \
  || bad "pre-push: a task branch must pass silently" "    exit $pp_rc: $out"
[ ! -s "$ptrace" ] \
  && ok "pre-push: a ref that is not main never consults the gate" \
  || bad "pre-push: the gate was consulted for a task branch" "    $(cat "$ptrace")"
pp_push -q --force origin "HEAD:refs/heads/method-task-x"
[ "$pp_rc" -eq 0 ] \
  && ok "pre-push: force-pushing a task branch is not this hook's business" \
  || bad "pre-push: a task branch force-push must pass" "    exit $pp_rc: $out"

# --- a rewrite and a deletion of main, whatever else holds ----------------
# These are the belt, not the braces. GitHub refuses neither: `main` carries
# allow_force_pushes: false and allow_deletions: false, but also
# enforce_admins: false, and the credential this project pushes with is a
# repository admin, so branch protection does not bind it (measured
# 2026-10-28).
pp_fresh
land=$(pp_landing "$tmp/push-good.md"); gsha=${land##* }; pp_receipt "$gsha"
pp_push origin "$MAIN"
[ "$pp_rc" -eq 0 ] || bad "pre-push: the setup landing failed" "$out"
# A divergent landing, fully gated, identical tree, filled record: nothing
# but the fast-forward rule can refuse it, which is the point.
(cd "$pdir" && git reset -q --hard HEAD~1) >/dev/null 2>&1
land2=$(pp_landing "$tmp/push-good.md"); gsha2=${land2##* }; pp_receipt "$gsha2"
pp_case "a force-push that rewrites main is refused, gate and record notwithstanding" 1 \
  'not a fast-forward' --force origin "$MAIN"
# Without --force the same rewrite never reaches the hook: git declines it
# during the push negotiation. Measured, and recorded as such -- the hook's
# own refusal above is the one that matters, because it is the one --force
# would otherwise walk past.
pp_case "the same rewrite without --force is refused, by git before the hook" 1 \
  '(non-fast-forward|not a fast-forward)' origin "$MAIN"
pp_case "deleting main is refused" 1 \
  'this push deletes main' origin ":refs/heads/main"
pp_case "deleting main by --delete is refused too" 1 \
  'this push deletes main' --delete origin main
[ -n "$(git --git-dir="$prem" rev-parse --verify -q refs/heads/main)" ] \
  && ok "pre-push: main still exists on the remote after both refusals" \
  || bad "pre-push: main was deleted on the remote" ""

# A remote tip this repository has never seen: the hook cannot tell a
# fast-forward from a rewrite, so it refuses and says to fetch rather than
# guessing. A second clone that never fetched is how that happens.
pdir2="$tmp/push-other"
rm -rf "${pdir2:?}"
mkdir -p "$pdir2/scripts/githooks"
cp "$root/scripts/gate.sh" "$pdir2/scripts/gate.sh"
cp "$root/scripts/githooks/pre-push" "$pdir2/scripts/githooks/pre-push"
chmod +x "$pdir2/scripts/gate.sh" "$pdir2/scripts/githooks/pre-push"
(cd "$pdir2" && git init -q . && git config core.hooksPath scripts/githooks \
  && git config user.name t && git config user.email t@t \
  && git remote add origin "$prem" && echo other > file.txt && git add -A \
  && git commit -q -F "$tmp/push-good.md") >/dev/null 2>&1
osha=$(cd "$pdir2" && git rev-parse HEAD); pp_receipt "$osha"
out=$(cd "$pdir2" && HERD_GATE_RECEIPTS="$prec" \
  git push --force origin "HEAD:refs/heads/main" 2>&1); pp_rc=$?
if [ "$pp_rc" -eq 1 ] \
   && printf '%s' "$out" | grep -q 'cannot tell a fast-forward'; then
  ok "pre-push: a remote tip this clone has never seen is refused, not guessed"
else
  bad "pre-push: an unknown remote tip must be refused" "    exit $pp_rc:
$(printf '%s\n' "$out" | sed 's/^/    /' | head -8)"
fi
rm -rf "${pdir2:?}"

# --- the guard nobody installs is decoration -----------------------------
# core.hooksPath is what makes this file run at all, and the hook cannot
# report its own absence when it is not installed -- nothing runs. What it
# can do is answer for the worktree it is asked in, which is how a worktree
# without the setting is detectable.
pp_fresh
(cd "$pdir" && ./scripts/githooks/pre-push --check-install >/dev/null 2>&1) \
  && ok "pre-push: --check-install passes where core.hooksPath points at it" \
  || bad "pre-push: --check-install must pass in an installed worktree" ""
(cd "$pdir" && git config --unset core.hooksPath)
(cd "$pdir" && ./scripts/githooks/pre-push --check-install >/dev/null 2>&1) \
  && bad "pre-push: --check-install must fail where the hook is not installed" "" \
  || ok "pre-push: --check-install reports a worktree with no core.hooksPath"
# And with the setting gone, the hook really is absent: the same push that
# was refused above now goes through. This is the limit, measured.
pp_push -q origin "HEAD:refs/heads/main"
[ "$pp_rc" -eq 0 ] \
  && ok "pre-push: with core.hooksPath unset the guard does not run (known limit)" \
  || bad "pre-push: expected the unguarded push to succeed" "    exit $pp_rc: $out"
# --no-verify is the other limit, and the contract is what forbids it.
pp_fresh
sha=$(pp_commit "$tmp/push-oneline.md")
pp_push -q --no-verify origin "HEAD:refs/heads/main"
[ "$pp_rc" -eq 0 ] \
  && ok "pre-push: --no-verify bypasses the hook entirely (known limit)" \
  || bad "pre-push: --no-verify is expected to bypass the hook" "    exit $pp_rc: $out"
rm -rf "${pdir:?}" "${prem:?}" "${prec:?}" "${ptrace:?}"
rm -f "${tmp:?}"/push-good.md "${tmp:?}"/push-oneline.md \
  "${tmp:?}"/push-nosection.md "${tmp:?}"/push-placeholder.md \
  "${tmp:?}"/push-nofield.md "${tmp:?}"/push-longsubject.md
echo
echo "check-orchestration: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
