#!/usr/bin/env bash
# Start a Claude Code session on Herd with Herd's own state root:
#
#   /mnt/ssd_980/herd-project/ops/agents/session.sh [claude arguments...]
#
# Why it exists rather than "remember to source env.sh": the failure this
# whole directory is about was a session started by someone else's launcher.
# One command that cannot be got half right is the thing that prevents the
# relapse; env.sh stays separate and sourceable for a shell that only needs
# cargo or nix. See docs/orchestration/state-root.md.
set -euo pipefail

HERE=$(dirname "$(readlink -f -- "${BASH_SOURCE[0]}")")
# shellcheck source=ops/agents/env.sh
. "$HERE/env.sh"

# Herd operates nothing (invariant 9). A session that began as root could.
if [ "$(id -u)" -eq 0 ]; then
  echo 'herd session: refusing to run as root' >&2
  exit 1
fi

claude_bin=${HERD_CLAUDE_BIN:-claude}
if ! command -v "$claude_bin" >/dev/null 2>&1; then
  echo "herd session: no '$claude_bin' on PATH" >&2
  echo "  the Claude Code binary is a shared tool, not shared state: point" >&2
  echo "  HERD_CLAUDE_BIN at one. env.sh already puts this host's per-user" >&2
  echo "  profile on PATH, which is where the binary is; reaching for" >&2
  echo "  another project's copy is how the state root was lost before." >&2
  exit 1
fi

# Claude Code reads the token from the config directory, and nothing puts one
# there for us: claude-creds-sync.service keeps two existing files equal and
# returns immediately while either is missing. So the first session in a new
# config directory has to log in, and only an interactive terminal can.
if [ ! -s "$CLAUDE_CONFIG_DIR/.credentials.json" ]; then
  echo "herd session: no .credentials.json in $CLAUDE_CONFIG_DIR" >&2
  echo '  log in once with /login, or copy the file from a config directory' >&2
  echo '  already logged in: install -m600 <src> <this path>.' >&2
  echo '  claude-creds-sync.service does not create it.' >&2
  if [ ! -t 0 ]; then
    echo '  stdin is not a terminal, so /login is impossible here.' >&2
    exit 1
  fi
fi

# A session that cannot reach GitHub cannot open a pull request, cannot merge
# one, and cannot `git push`: `origin` is https and the credential helper is
# `gh auth git-credential`. env.sh points GH_CONFIG_DIR at the host user's
# login because HOME no longer does; if that directory holds no `hosts.yml`
# the session finds out at the end of a task instead of at the start, which
# is the ORCH-068 failure. Say it now. Not fatal: a session that only reads,
# builds and tests needs none of it.
if [ ! -s "$GH_CONFIG_DIR/hosts.yml" ]; then
  echo "herd session: no hosts.yml in $GH_CONFIG_DIR" >&2
  echo '  this session cannot open or merge a pull request, and a push to' >&2
  echo '  an https remote will fail. Run `gh auth login` as the host user,' >&2
  echo '  or point HERD_GH_CONFIG_DIR at a directory already logged in.' >&2
  echo '  Never copy a token into this repository (invariant 7).' >&2
fi

cd "$HERD_REPO"
exec "$claude_bin" "$@"
