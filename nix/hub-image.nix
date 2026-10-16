# The hub's container image: `herd-hub:<workspace version>` (M16).
#
# The name and the tag are the deployment's contract — the compose file refers
# to this image by `herd-hub:0.1.0` — and the tag comes from the workspace's
# own version, read once in `flake.nix`, so it can never drift from the binary
# inside the image.
#
# What is in it: the hub, a C library, and two text files naming the user it
# runs as. No shell, no package manager, no CA bundle: the hub execs nothing,
# and TLS is the reverse proxy's job (D4), so none of that would be anything
# but surface.
{
  dockerTools,
  runCommand,
  hub,
  version,
}:
let
  # The hub runs as this user and never as root. A fixed uid, because the
  # state directory is a volume on the host and an operator has to be able to
  # give it to somebody: an image whose uid moved between builds would make
  # that ownership a guess.
  user = "herd";
  uid = 1000;
  gid = 1000;

  # Hand-written rather than `dockerTools.fakeNss`, which would also bring
  # `nobody`, a `/etc/nsswitch.conf` and an `/etc/shadow` this image has no
  # use for. Two users and one group: root, because a numeric lookup that
  # finds nothing confuses tools that read the file, and `herd`, which is who
  # the hub is. Neither can log in.
  passwd = runCommand "herd-hub-passwd" { } ''
    mkdir -p $out/etc
    cat > $out/etc/passwd <<EOF
    root:x:0:0:root:/:/sbin/nologin
    ${user}:x:${toString uid}:${toString gid}:Herd hub:${stateDir}:/sbin/nologin
    EOF
    cat > $out/etc/group <<EOF
    root:x:0:
    ${user}:x:${toString gid}:
    EOF
  '';

  # A mount point, not a default. The hub is told where its state goes at
  # startup — `--state-dir`, or `HERD_STATE_DIR` — and this image bakes no
  # host path; what it bakes is one empty directory inside the container,
  # owned by the hub's own uid, for the deployment to mount a volume over.
  # Docker seeds a fresh named volume from it, so the volume inherits that
  # ownership instead of landing root-owned under a non-root process.
  #
  # This is the deployment's second contract after the image name, and it is
  # exact: the volume is mounted **on this path and not under it**, and
  # `HERD_STATE_DIR` is **this path**, so that the directory docker seeds the
  # volume from is the directory the hub writes. Mounting a subdirectory
  # instead — `/var/lib/herd/state`, which this image does not contain —
  # makes docker create the mount point itself, root-owned, and the hub
  # refuses to start with `Permission denied` on its first job directory. See
  # `deploy/README.md`, "the first start", for that failure in full.
  stateDir = "/var/lib/herd";
in
dockerTools.buildLayeredImage {
  name = "herd-hub";
  tag = version;

  contents = [
    hub
    passwd
  ];

  # `chown` needs a root that only fakeroot can give inside a build.
  fakeRootCommands = ''
    mkdir -p .${stateDir}
    chown -R ${toString uid}:${toString gid} .${stateDir}
    chmod 0700 .${stateDir}
  '';

  config = {
    Entrypoint = [ "/bin/herd-hub" ];
    User = user;
    # So that the hub's own relative default for `--state-dir` — `herd-state`
    # — lands inside the volume rather than somewhere nothing may be written.
    # The deployment sets `HERD_STATE_DIR` and never relies on this, but an
    # operator who forgets it gets `/var/lib/herd/herd-state`: a hub that
    # starts, and whose state is still on the volume anyone would back up,
    # rather than one that dies on a read-only root.
    WorkingDir = stateDir;
    # No `ExposedPorts` on purpose: the port is `--listen`, given at startup,
    # and an image that advertised one would be advertising a guess. The
    # deployment publishes what it publishes.
    Env = [ "RUST_BACKTRACE=1" ];
  };
}
