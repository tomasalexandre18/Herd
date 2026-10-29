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
# Why it reads the cgroup too. On 2026-10-29 five agents each lost a whole
# twelve-core suite run to a preflight that had cleared them, and three
# branches sat red because of it. /proc/loadavg and MemAvailable both read
# fine; what decides whether the suite can pass is neither. The suite runs
# inside a systemd slice — /chatons.slice on this host, memory.high 18 GiB —
# that sibling builds had already filled, and the real-binary tests need a
# runner that can lend 2 GiB. The runner then *correctly* announced the room
# the slice had left: "announcing 661 MiB of memory rather than 31.1 GiB,
# because /proc/self/cgroup is enclosed by memory.high 18 GiB on
# /chatons.slice, which already holds 17.3 GiB". The tests failed on a true
# statement about the host.
#
# The two readings are independent, measured the same day: runs were cleared
# at loads of 8.33, 2.27 and 4.15 and went red, slice room was seen at 3,
# 326, 661, 772, 1281 and 6591 MiB, and 1.3 GiB of room was measured at a
# load of 1.28. Reading one says nothing about the other, so check 4 below
# reads the room in the cgroup this script is *actually* standing in.
#
# It follows herd-runner/src/room.rs step for step — the same walk, the same
# choice of knob, the same subtraction of reclaimable page cache, the same
# reading of an unreadable memory.current — because two readings of one figure
# that disagree would be worse than one that is absent. Where nothing encloses
# this process there is nothing to read: the check says the reading was
# unavailable and clears. An unmeasured thing is reported as unmeasured, never
# as a refusal; refusing on an unreadable file would stop every build on every
# machine with no slice.
#
# The subtraction was added on 2026-10-29 with room.rs's own: memory.current
# counts reclaimable page cache, so this check read 3224 MiB of room at 22:17
# that evening on an idle host whose MemAvailable said 22.3 GiB, and would
# have refused every heavy phase on it at the floor of 8 GiB below.
# inactive_file less shmem comes off the charge; active_file does not. Both
# readings moved together, as the paragraph above requires.
#
# What it is worth moves by the hour. Measured with this script, old reading
# then new: 9548 then 12107 MiB at 22:35, when only 2.5 of the host's 7.8 GiB
# of cache was inactive; 3054 then 10181 MiB at 22:44, the two scripts run
# back to back on an idle host whose MemAvailable said 23.1 GiB — where the
# old arithmetic refused the phase at the 8 GiB floor and the new one cleared
# it. Quote the range, not one of the three.
#
# It reads the host and exits non-zero with one sentence saying what to do.
# It changes nothing, starts nothing and needs no toolchain.
#
# Usage: ./scripts/bench-preflight.sh [--target-dir <path>] [--quiet]
# Exit:  0 safe to start, 1 refused (reason on stderr), 2 bad usage.
# Each threshold below is overridable by the environment variable named
# beside it, for a host whose shape is not this one's.
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

# Floor on the room left in the cgroup this phase will run in, in GiB. Not
# the same figure as MemAvailable above and not a substitute for it: a slice
# can be nearly full while the host has memory to spare, and it is the slice
# that decides whether the suite can pass.
#
# Why 8 and not 2. The real-binary tests need a runner that can lend 2 GiB,
# so 2 is where they cannot even start. One agent held for 5 GiB on
# 2026-10-29 and still lost the suite: its own cargo and rustc are charged to
# the same cgroup, and they grew about 4 GiB while it ran. 2 for the test
# plus ~4 for the suite's measured growth is 6, and 8 leaves 2 GiB of slack
# for the sibling phase that starts while this one runs — workflow.md allows
# two at once. It is a starting point from one day's measurements on one
# host, not a constant of nature; fractions are accepted.
MIN_CGROUP_ROOM_GIB=${HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB:-8}

# The two files check 4 reads. Variables rather than constants so that the
# walk and the arithmetic can be driven over a tree a test builds, on any
# host: what a host cannot do is *create* a bounded cgroup, not read one.
# The same seam, for the same reason, as `enclosing()` in room.rs.
CGROUP_MOUNT=${HERD_PREFLIGHT_CGROUP_MOUNT:-/sys/fs/cgroup}
SELF_CGROUP=${HERD_PREFLIGHT_SELF_CGROUP:-/proc/self/cgroup}

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
    # The header block, counted rather than written down: a literal range
    # goes stale the first time a paragraph is added above it, and it did.
    -h|--help)
      sed -n "2,$(($(grep -n '^set -uo pipefail' "$0" | cut -d: -f1) - 1))p" "$0" \
        | sed 's/^# \{0,1\}//'; exit 0 ;;
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

