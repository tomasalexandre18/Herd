#!/usr/bin/env bash
# PreToolUse hook: the orchestrator (the main session, which has neither
# agent_id nor agent_type) may not write in the repository nor commit, merge, tag or
# release from its shell. Execution agents pass: one spawned by the Agent
# tool carries agent_id, one started as `claude --agent herd-step` carries
# agent_type (both observed on 2.1.303, see docs/orchestration/permissions.md).
#
# Best effort, documented as such in docs/orchestration/permissions.md:
# a shell rule is a regex, and it catches the ordinary slip, not the
# determined one. The contract and the review of traces remain the real
# protection.
#
# Recognised since 2026-10-28: git invoked with global options in front of
# the subcommand. `git -C <dir> merge`, `git --git-dir=<d> --work-tree=<d>
# commit` and the same shape with `--exec-path`, `--namespace`, `--bare`,
# `--no-pager` or `-P` all used to slip past every git rule here, because
# each rule matched the verb in first position only. `git -C <dir>` is about
# to be the documented idiom for driving another worktree, so it was the one
# bypass certain to be used by accident rather than on purpose.
#
# Still slips past, by decision or for want of a better rule:
# `git -c key=val commit` -- a deliberate, asserted bypass (the "known
# bypasses" section of scripts/check-orchestration.sh and
# docs/orchestration/permissions.md); closing it is a decision of its own,
# not a side effect of this one -- `/usr/bin/git commit`, `sh -c`,
# `python -c`, a verb built up in a shell variable, and an alias.
#
# This guard constrains the MAIN SESSION ONLY, and that is the intent, not
# an oversight: an execution agent carries agent_id or agent_type and is let
# through below before any rule runs, because committing, merging and
# pushing are its job. Nothing here protects the repository against an
# agent. What refuses an agent's bad push to `main` is
# scripts/githooks/pre-push, and only where core.hooksPath points at it.
#
# Fails OPEN: unparseable input or a missing perl lets the call through,
# so the hook can never immobilise a session. Disable without editing
# settings by creating .claude/hooks/DISABLED next to this file.
set -u
here=$(cd "$(dirname "$0")" && pwd)
[ -e "$here/DISABLED" ] && exit 0
command -v perl >/dev/null 2>&1 || {
  printf 'orchestrator-guard: no perl, the guard is OFF for this call.\n' >&2
  exit 0; }

