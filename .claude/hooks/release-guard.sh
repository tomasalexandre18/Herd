#!/usr/bin/env bash
# PreToolUse hook on Bash for herd-release: prepare, never publish.
# Blocks tags, tag pushes, release publication, workflow dispatch and
# anything that operates a machine. Best effort regex; fails open on
# unparseable input. Disable with .claude/hooks/DISABLED.
set -u
here=$(cd "$(dirname "$0")" && pwd)
[ -e "$here/DISABLED" ] && exit 0
command -v perl >/dev/null 2>&1 || exit 0
c=$(cat | perl -MJSON::PP -0777 -ne '
  my $d = eval { JSON::PP->new->decode($_) } or exit 0;
  my $v = ($d->{tool_input} || {})->{command}; $v = "" unless defined $v;
  $v =~ s/\n/ /g; print $v;' 2>/dev/null) || exit 0
[ -z "$c" ] && exit 0
deny() { printf 'release-guard: %s\nherd-release prepares a release and never publishes it (CLAUDE.md, Git and main).\n' "$1" >&2; exit 2; }
printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)git\s+tag(\s|$)' && deny "git tag in: $c"
printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)git\s+push\s.*(--tags|refs/tags|\sv[0-9])' && deny "tag push in: $c"
printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)gh\s+(release|workflow\s+run|pr\s+merge)(\s|$)' && deny "publication through gh in: $c"
printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)gh\s+api\s.*(dispatches|/releases)' && deny "publication through gh api in: $c"
printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)(docker|podman|nixos-rebuild|systemctl|ssh|scp|rsync|sudo)(\s|$)' && deny "operating a machine in: $c (D161)"
exit 0
