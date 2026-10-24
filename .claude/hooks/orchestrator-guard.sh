#!/usr/bin/env bash
# PreToolUse hook: the orchestrator (the main session, which has no
# agent_id) may not write in the repository nor commit, merge, tag or
# release from its shell. Execution agents (agent_id present) pass.
#
# Best effort, documented as such in docs/orchestration/permissions.md:
# a shell rule is a regex, so `git -c x commit` or `/usr/bin/git commit`
# slip past it. The contract and the review of traces remain the real
# protection; this hook catches the ordinary slip, not the determined one.
#
# Fails OPEN: unparseable input or a missing perl lets the call through,
# so the hook can never immobilise a session. Disable without editing
# settings by creating .claude/hooks/DISABLED next to this file.
set -u
here=$(cd "$(dirname "$0")" && pwd)
[ -e "$here/DISABLED" ] && exit 0
command -v perl >/dev/null 2>&1 || exit 0

input=$(cat)
# One record: agent_id, tool_name, cwd, file_path, command, separated by
# the unit separator (0x1f): a tab is IFS whitespace and `read` would
# collapse an empty field into its neighbour.
fields=$(printf '%s' "$input" | perl -MJSON::PP -0777 -ne '
  my $d = eval { JSON::PP->new->decode($_) } or exit 0;
  my $t = $d->{tool_input} || {};
  for my $v ($d->{agent_id}, $d->{tool_name}, $d->{cwd}, $t->{file_path}, $t->{command}) {
    $v = "" unless defined $v; $v =~ s/[\x1f\n]/ /g; print $v, "\x1f";
  }' 2>/dev/null) || exit 0
IFS=$'\x1f' read -r agent_id tool cwd file_path command <<<"$fields"

if [ -e "$here/.log-to" ]; then
  log=$(cat "$here/.log-to")
  printf '%s\tagent=%s\ttool=%s\tpath=%s\tcmd=%.80s\n' "$(date -u +%FT%TZ)" \
    "${agent_id:-main}" "$tool" "$file_path" "$command" >> "$log" 2>/dev/null
fi

[ -n "$agent_id" ] && exit 0          # an execution agent: not our concern
[ -z "$tool" ] && exit 0

root=$(cd "$here/../.." && pwd)        # the repository this hook belongs to
deny() { printf 'orchestrator-guard: %s\nThe orchestrator does not write in the repository; brief herd-step or herd-merger (CLAUDE.md, Roles).\n' "$1" >&2; exit 2; }

case "$tool" in
  Edit|Write|NotebookEdit|MultiEdit)
    [ -z "$file_path" ] && exit 0
    case "$file_path" in /*) abs=$file_path ;; *) abs="${cwd:-$root}/$file_path" ;; esac
    case "$abs" in
      "$root"/.claude/worktrees/*) exit 0 ;;     # an agent worktree, not the checkout
      "$root"/*|"$root") deny "$tool on $file_path is inside the repository" ;;
    esac
    exit 0 ;;
  Bash)
    [ -z "$command" ] && exit 0
    c=$command
    if printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)git\s+(commit|merge|rebase|cherry-pick|revert|am|apply|tag)(\s|$)'; then
      deny "git write verb in: $c"; fi
    if printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)git\s+push(\s|$)'; then
      deny "git push in: $c"; fi
    if printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)gh\s+(pr\s+(merge|create|edit|close)|release|workflow\s+run)(\s|$)'; then
      deny "gh write verb in: $c"; fi
    if printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)(sed|perl)\s+(-[a-zA-Z]*i|-[a-zA-Z]*\s+-i)'; then
      deny "in-place edit (sed -i / perl -i) in: $c"; fi
    if printf '%s' "$c" | grep -qE '(>>?|tee(\s+-a)?)\s*"?(\./)?(CLAUDE\.md|README\.md|docs/|scripts/|herd-[a-z]+/|herdctl/|deploy/|nix/|flake\.|Cargo\.|\.github/|\.claude/(agents|hooks|settings))'; then
      deny "redirection into a repository path in: $c"; fi
    case "$c" in *"$root"/*) 
      if printf '%s' "$c" | grep -qE "(>>?|tee(\s+-a)?|cp|mv|rm|touch|mkdir|install)\s+[^;&|]*$root/" && ! printf '%s' "$c" | grep -q "$root/.claude/worktrees/"; then
        deny "write into the orchestrator's checkout in: $c"; fi ;;
    esac
    exit 0 ;;
esac
exit 0