input=$(cat)
# One record: agent_id, agent_type, tool_name, cwd, file_path, command,
# separated by the unit separator (0x1f): a tab is IFS whitespace and `read` would
# collapse an empty field into its neighbour. An agent spawned by the Agent
# tool carries agent_id and agent_type; a session started as
# `claude --agent herd-step` carries agent_type only; the orchestrator's
# main thread carries neither (probed on 2.1.303, 2026-10-24).
fields=$(printf '%s' "$input" | perl -MJSON::PP -0777 -ne '
  my $d = eval { JSON::PP->new->decode($_) } or exit 0;
  my $t = $d->{tool_input} || {};
  for my $v ($d->{agent_id}, $d->{agent_type}, $d->{tool_name}, $d->{cwd}, $t->{file_path}, $t->{command}) {
    $v = "" unless defined $v; $v =~ s/[\x1f\n]/ /g; print $v, "\x1f";
  }' 2>/dev/null) || exit 0
IFS=$'\x1f' read -r agent_id agent_type tool cwd file_path command <<<"$fields"

if [ -e "$here/.log-to" ]; then
  log=$(cat "$here/.log-to")
  [ -e "$here/.log-raw" ] && printf '%s\n' "$input" >> "$log.raw" 2>/dev/null
  printf '%s\tagent=%s\ttool=%s\tpath=%s\tcmd=%.80s\n' "$(date -u +%FT%TZ)" \
    "${agent_id:-${agent_type:-main}}" "$tool" "$file_path" "$command" >> "$log" 2>/dev/null
fi

[ -n "$agent_id$agent_type" ] && exit 0   # an execution agent: not our concern
[ -z "$tool" ] && exit 0

root=$(cd "$here/../.." && pwd)        # the repository this hook belongs to
deny() { printf 'orchestrator-guard: %s\nThe orchestrator does not write in the repository; brief herd-step or herd-merger (CLAUDE.md, Roles).\n' "$1" >&2; exit 2; }

case "$tool" in
  Edit|Write|NotebookEdit|MultiEdit)
    [ -z "$file_path" ] && exit 0
    case "$file_path" in /*) abs=$file_path ;; *) abs="${cwd:-$root}/$file_path" ;; esac
    # Resolve "." and ".." textually, so that
    # .claude/worktrees/../../docs/x.md cannot pose as an agent worktree,
    # and /tmp/..<root>/docs/x.md cannot hide the prefix (V7, finding 1).
    abs=$(perl -e 'my @o; for (split m{/}, shift) { next if $_ eq "" || $_ eq ".";
      if ($_ eq "..") { pop @o } else { push @o, $_ } } print "/", join "/", @o' \
      "$abs" 2>/dev/null) || exit 0
    case "$abs" in
      "$root"/.claude/worktrees/*) exit 0 ;;     # an agent worktree, not the checkout
      "$root"/*|"$root") deny "$tool on $file_path is inside the repository" ;;
    esac
    exit 0 ;;
  Bash)
    [ -z "$command" ] && exit 0
    c=$command
    # `git -C <dir> merge` has to be read as `git merge`: every git rule
    # below matches the verb right after `git`, so one global option in
    # front of it defeated all of them at once. Strip the global options
    # that move the verb out of first position, then match on the result.
    # `-c key=val` is deliberately NOT in this list (see the header). A
    # quoted argument is handled, so `git -C "/a b" push` is read too.
    cn=$(printf '%s' "$c" | perl -0777 -pe '
      s{(^|[;&|(]|\s)git((?:\s+(?:
            -C\s+(?:"[^"]*"|\x27[^\x27]*\x27|\S+)
          | --git-dir(?:=\S+|\s+(?:"[^"]*"|\x27[^\x27]*\x27|\S+))
          | --work-tree(?:=\S+|\s+(?:"[^"]*"|\x27[^\x27]*\x27|\S+))
          | --exec-path(?:=\S+|\s+(?:"[^"]*"|\x27[^\x27]*\x27|\S+))
          | --namespace(?:=\S+|\s+\S+)
          | --bare | --no-pager | --no-replace-objects
          | --literal-pathspecs | -P
        ))+)\s+}{$1git }gx' 2>/dev/null) || cn=$c
    [ -n "$cn" ] || cn=$c
    if printf '%s' "$cn" | grep -qE '(^|[;&|(]|\s)git\s+(commit|merge|rebase|cherry-pick|revert|am|apply|tag)(\s|$)'; then
      deny "git write verb in: $c"; fi
    if printf '%s' "$cn" | grep -qE '(^|[;&|(]|\s)git\s+(reset|checkout|switch|restore|stash|clean)(\s|$)'; then
      deny "git verb that rewrites a checkout in: $c"; fi
    if printf '%s' "$cn" | grep -qE '(^|[;&|(]|\s)git\s+push(\s|$)'; then
      deny "git push in: $c"; fi
    if printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)gh\s+(pr\s+(merge|create|edit|close)|release|workflow\s+run)(\s|$)'; then
      deny "gh write verb in: $c"; fi
    if printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)(sed|perl)\s+(-[a-zA-Z]*i|-[a-zA-Z]*\s+-i)'; then
      deny "in-place edit (sed -i / perl -i) in: $c"; fi
    if printf '%s' "$c" | grep -qE '(>>?|tee(\s+-a)?)\s*"?(\./)?(CLAUDE\.md|README\.md|docs/|scripts/|herd-[a-z]+/|herdctl/|deploy/|nix/|flake\.|Cargo\.|\.github/|\.claude/(agents|hooks|settings))'; then
      deny "redirection into a repository path in: $c"; fi
    # A relative redirection lands wherever the shell stands: inside the
    # checkout, that is a repository write. A target that starts with $ or
    # with a quoted variable cannot be judged here and passes (documented).
    #
    # Narrowed 2026-10-28: the coarse rule denied `echo x > ../../notes.md`
    # and `echo x > ~/notes.md` -- targets plainly OUTSIDE the checkout --
    # whenever the shell happened to stand inside it, which it usually does,
    # and a guard that refuses the orchestrator its own notes is a guard
    # somebody switches off. So each target is now resolved against cwd and
    # only one that lands in the checkout is denied. It still fails CLOSED:
    # no extractable target, a `~user` form or an unresolvable $variable all
    # keep the old verdict, deny.
    case "${cwd:-}" in
      "$root"|"$root"/*)
        if printf '%s' "$c" | grep -qE '(>>?|tee(\s+-a)?)\s*"?[^/&$[:space:]"({-]'; then
          where=$(perl -e '
            my ($cmd, $cwd, $r, $home) = @ARGV;
            my @t;
            while ($cmd =~ m{(?:>>?|\btee(?:\s+-a)?)\s*(?:"([^"]+)"|\x27([^\x27]+)\x27|([^\s"\x27&|;()<>]+))}g) {
              push @t, defined $1 ? $1 : defined $2 ? $2 : $3;
            }
            print("unknown"), exit unless @t;
            for my $p (@t) {
              if ($p =~ m{^~}) {
                print("unknown"), exit unless $p =~ m{^~(/|$)} && length $home;
                $p =~ s{^~}{$home};
              }
              $p = "$cwd/$p" unless $p =~ m{^/};
              my @o; for (split m{/}, $p) { next if $_ eq q{} || $_ eq q{.};
                $_ eq q{..} ? pop @o : push @o, $_; }
              my $abs = "/" . join "/", @o;
              next if index($abs, "$r/.claude/worktrees/") == 0;
              print("inside"), exit if $abs eq $r || index($abs, "$r/") == 0;
            }
            print "outside";' "$c" "$cwd" "$root" "${HOME:-}" 2>/dev/null)
          [ "$where" = outside ] \
            || deny "redirection to a relative path while in the checkout: $c"
        fi ;;
      *)
        # `cd <somewhere in the checkout> && <write>`: the redirection target
        # is relative, so the rule above does not see it when cwd is outside.
        # Only when cwd is outside: inside, the resolving rule above has
        # already judged the same command, and the two must not disagree.
        if printf '%s' "$c" | grep -qE "(^|[;&|(]|\s)cd\s+\"?(\./)?($root|docs|scripts|herd-[a-z]+|herdctl|deploy|nix|[.]github|[.]claude)(/|\"|\s|$)" \
           && printf '%s' "$c" | grep -qE '(>>?|tee(\s+-a)?)\s*"?[^/&$[:space:]"({-]'; then
          deny "cd into the checkout followed by a write in: $c"; fi ;;
    esac
    case "$c" in *"$root"/*) 
      if printf '%s' "$c" | grep -qE "(>>?|tee(\s+-a)?|cp|mv|rm|touch|mkdir|install)\s+[^;&|]*$root/" && ! printf '%s' "$c" | grep -q "$root/.claude/worktrees/"; then
        deny "write into the orchestrator's checkout in: $c"; fi ;;
    esac
    exit 0 ;;
esac
exit 0
