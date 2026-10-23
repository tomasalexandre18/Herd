#!/bin/sh
# Does the hub's *published container image* actually run a job? This script
# answers that, against an image reference docker can already get — which
# since D162 means a tag `docker load` just put there, out of the archive the
# release attaches.
#
#   image-smoke.sh <image reference> <directory holding herd-runner, herdctl>
#
# `.github/workflows/release.yml` runs it once per run, tag or no tag, on the
# archive the flake built and loaded locally. It is now the *only* proof the
# image has — D153's second run, against an image pulled back from Docker Hub
# by digest, went with the registry — and that makes it worth more than it
# was, not less: `docker load` and run is exactly what an operator does with
# the attached archive, so this script and the deployment now walk the same
# path. It needs no credentials, which is the other half of why it survived:
# it cannot be the step that stops a release for want of a secret.
#
# It is the image's sibling of `scripts/release-smoke.sh`: a real hub, a real
# runner and two real jobs, not `--version` once. The hub runs **in the
# container, shaped the way `deploy/docker-compose.yml` shapes it** — read
# only root filesystem, a volume on `/var/lib/herd` and nothing under it,
# HERD_STATE_DIR naming that same path, the image's own non-root user — so a
# change that broke the deployment's first start fails here rather than on
# the server. The runner and `herdctl` are on the host, dialling the
# published loopback port, which is the first of the two paths
# `deploy/release-notice.md` calls proven.
#
# POSIX sh for the same reason the sibling is: nothing here should be testing
# a distribution's bash.
set -eu

image=${1:-}
bin=${2:-}
if [ -z "$image" ] || [ -z "$bin" ]; then
  echo "usage: image-smoke.sh <image reference> <directory with the binaries>" >&2
  exit 2
fi
PATH="$bin:$PATH"
export PATH

# Above 20000, and not release-smoke.sh's 28787: the two run one after the
# other in the same job, on a host that may be slow to release a port.
port=${HERD_IMAGE_SMOKE_PORT:-28788}
hub_url="ws://127.0.0.1:$port"
container="herd-image-smoke-$$"
root=$(mktemp -d)
runner_pid=
started=

cleanup() {
  [ -z "$runner_pid" ] || kill "$runner_pid" 2>/dev/null || true
  if [ -n "$started" ]; then
    echo "image-smoke: the hub container's own log"
    docker logs "$container" 2>&1 | sed 's/^/    /' || true
    # -v: the anonymous state volume goes with it. A smoke run leaves nothing
    # behind on the host it ran on.
    docker rm -f -v "$container" >/dev/null 2>&1 || true
  fi
  rm -rf "$root"
}
trap cleanup EXIT INT TERM

fail() {
  echo "image-smoke: $1" >&2
  exit 1
}

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

echo "image-smoke: $image"
# What docker has, before anything is run out of it. The id is the content,
# and the user is the one claim a working entrypoint would hide: a hub that
# came up as root would still answer, and must not pass here.
docker image inspect "$image" \
  --format 'image-smoke: id {{.Id}}, user {{.Config.User}}, entrypoint {{.Config.Entrypoint}}'
user=$(docker image inspect "$image" --format '{{.Config.User}}')
[ "$user" = "herd" ] || fail "the image runs as '$user', not herd"

# 1. The hub, in the container, shaped like the deployment. `--read-only` is
#    not a flourish: `deploy/docker-compose.yml` sets it, so an image that
#    grew a write outside its state volume has to fail somewhere that is not
#    the server.
docker run -d --name "$container" \
  -p "127.0.0.1:$port:$port" \
  -e "HERD_LISTEN=0.0.0.0:$port" \
  -e "HERD_STATE_DIR=/var/lib/herd" \
  -v "/var/lib/herd" \
  --read-only \
  --tmpfs /tmp \
  --cap-drop ALL \
  --security-opt "no-new-privileges:true" \
  "$image" >/dev/null
started=yes

wait_for "the hub in the container never answered on $hub_url" \
  herdctl --hub "$hub_url" runner list

# 2. The version is the binary's own word, not a tag somebody typed. The
#    image has no shell, so this is `docker run` on the entrypoint and not
#    `docker exec sh -c`.
inside=$(docker run --rm "$image" --version)
echo "image-smoke: the hub inside says: $inside"

# 3. A runner on the host, approved and carrying the token the hub minted.
token=$(herdctl --hub "$hub_url" runner approve image-smoke)
[ -n "$token" ] || fail "approving the runner printed no token on stdout"
HERD_TOKEN="$token" herd-runner --hub "$hub_url" --name image-smoke \
  --data-root "$root/runner" &
runner_pid=$!
wait_for "the runner never reached the hub in the container" \
  sh -c "herdctl --hub '$hub_url' runner list | grep -q 'connected now'"

# 4. One real job, placed by the hub in the container and recorded on the
#    volume docker seeded from the image's own directory.
out=$(herdctl --hub "$hub_url" run -- sh -c 'echo the-image-hub-placed-this')
echo "$out"
echo "$out" | grep -q the-image-hub-placed-this ||
  fail "the job's own output did not come back"

# 5. And the verdict is worn untouched (E-02), which is the hub's half of it.
code=0
herdctl --hub "$hub_url" run -- sh -c 'exit 7' || code=$?
[ "$code" = 7 ] || fail "a job exiting 7 gave herdctl exit $code, not 7"

echo "image-smoke: a containerised hub, a runner and two jobs, out of $image"
