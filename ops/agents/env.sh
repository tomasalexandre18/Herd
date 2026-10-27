#!/usr/bin/env bash
# Put every writable path a Herd Claude Code session touches under Herd's own
# state root, and nowhere else. Source it before `claude`, `nix` or `cargo`:
#
#   source /mnt/ssd_980/herd-project/ops/agents/env.sh
#
# Why it exists: until 2026-10-24 Herd's sessions were started by chatOnS's
# ops/agents/env.sh, so they ran inside chatOnS's configuration directory and
# inherited its user settings, including an `autoMode` block granting sudo,
# systemctl, docker and nixos-rebuild. Herd is shipped, not operated (D161,
# invariant 9), and a project that borrows another project's state also loses
# its own when that project moves. See docs/orchestration/state-root.md.
#
# This file is the one the sessions on the host are actually started by. It
# drifted once (ORCH-068): the shipped copy set no GH_CONFIG_DIR, so a
# session started from it could neither open nor merge a pull request, nor
# even `git push`. scripts/check-orchestration.sh now sources a sandboxed
# copy and asserts what a session needs; keep the two in step.
#
# It is sourced, never executed: it exports, defines and creates directories,
# and starts nothing. ops/agents/session.sh is the launcher on top of it.

# The one line to edit if the state root ever moves. Hardcoded on purpose:
# the guard below is only worth something if the root is not itself taken
# from the environment this script is meant to correct.
HERD_STATE_ROOT=/mnt/ssd_980/herd
export HERD_STATE_ROOT
export HERD_CLAUDE_ROOT=$HERD_STATE_ROOT/claude

# The checkout this file belongs to, so a worktree's copy points at that
# worktree. The checkout is versioned work, not session state: it lives
# outside the state root and the guard below does not cover it.
herd_here=$(dirname "$(readlink -f -- "${BASH_SOURCE[0]}")")
HERD_REPO=$(cd "$herd_here/../.." && pwd)
unset herd_here
export HERD_REPO

export HOME=$HERD_CLAUDE_ROOT/home
export CLAUDE_CONFIG_DIR=$HERD_CLAUDE_ROOT/config
export XDG_CONFIG_HOME=$HOME/.config XDG_CACHE_HOME=$HOME/.cache
export XDG_DATA_HOME=$HOME/.local/share XDG_STATE_HOME=$HOME/.local/state
# Not a tmpfs and not cleared at boot, unlike a login's XDG_RUNTIME_DIR.
# Nothing here may be assumed to disappear on reboot.
export XDG_RUNTIME_DIR=$HERD_CLAUDE_ROOT/run
export TMPDIR=$HERD_CLAUDE_ROOT/tmp
export TMP=$TMPDIR TEMP=$TMPDIR
# `nix develop` gives cargo; cargo still writes its registry and its git
# checkouts to CARGO_HOME, which is a gigabyte of state and belongs here.
# Under the Claude root, beside the rest of one session's state, which is
# where the registry in use on this host already is.
export CARGO_HOME=$HERD_CLAUDE_ROOT/cargo
export HISTFILE=$HOME/.bash_history

# `gh` keeps its login in GH_CONFIG_DIR, and HOME above no longer points at
# the host user's. Without this line a session can neither `gh pr create` nor
# `gh pr merge`, and cannot even `git push`: `origin` is https and the
# credential helper is `gh auth git-credential`, which reads the same
# directory. That login is the host user's, shared like the Claude Code
# binary and deliberately NOT Herd session state, so it sits outside the
# state root and the guard below does not cover it. A path, never a token:
# nothing of it is in this repository (invariant 7).
herd_login_home=$(getent passwd "$(id -un)" 2>/dev/null | cut -d: -f6)
case $herd_login_home in /*) ;; *) herd_login_home=/home/$(id -un) ;; esac
export GH_CONFIG_DIR=${HERD_GH_CONFIG_DIR:-$herd_login_home/.config/gh}
unset herd_login_home

# These four directories go in front of whatever was inherited, in this
# order, because a session started from a thin environment otherwise finds
# no `claude` at all and a session started from chatOnS's shell finds
# chatOnS's copies first:
#   - $HERD_CLAUDE_ROOT/tools/dtach/bin: the `dtach` a long-lived session is
#     detached and re-attached with. Herd's own, under Herd's own root.
#   - /run/wrappers/bin before /run/current-system/sw/bin, as on a NixOS
#     login: the setuid wrappers (fusermount3, newuidmap, mount, su) live
#     there and the store copies behind sw/bin are not setuid. This is for
#     the wrappers a build or a container test may need; it is NOT a
#     permission decision. What keeps a Herd session away from the
#     consumer's machines is invariant 9 plus the fact that this config
#     directory carries no `autoMode` allowances, not a PATH.
#   - /etc/profiles/per-user/<user>/bin: where `claude` is on this host.
# What is inherited stays behind them rather than being dropped, so that
# this file is still sourceable on a host that is not this one — the guard
# below needs `realpath` and `mkdir`, and neither is in the four.
herd_path_head=$HERD_CLAUDE_ROOT/tools/dtach/bin:/run/wrappers/bin
herd_path_head=$herd_path_head:/etc/profiles/per-user/$(id -un)/bin
herd_path_head=$herd_path_head:/run/current-system/sw/bin
case $PATH in
  "$herd_path_head"|"$herd_path_head":*) ;;  # already in front; do not grow
  *) PATH=$herd_path_head${PATH:+:$PATH} ;;
esac
export PATH
unset herd_path_head

# Alexa's subscription, never an ambient API key, gateway or cloud backend:
# one exported variable in a parent shell would otherwise bill a third party
# and bypass the account the session is supposed to run under.
unset ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN ANTHROPIC_BASE_URL
unset CLAUDE_CODE_USE_BEDROCK CLAUDE_CODE_USE_VERTEX CLAUDE_CODE_USE_FOUNDRY
# Do not carry a previous wrapper's loader overrides into `nix develop`.
unset LD_LIBRARY_PATH LD_PRELOAD
# A shell that sourced chatOnS's env.sh first must not keep its markers: they
# are what tells a session, and a script, that it is chatOnS's.
# shellcheck disable=SC2086  # deliberate splitting: one name per variable
unset ${!CHATONS_@} HC

umask 077

# Refuse rather than scatter: if any of these resolved outside the state root
# we would be writing Herd's session state into someone else's directory
# again, which is the whole failure this file exists to prevent. GH_CONFIG_DIR
# is not in the list and must not be: it is someone else's directory on
# purpose, it holds no Herd state, and this script never creates it.
for herd_path in "$HOME" "$CLAUDE_CONFIG_DIR" "$XDG_CONFIG_HOME" \
  "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_RUNTIME_DIR" \
  "$TMPDIR" "$CARGO_HOME"; do
  case $(realpath -m "$herd_path") in
    "$HERD_STATE_ROOT"/*)
      mkdir -p "$herd_path" || {
        echo "herd env: cannot create $herd_path" >&2
        if [ "${BASH_SOURCE[0]}" = "$0" ]; then exit 1; else return 1; fi
      }
      ;;
    *)
      echo "herd env: state path outside $HERD_STATE_ROOT: $herd_path" >&2
      if [ "${BASH_SOURCE[0]}" = "$0" ]; then exit 1; else return 1; fi
      ;;
  esac
done
unset herd_path
