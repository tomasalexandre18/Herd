#!/usr/bin/env bash
# What a WSL2 machine really gives a Herd runner, read once and printed in a
# block you can paste into a message. For M17: the second machine is a
# Windows PC joining as a runner inside WSL2 (D145), and every line below is
# something nobody on the project can check from anywhere else.
#
# Run it on the PC, inside the WSL distribution the runner will live in:
#
#   ./deploy/wsl2-check.sh                        # hub address from HERD_HUB_URL
#   ./deploy/wsl2-check.sh ws://100.x.y.z:8787    # or given here
#
# It reads this machine and changes nothing that outlives it. One check is
# not a pure read and is marked where it happens: DELEGATION starts a
# transient systemd scope, creates one cgroup directory inside that scope's
# own cgroup and removes it again. That is the same probe the runner itself
# does at startup (herd-runner/src/cgroup.rs), and there is no way to answer
# "may this user make a cgroup here" without trying. The scope is gone when
# the probe returns; nothing is installed, enabled, written to /etc or left
# behind.
#
# Exit code: 0 when nothing FAILED, 1 otherwise. UNKNOWN is not a failure —
# it is this script saying it could not find out, which is information.

set -u

fails=0
unknowns=0

# One result line. State is PASS, FAIL or UNKNOWN; the rest is the evidence,
# because a bare PASS is not something a reader can check.
say() {
  case "$1" in
    FAIL) fails=$((fails + 1)) ;;
    UNKNOWN) unknowns=$((unknowns + 1)) ;;
  esac
  printf '%-8s %-14s %s\n' "$1" "$2" "$3"
}

note() { printf '         %-14s %s\n' '' "$1"; }

section() { printf '\n-- %s\n' "$1"; }

printf '==== herd wsl2-check ====\n'
printf 'date     %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

section 'this machine'

kernel=$(uname -sr 2>/dev/null || echo unknown)
say INFO kernel "$kernel"

if grep -qiE 'microsoft|wsl' /proc/version 2>/dev/null; then
  say PASS wsl2 "/proc/version names Microsoft: $(cat /proc/version)"
else
  say UNKNOWN wsl2 "/proc/version does not name Microsoft, so this is \
probably not WSL2: $(cat /proc/version 2>/dev/null || echo unreadable)"
fi
say INFO distro "${WSL_DISTRO_NAME:-\$WSL_DISTRO_NAME is unset (not WSL, or an old WSL)}"
if [ -r /etc/os-release ]; then
  say INFO os "$(. /etc/os-release && printf '%s' "${PRETTY_NAME:-${NAME:-unknown}}")"
fi
say INFO user "$(id -un) ($(id -u)), groups: $(id -nG)"
for group in docker wheel sudo libvirtd; do
  if id -nG | tr ' ' '\n' | grep -qx "$group"; then
    note "this user is in \"$group\", which is root in all but name: the \
runner warns about it by name at startup and the answer is a user in none \
of them (D78)"
  fi
done

section 'systemd as PID 1'

pid1=$(ps -p 1 -o comm= 2>/dev/null | tr -d ' ')
if [ "$pid1" = systemd ]; then
  say PASS pid1 "PID 1 is systemd ($(readlink -f /proc/1/exe 2>/dev/null || echo 'exe unreadable'))"
else
  say FAIL pid1 "PID 1 is \"${pid1:-unreadable}\", not systemd. WSL2 does \
not boot systemd unless it is asked to: put [boot]/systemd=true in \
/etc/wsl.conf, then run \"wsl --shutdown\" from Windows and open the \
distribution again"
fi

if [ -r /etc/wsl.conf ]; then
  say INFO wsl.conf "/etc/wsl.conf reads:"
  sed 's/^/           | /' /etc/wsl.conf
else
  say INFO wsl.conf "/etc/wsl.conf does not exist (or is unreadable), so \
this distribution asks for nothing at boot"
fi

if command -v systemctl > /dev/null 2>&1; then
  say INFO systemd "$(systemctl --version 2>/dev/null | head -1)"
  state=$(systemctl is-system-running 2>/dev/null)
  say INFO system-state "systemctl is-system-running: ${state:-unreadable}"
else
  say FAIL systemd 'no systemctl on PATH at all'
fi

section 'cgroup v2'

fstype=$(stat -fc %T /sys/fs/cgroup 2>/dev/null)
if [ "$fstype" = cgroup2fs ]; then
  say PASS cgroup2 '/sys/fs/cgroup is cgroup2fs (stat -fc %T)'
