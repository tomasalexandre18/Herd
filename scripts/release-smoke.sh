#!/bin/sh
# Does a release's three binaries actually work on the machine they were
# downloaded onto? This script answers that, and it is written to be run
# **inside a container of an ordinary Linux distribution with no Nix at
# all** — `.github/workflows/release.yml` runs it in `archlinux` and in
# `alpine` on every release, because the binaries the flake builds by
# default are linked against glibc out of the Nix store and would not even
# exec there.
#
#   release-smoke.sh <directory holding herd-hub, herd-runner and herdctl>
#
# It is a real hub, a real runner and a real job — not `--help` three times.
# `--help` proves the loader, and the loader is only the first thing that can
# go wrong with a static binary: musl resolves names with its own resolver,
# opens its own sockets and reads its own `/etc`, so joining a hub by
# **hostname** and running a job that execs a shell is what proves the rest.
#
# POSIX sh, no bash: `alpine` has no bash, and a script that needed one would
# be testing the distribution's packages instead of Herd's binaries.
set -eu

bin=${1:-}
if [ -z "$bin" ]; then
  echo "usage: release-smoke.sh <directory holding the three binaries>" >&2
  exit 2
fi
PATH="$bin:$PATH"
export PATH

# Above 20000 so that this never collides with a hub somebody is running on
# the default 8787.
port=${HERD_SMOKE_PORT:-28787}
hub_url="ws://localhost:$port"
root=$(mktemp -d)
hub_pid=
runner_pid=

cleanup() {
  [ -z "$runner_pid" ] || kill "$runner_pid" 2>/dev/null || true
  [ -z "$hub_pid" ] || kill "$hub_pid" 2>/dev/null || true
  rm -rf "$root"
}
trap cleanup EXIT INT TERM

fail() {
  echo "release-smoke: $1" >&2
  exit 1
}

# Wait for a command to succeed, saying what it was waiting for if it never
# does. Thirty seconds: a dial that has not happened by then is broken, not
# slow.
wait_for() {
  what=$1
  shift
  n=0
  until "$@" >/dev/null 2>&1; do
    n=$((n + 1))
    [ "$n" -lt 30 ] || fail "$what"
    sleep 1
  done
}

echo "release-smoke: on $(uname -sm), $(sed -n 's/^PRETTY_NAME=//p' /etc/os-release 2>/dev/null || echo 'an unnamed distribution')"
echo "release-smoke: nix is $(command -v nix || echo 'absent, which is the point')"

# 1. Each binary execs and answers. A dynamically linked binary on a machine
#    with no Nix store dies here, before main, and says so.
herd-hub --version
herd-runner --version
herdctl --version

# 2. A hub, with its own state root under a temporary directory.
herd-hub --listen "127.0.0.1:$port" --state-dir "$root/hub" &
hub_pid=$!
wait_for "the hub never answered on $hub_url" \
  herdctl --hub "$hub_url" runner list

# 3. A runner, approved and carrying the token the hub minted. The token goes
#    through the environment and never onto a command line, for the reason
#    `docs/joining.md` step 5 gives.
token=$(herdctl --hub "$hub_url" runner approve smoke)
[ -n "$token" ] || fail "approving the runner printed no token on stdout"
HERD_TOKEN="$token" herd-runner --hub "$hub_url" --name smoke \
  --data-root "$root/runner" &
runner_pid=$!
wait_for "the runner never reached the hub" \
  sh -c "herdctl --hub '$hub_url' runner list | grep -q 'connected now'"

# 4. One real job: the hub placed it, the runner execed a shell under its
#    caps, the log came back and the verdict with it.
out=$(herdctl --hub "$hub_url" run -- sh -c 'echo a-static-herd-ran-this; id -u')
echo "$out"
echo "$out" | grep -q a-static-herd-ran-this ||
  fail "the job's own output did not come back"

# 5. And the job's exit code is worn untouched, which is the whole of E-02.
code=0
herdctl --hub "$hub_url" run -- sh -c 'exit 7' || code=$?
[ "$code" = 7 ] || fail "a job exiting 7 gave herdctl exit $code, not 7"

echo "release-smoke: a hub, a runner and two jobs, all static, no Nix anywhere"