# --- Reading the cgroup, the way herd-runner/src/room.rs reads it ----------
#
# The four functions below are that module's `own_cgroup`, `limit_in`,
# `reclaimable_in` and `enclosing` in shell, and nothing here is a second
# opinion: the knob is the lower of memory.max and memory.high, because they
# fail differently but say the same thing about how much memory can be held
# here; the tightest ceiling anywhere up the chain wins, because a bound three
# levels up still binds; the charge is subtracted once, less the reclaimable
# page cache inside it, because memory.current counts cache the kernel drops
# on demand and holding a warm cache against the room announced almost
# nothing on an idle machine; the reclaimable figure is inactive_file less
# shmem — active_file may be in use and shmem is swap-backed rather than
# droppable, so neither is given back, which makes the result a floor on the
# room rather than an estimate of it; and a cgroup whose ceiling reads while
# its memory.current does not is read as fully charged with nothing reclaimed
# against that assumption, erring towards less room, which is the only
# direction invariant 2 allows.

# This process's cgroup, out of the `0::` line and no other: a host still
# showing cgroup v1 controllers writes one line per hierarchy and none of
# them is a v2 path. No `0::` line is a host with no cgroup v2 at all, and
# then there is no enclosing ceiling to read.
own_cgroup() {
  local line
  [ -r "$SELF_CGROUP" ] || return 1
  while IFS= read -r line; do
    case "$line" in
      0::*) printf '%s\n' "${line#0::}" | tr -d ' \t\r'; return 0 ;;
    esac
  done < "$SELF_CGROUP"
  return 1
}

# The figure in a memory.max, memory.high or memory.current, or nothing:
# `max` is the word the kernel writes for "no ceiling", which is not a
# figure, and an unreadable file is not one either.
numeric_in() {
  local text
  text=$(cat -- "$1" 2>/dev/null) || return 1
  text=${text//[[:space:]]/}
  case "$text" in
    ''|*[!0-9]*) return 1 ;;
  esac
  printf '%s\n' "$text"
}

# The reclaimable page cache one cgroup holds, in bytes, out of its
# memory.stat: inactive_file less shmem, and 0 for anything that will not
# read. room.rs's `reclaimable_in`, field for field. A missing field reads as
# 0, which charges this host for cache it may not be holding — the safe
# direction, and the figure this check used before the subtraction existed.
reclaimable_in() {
  local file=$1 inactive=0 shmem=0 name value
  [ -r "$file" ] || { printf '0\n'; return 0; }
  while read -r name value; do
    case "$name" in
      inactive_file) inactive=$value ;;
      shmem) shmem=$value ;;
      *) continue ;;
    esac
  done < "$file"
  case "$inactive$shmem" in *[!0-9]*) printf '0\n'; return 0 ;; esac
  printf '%s\n' "$(( inactive > shmem ? inactive - shmem : 0 ))"
}

