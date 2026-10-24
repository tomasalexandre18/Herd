#!/usr/bin/env bash
# Refuses to let a heavy phase start on a host that cannot carry it.
#
# Run this before every compile, test run or bench on the shared host:
#
#   ./scripts/bench-preflight.sh && nix develop --command ./scripts/test.sh
#
# Why it exists. On 2026-10-24 the root filesystem reached 100 %: thirty-two
# agent worktrees under /tmp/claude-1002 each carried a `target/` of 1.2 to
# 5.4 GiB on `rpool/root` (68 GiB), while /mnt/ssd_980 (341 GiB free) sat
# empty. An agent's `./scripts/test.sh` died on ENOSPC mid-compile, and this
# host also runs the chatOnS production, so a full `/` is the consumer's
# incident, not ours. Nothing checked before building and nothing cleaned up
# after. This script is the "checked before" half; the "cleaned up after"
# half is the rule in docs/orchestration/gotchas/build-weight-off-root.md.
#
# It reads the host and exits non-zero with one sentence saying what to do.
# It changes nothing, starts nothing and needs no toolchain.
#
# Usage: ./scripts/bench-preflight.sh [--target-dir <path>] [--quiet]
# Exit:  0 safe to start, 1 refused (reason on stderr), 2 bad usage.
#
# Note on style: `set -e` is deliberately absent, as in the other scripts
# here, so that a refusal is always a printed sentence and never a silent
# non-zero. That makes every `cd` and every command substitution the
# author's responsibility: `scripts/check-orchestration.sh` once had an
# untested `cd` that would have committed into a real repository when the
# directory was missing. Every `cd` below is tested.
set -uo pipefail
export LC_ALL=C

# --- Named thresholds. Each one is a measurement, not a round number. ------

# Free space, in GiB, required on the filesystem that will hold `target/`.
# The largest `target/` measured on 2026-10-24 was 5.4 GiB; one cold run of
# the four gates on 2026-10-25 (clippy --all-targets, then the suite) left
# 6.6 GiB. So one full set of gates costs about 7 GiB. Leave 3 more for a
# second heavy agent that may start while you build — workflow.md allows two
# at once — and the floor is 10. Below it a build can still finish, but it
# can also die on ENOSPC halfway, which is the failure this script exists to
# prevent.
MIN_FREE_GIB=${HERD_PREFLIGHT_MIN_FREE_GIB:-10}

# Ceiling on the 1-minute load average, per core. Two heavy agents, each
# told to build with `-j 4`, put about 8 runnable tasks on this 12-core
# host: 0.67 per core. 0.75 is the first value above that, so this gate
# refuses the *third* heavy phase and leaves the two the workflow allows
# alone. It also catches a burst from the chatOnS production, which shares
# the host and is not counted anywhere else.
MAX_LOAD_PER_CORE=${HERD_PREFLIGHT_MAX_LOAD_PER_CORE:-0.75}

# Floor on MemAvailable, in GiB. The host has 31 GiB and the production
# holds about 19 of them. This is not a cap on the build — cargo's memory is
# bounded by `-j`, not by a shell script — it is a floor below which the
# next rustc is what sends the kernel looking for something to kill, and the
# largest resident process here belongs to the consumer. 2 GiB is roughly
# two concurrent rustc invocations' peak, so crossing it means the build
# would be allocating out of someone else's pocket.
MIN_AVAIL_MEM_GIB=${HERD_PREFLIGHT_MIN_AVAIL_MEM_GIB:-2}

# Where build output belongs on this host: Herd's own directory, on the
# volume with room. Absent on any other machine, and then this check is
# skipped rather than failed.
HERD_BUILD_ROOT=${HERD_BUILD_ROOT:-/mnt/ssd_980/herd/build}

# --- Arguments -------------------------------------------------------------

quiet=0
target_dir=""
while [ $# -gt 0 ]; do
  case "$1" in
    --target-dir)
      [ $# -ge 2 ] || { printf 'bench-preflight: --target-dir needs a path\n' >&2; exit 2; }
      target_dir=$2; shift 2 ;;
    --quiet) quiet=1; shift ;;
    -h|--help)
      sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) printf 'bench-preflight: unknown argument %s\n' "$1" >&2; exit 2 ;;
  esac
done

root=$(cd -- "$(dirname -- "$0")/.." && pwd) || {
  printf 'bench-preflight: cannot reach the repository root from %s\n' "$0" >&2
  exit 2
}

if [ -z "$target_dir" ]; then
  target_dir=${CARGO_TARGET_DIR:-$root/target}
fi

