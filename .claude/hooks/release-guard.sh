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
# `git tag` creates a tag unless its first argument lists or inspects
# (-l, --list, -n, --contains, --points-at, --merged, --no-merged, or no
# argument at all). Each occurrence in a compound command is judged alone,
# so `git tag -l; git tag -a v1` is still refused. V10 found that refusing
# every `git tag` blocked a read-only listing.
if printf '%s' "$c" | perl -ne '
  while (/(?:^|[;&|(]|\s)git\s+tag(?:\s+([^;&|)]*))?/g) {
    my $a = defined $1 ? $1 : ""; $a =~ s/^\s+|\s+$//g;
    exit 0 unless $a eq "" || $a =~ /^(-l|--list|-n\d*|--contains|--points-at|--merged|--no-merged)(\s|$)/;
  } exit 1'; then deny "git tag in: $c"; fi
printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)git\s+push\s.*(--tags|refs/tags|\sv[0-9])' && deny "tag push in: $c"
printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)gh\s+(release|workflow\s+run|pr\s+merge)(\s|$)' && deny "publication through gh in: $c"
printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)gh\s+api\s.*(dispatches|/releases)' && deny "publication through gh api in: $c"
printf '%s' "$c" | grep -qE '(^|[;&|(]|\s)(docker|podman|nixos-rebuild|systemctl|ssh|scp|rsync|sudo)(\s|$)' && deny "operating a machine in: $c (D161)"
exit 0