# The tightest ceiling on the way from the root of $CGROUP_MOUNT down to
# <cgroup>, as `<ceiling bytes> <charged bytes> <reclaimable bytes> <knob>
# <cgroup>`. Non-zero, and nothing printed, when no level up the chain carries
# one — which is the bare machine, and must cost nothing there.
cgroup_enclosure() {
  local rest=${1#/} part path dir level=0 knob value low lowknob
  local best_ceiling="" best_charged="" best_reclaimable="" best_knob=""
  local best_cgroup=""
  local -a names=()
  while [ -n "$rest" ]; do
    part=${rest%%/*}
    [ -z "$part" ] || names+=("$part")
    case "$rest" in */*) rest=${rest#*/} ;; *) rest="" ;; esac
  done
  path=""
  # The root of the mount is walked too: a ceiling can sit anywhere up the
  # chain, including on the whole hierarchy.
  while [ "$level" -le "${#names[@]}" ]; do
    [ "$level" -eq 0 ] || path="$path/${names[$((level - 1))]}"
    dir="$CGROUP_MOUNT$path"
    low=""; lowknob=""
    for knob in memory.max memory.high; do
      if value=$(numeric_in "$dir/$knob"); then
        if [ -z "$low" ] || [ "$value" -lt "$low" ]; then
          low=$value; lowknob=$knob
        fi
      fi
    done
    if [ -n "$low" ] && { [ -z "$best_ceiling" ] || [ "$low" -lt "$best_ceiling" ]; }; then
      best_ceiling=$low; best_knob=$lowknob; best_cgroup=${path:-/}
      if best_charged=$(numeric_in "$dir/memory.current"); then
        best_reclaimable=$(reclaimable_in "$dir/memory.stat")
      else
        # Fully charged, and nothing given back against an assumption: see
        # the paragraph above and room.rs's `ceiling_at`.
        best_charged=$low; best_reclaimable=0
      fi
    fi
    level=$((level + 1))
  done
  [ -n "$best_ceiling" ] || return 1
  printf '%s %s %s %s %s\n' "$best_ceiling" "$best_charged" "$best_reclaimable" \
    "$best_knob" "$best_cgroup"
}

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

# --- 4. Room in the cgroup this phase will actually run in -----------------

case "$MIN_CGROUP_ROOM_GIB" in
  ''|.|*[!0-9.]*|*.*.*)
    printf 'bench-preflight: HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB must be a number of GiB, not %s\n' \
      "$MIN_CGROUP_ROOM_GIB" >&2
    exit 2 ;;
esac
cgroup_floor_mib=$(awk -v g="$MIN_CGROUP_ROOM_GIB" 'BEGIN { printf "%d", g * 1024 }')

if ! own=$(own_cgroup); then
  say "slice room:  no cgroup v2 line in $SELF_CGROUP, reading unavailable"
elif ! enclosure=$(cgroup_enclosure "$own"); then
  say "slice room:  nothing from $own up to the root of $CGROUP_MOUNT carries \
a memory.high or a memory.max, reading unavailable"
else
  read -r ceiling_bytes charged_bytes reclaimable_bytes knob enclosing_cgroup \
    <<<"$enclosure"
  held_bytes=$(( charged_bytes > reclaimable_bytes ? charged_bytes - reclaimable_bytes : 0 ))
  room_mib=$(( (ceiling_bytes > held_bytes ? ceiling_bytes - held_bytes : 0) / 1048576 ))
  say "slice room:  ${room_mib} MiB under $knob on $enclosing_cgroup, floor \
${cgroup_floor_mib} MiB (${MIN_CGROUP_ROOM_GIB} GiB)"
  say "             charge $((charged_bytes / 1048576)) MiB, of which \
$((reclaimable_bytes / 1048576)) MiB is reclaimable page cache and not held"
  if [ "$room_mib" -lt "$cgroup_floor_mib" ]; then
    refuse "only ${room_mib} MiB of memory left under $knob on \
$enclosing_cgroup, floor is ${cgroup_floor_mib} MiB: a runner started here \
would announce that figure and be right, while the real-binary tests need \
one that can lend 2 GiB and the suite grows about 4 GiB more in the same \
cgroup as it runs. The run would go red on a true statement about the host. \
Wait for the sibling phases charged to $enclosing_cgroup to finish, or set \
HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB if this host has a different shape"
  fi
fi

# --- Verdict ---------------------------------------------------------------

if [ "$refused" -ne 0 ]; then
  printf 'bench-preflight: REFUSED, do not start the heavy phase.\n' >&2
  exit 1
fi
say "bench-preflight: ok, safe to start."
# Said on the way out, because every figure above is a reading of this
# instant and nothing here watches the host afterwards. Measured on
# 2026-10-29: two phases cleared at a quiet moment reached a one-minute load
# of 29 mid-suite, and a slice with room at the start was full before the
# suite ended. A watchdog is not this script's job; saying so is.
say "note:        all four figures are of this instant only. Nothing watches"
say "             the host after this line, and a phase cleared at a quiet"
say "             moment has met load 29 and a full slice mid-suite. If the"
say "             run goes red on memory, read these figures again before"
say "             reading the diff."
exit 0