refused=0
say()    { [ "$quiet" -eq 1 ] || printf '%s\n' "$1"; }
refuse() { refused=1; printf 'bench-preflight: %s\n' "$1" >&2; }

# The filesystem numbers are wanted for the directory that *will* exist, so
# walk up to the nearest ancestor that does.
existing_ancestor() {
  local p=$1
  while [ -n "$p" ] && [ ! -d "$p" ]; do
    local parent=${p%/*}
    [ "$parent" != "$p" ] || break
    p=${parent:-/}
  done
  [ -d "$p" ] && printf '%s\n' "$p"
}

mount_of() { df -k --output=target -- "$1" 2>/dev/null | sed -n '2s/^ *//p'; }
avail_kib_of() { df -k --output=avail -- "$1" 2>/dev/null | awk 'NR==2 {print $1}'; }

# --- 1. Room on the filesystem that will hold target/ ----------------------

probe=$(existing_ancestor "$target_dir")
if [ -z "$probe" ]; then
  refuse "no existing directory on the way to $target_dir; create its parent first"
else
  avail_kib=$(avail_kib_of "$probe")
  target_mount=$(mount_of "$probe")
  if [ -z "${avail_kib:-}" ]; then
    refuse "df could not read the filesystem holding $probe"
  else
    avail_gib=$((avail_kib / 1024 / 1024))
    say "target dir:  $target_dir"
    say "filesystem:  $target_mount, ${avail_gib} GiB free"
    if [ "$avail_gib" -lt "$MIN_FREE_GIB" ]; then
      refuse "only ${avail_gib} GiB free on $target_mount, need ${MIN_FREE_GIB}. \
Free space or point CARGO_TARGET_DIR at a roomier volume; on this host, \
delete the target/ of finished worktrees: see \
docs/orchestration/gotchas/build-weight-off-root.md"
    fi
  fi

  # 1b. On this host specifically, build output must not land on the root
  # filesystem while Herd's own build root exists on the roomy volume. This
  # is the structural half of the 2026-10-24 incident; the free-space check
  # above only catches it once it is already nearly too late.
  if [ -d "$HERD_BUILD_ROOT" ]; then
    build_mount=$(mount_of "$HERD_BUILD_ROOT")
    if [ -n "${target_mount:-}" ] && [ -n "${build_mount:-}" ] &&
       [ "$target_mount" != "$build_mount" ]; then
      refuse "target/ would land on $target_mount while $HERD_BUILD_ROOT \
($build_mount) is the volume with room. Run: \
export CARGO_TARGET_DIR=$HERD_BUILD_ROOT/\$(basename \"\$PWD\")"
    fi
  fi
fi

# --- 2. Load, relative to the cores this host actually has -----------------

ncpu=$(nproc 2>/dev/null)
case "${ncpu:-}" in ''|*[!0-9]*) ncpu=1 ;; esac
read -r load1 _ < /proc/loadavg || load1=""
if [ -z "$load1" ]; then
  refuse "cannot read /proc/loadavg"
else
  load_ceiling=$(awk -v n="$ncpu" -v p="$MAX_LOAD_PER_CORE" 'BEGIN { printf "%.2f", n * p }')
  say "load (1m):   $load1 on $ncpu cores, ceiling $load_ceiling"
  if awk -v a="$load1" -v b="$load_ceiling" 'BEGIN { exit !(a >= b) }'; then
    refuse "load $load1 is at or above $load_ceiling on $ncpu cores: two heavy \
agents are already running, or the consumer's production is busy. Wait, or \
run a read-only task instead"
  fi
fi

# --- 3. Memory headroom, for the production's sake more than the build's ---

mem_avail_kib=$(awk '/^MemAvailable:/ { print $2 }' /proc/meminfo 2>/dev/null)
if [ -z "${mem_avail_kib:-}" ]; then
  say "memory:      MemAvailable not readable, check skipped"
else
  mem_avail_gib=$((mem_avail_kib / 1024 / 1024))
  say "memory:      ${mem_avail_gib} GiB available, floor ${MIN_AVAIL_MEM_GIB}"
  if [ "$mem_avail_gib" -lt "$MIN_AVAIL_MEM_GIB" ]; then
    refuse "only ${mem_avail_gib} GiB of memory available, floor is \
${MIN_AVAIL_MEM_GIB}: the next rustc could get the consumer's production \
killed. Wait for the running phase to finish"
  fi
fi

# --- Verdict ---------------------------------------------------------------

if [ "$refused" -ne 0 ]; then
  printf 'bench-preflight: REFUSED, do not start the heavy phase.\n' >&2
  exit 1
fi
say "bench-preflight: ok, safe to start."
exit 0
