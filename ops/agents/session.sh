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
  echo "  HERD_CLAUDE_BIN at one, e.g. the nix-built copy on this host," >&2
  echo "  /mnt/ssd_980/chatons/ops/agents/tools/bin/claude" >&2
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

cd "$HERD_REPO"
exec "$claude_bin" "$@"