else
  say FAIL cgroup2 "stat -fc %T /sys/fs/cgroup says \"${fstype:-unreadable}\", \
not cgroup2fs. Herd's per-job memory ceiling is a cgroup v2 subtree; \
without one it falls back to a per-process address-space limit"
fi

if [ -r /sys/fs/cgroup/cgroup.controllers ]; then
  say INFO controllers "at the root: $(cat /sys/fs/cgroup/cgroup.controllers)"
else
  say UNKNOWN controllers '/sys/fs/cgroup/cgroup.controllers is unreadable'
fi

own=$(sed -n 's/^0:://p' /proc/self/cgroup 2>/dev/null)
say INFO own-cgroup "this shell sits in ${own:-an unreadable cgroup}"

section 'a delegated subtree for a user unit'
note 'this check starts a transient scope and removes one directory'

if ! command -v systemd-run > /dev/null 2>&1; then
  say UNKNOWN delegation 'no systemd-run on PATH, so this cannot be tested here'
else
  probe=$(systemd-run --user --scope -p Delegate=yes --quiet -- /bin/sh -c '
    cg=$(sed -n "s/^0:://p" /proc/self/cgroup)
    base=/sys/fs/cgroup$cg
    printf "cgroup=%s\n" "$base"
    if [ -r "$base/cgroup.controllers" ]; then
      printf "controllers=%s\n" "$(cat "$base/cgroup.controllers")"
    else
      printf "controllers=unreadable\n"
    fi
    if mkdir "$base/herd-delegation-probe" 2>/dev/null; then
      printf "mkdir=ok\n"
      rmdir "$base/herd-delegation-probe" 2>/dev/null
    else
      printf "mkdir=denied\n"
    fi
  ' 2>&1)
  status=$?
  controllers=$(printf '%s\n' "$probe" | sed -n 's/^controllers=//p')
  made=$(printf '%s\n' "$probe" | sed -n 's/^mkdir=//p')
  cgroup=$(printf '%s\n' "$probe" | sed -n 's/^cgroup=//p')
  has_memory=no
  case " $controllers " in *' memory '*) has_memory=yes ;; esac
  if [ "$status" -ne 0 ] || [ -z "$made" ]; then
    say UNKNOWN delegation "systemd-run --user --scope -p Delegate=yes could \
not be run (exit $status). Its own words follow, and they are the answer:"
    printf '%s\n' "$probe" | sed 's/^/           | /'
    note "a user manager is needed for this: it wants XDG_RUNTIME_DIR and a \
running user@.service, which is one of the things systemd as PID 1 brings"
  elif [ "$made" = ok ] && [ "$has_memory" = yes ]; then
    say PASS delegation "a user scope with Delegate=yes got $cgroup, offering \
\"$controllers\", and this user could create a cgroup under it. That is \
exactly what the runner needs for a kernel-enforced per-job memory ceiling"
  elif [ "$made" = ok ]; then
    say FAIL delegation "a cgroup could be created under $cgroup, but its \
controllers are \"$controllers\" and \"memory\" is not among them, so no \
kernel memory ceiling can be set there"
  else
    say FAIL delegation "a user scope with Delegate=yes was created at \
$cgroup but this user could not make a cgroup under it (controllers: \
\"$controllers\"). The runner will say FALLING BACK"
  fi
fi

section 'what WSL2 gave this VM, and what Herd may lend inside it'

memtotal_kb=$(sed -n 's/^MemTotal: *\([0-9]*\) kB/\1/p' /proc/meminfo 2>/dev/null)
if [ -n "${memtotal_kb:-}" ]; then
  say INFO memtotal "$((memtotal_kb / 1024)) MiB (/proc/meminfo MemTotal). \
Under WSL2 this is the VM's size and not the PC's RAM, and it is the figure \
--host-memory has to stay under"
else
  say UNKNOWN memtotal '/proc/meminfo could not be read'
fi
say INFO threads "$(nproc 2>/dev/null || echo unknown) hardware threads visible here"
swap_kb=$(sed -n 's/^SwapTotal: *\([0-9]*\) kB/\1/p' /proc/meminfo 2>/dev/null)
say INFO swap "${swap_kb:-unknown} kB of swap. WSL2 gives its VM a swap \
file of its own unless .wslconfig says otherwise"

found_wslconfig=no
for candidate in /mnt/c/Users/*/.wslconfig; do
  [ -r "$candidate" ] || continue
  found_wslconfig=yes
  say INFO .wslconfig "$candidate reads:"
  sed 's/^/           | /' "$candidate"
done
if [ "$found_wslconfig" = no ]; then
  if [ -d /mnt/c/Users ]; then
    say UNKNOWN .wslconfig "no readable .wslconfig under /mnt/c/Users/*. \
Without one WSL2 picks the VM's memory itself, and MemTotal above is what it \
picked. If that figure is not what this machine should lend, write \
%USERPROFILE%\\.wslconfig with [wsl2]/memory= and run \"wsl --shutdown\""
  else
    say UNKNOWN .wslconfig "/mnt/c/Users does not exist, so no Windows \
drive is mounted here and this is almost certainly not WSL2"
  fi
fi

section 'the hub, dialled out to (nothing here ever listens)'

hub=${1:-${HERD_HUB_URL:-}}
if [ -z "$hub" ]; then
  say UNKNOWN hub "no hub address given. Pass it as the first argument, or \
set HERD_HUB_URL, and run this again — the one network fact worth having is \
whether this machine can reach the hub"
else
  say INFO hub "$hub"
  rest=${hub#*://}
  scheme=${hub%%://*}
  rest=${rest%%/*}
  host=${rest%%:*}
  port=${rest##*:}
  if [ "$port" = "$host" ]; then
    case "$scheme" in
      wss | https) port=443 ;;
      *) port=8787 ;;
    esac
    note "no port in the URL, so this check assumes $port"
  fi
  if timeout 5 bash -c "exec 3<>/dev/tcp/$host/$port" 2> /dev/null; then
    say PASS hub-tcp "a TCP connection to $host:$port opened from here. That \
is reachability and not a handshake: the runner's own dial is the proof, and \
it is step 3 of docs/joining.md"
  else
    say FAIL hub-tcp "no TCP connection to $host:$port from here within 5 s. \
If that address is on the tailnet, the question is whether the tailnet is \
reachable from inside this distribution at all — see the next lines"
  fi
fi

if command -v tailscale > /dev/null 2>&1; then
  say INFO tailscale "a tailscale CLI is here: $(tailscale version 2>/dev/null | head -1)"
  addr=$(tailscale ip -4 2>/dev/null | head -1)
  if [ -n "${addr:-}" ]; then
    say PASS tailnet "this distribution has its own tailnet address $addr"
  else
    say UNKNOWN tailnet "the tailscale CLI is here but gave no IPv4 address: \
\"tailscale status\" will say why"
  fi
else
  say UNKNOWN tailscale "no tailscale CLI inside this distribution. That is \
not a failure: the other shape is Windows holding the tailnet and WSL2 \
reaching it through mirrored networking. hub-tcp above is what decides which \
of the two this machine is on"
fi

if [ -e /dev/net/tun ]; then
  say PASS /dev/net/tun "present ($(stat -c '%A %U:%G' /dev/net/tun 2>/dev/null)), \
so a mesh daemon can run inside this distribution"
else
  say UNKNOWN /dev/net/tun "absent, so a mesh daemon cannot run inside this \
distribution and the tailnet has to come from Windows"
fi

listening=$(ss -tlnp 2>/dev/null | grep -c herd)
say INFO listening "${listening:-0} listening socket(s) mentioning herd \
(invariant 3 says this is always 0)"

section 'the binary'

if command -v herd-runner > /dev/null 2>&1; then
  say PASS herd-runner "$(command -v herd-runner): $(herd-runner --version 2>&1 | head -1)"
  file_says=$(file -b "$(command -v herd-runner)" 2>/dev/null)
  [ -z "$file_says" ] || note "file says: $file_says"
else
  say UNKNOWN herd-runner "not on PATH. The release carries a static musl \
binary that needs no Nix and no libc: see deploy/release-notice.md"
fi
if command -v herdctl > /dev/null 2>&1; then
  say INFO herdctl "$(herdctl --version 2>&1 | head -1)"
fi

printf '\n-- verdict\n'
printf '%s FAILED, %s UNKNOWN\n' "$fails" "$unknowns"
printf 'Send this whole block back. UNKNOWN lines are questions, not faults.\n'
printf '==== end herd wsl2-check ====\n'

[ "$fails" -eq 0 ]
