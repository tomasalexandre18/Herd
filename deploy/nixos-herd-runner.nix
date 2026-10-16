# A NixOS module that declares the same runner as deploy/herd-runner.service.
#
# WHAT THIS IS. An offering, not an installation. Nothing in Herd ever writes
# to /etc/nixos — that file is the owner's, and this one only becomes real if
# he imports it. Until he does, the server stays on the imperative path: a
# `useradd --system herd` and a unit copied into /etc/systemd/system/, which
# works and survives a reboot but is unmanaged state outside the NixOS
# generation. A machine is on exactly one of the two paths; `deploy/README.md`
# and `docs/joining.md` say which one, and so does the header of
# deploy/herd-runner.service.
#
# WHAT IT IS EQUIVALENT TO. Every directive of deploy/herd-runner.service,
# with what that unit marks OWNER declared alongside it instead of done by
# hand: the system user, the data root, the binary's path and the /dev/vfio
# group. One OWNER note stays a note on both paths — the token file, which is
# a secret and so must never be a value in a Nix expression (invariant 7).
# The reasoning for each figure is *not* repeated here — it is in the comments
# of that unit, which stay the one place a figure is explained.
#
# THE TWO MUST BE CHANGED TOGETHER. If they drift, nothing fails: both still
# produce a running runner. What breaks is that the machine's real limits stop
# being the ones the unit documents — a MemoryMax lowered in one place and not
# the other, or a --data-root moved on one path only, gives a host whose
# admission figures, memory ceiling and IO weight are not what anybody reading
# `deploy/` believes. The unit is the reference text; this module must say the
# same thing. Reviewing a change to one means opening the other.
#
# HOW TO IMPORT IT, from /etc/nixos. The flake already exposes
# `packages.<system>.herd-runner`, so the package needs nothing new:
#
#   # flake.nix of the system configuration
#   inputs.herd.url = "github:<owner>/herd";
#   ...
#   modules = [
#     (herd + "/deploy/nixos-herd-runner.nix")   # or inputs.herd.outPath
#     {
#       services.herd-runner.enable = true;
#       services.herd-runner.package =
#         inputs.herd.packages.x86_64-linux.herd-runner;
#     }
#   ];
#
# WHAT THE PROJECT'S OWN SERVER SAYS, as the worked example. These are the
# figures the live transient unit on that host already carries (D144), so the
# rebuild that makes it permanent is a no-op and not a change:
#
#     {
#       services.herd-runner.enable = true;
#       services.herd-runner.package =
#         inputs.herd.packages.x86_64-linux.herd-runner;
#       services.herd-runner.jobTimeout = 10800;
#       services.herd-runner.jobPidsMax = 4096;
#       services.herd-runner.jobIoMax =
#         "rbps=775MB,wbps=435MB,riops=180000,wiops=105000";
#       services.herd-runner.jobPath =
#         "/run/wrappers/bin:/run/current-system/sw/bin";
#       services.herd-runner.extraArgs = [
#         "--capabilities /mnt/ssd_980/herd-etc/chatons-runner.json"
#       ];
#     }
#
# jobPath is spelled out there although it is the default, because it is the
# one string a job sees and a host with another system profile must notice it.
# The io.max figures are the first consumer's, a quarter of the kernel's own
# cost model for the device the data root is on; D144 has where they come from
# and what they do not cover.
#
# The flake does *not* expose `nixosModules.herd-runner`. Adding
# `nixosModules.herd-runner = ./deploy/nixos-herd-runner.nix` to flake.nix
# would make the import one line instead of a path join; it is a one-line
# flake change and has deliberately not been made here, because a module
# output is a promise about this file's option names and those are new today.
#
# A JOB'S PATH IS STATED, NOT INHERITED FROM EITHER DEFAULT. The runner
# builds a job's environment from scratch and carries five variables into it —
# PATH, LANG, LC_ALL, TZ and TERM — so the service's PATH is every job's PATH.
# Neither default is usable: systemd's compiled-in one is /usr/bin and friends
# (on NixOS: `sh` and `env`, nothing else), and NixOS's own is coreutils,
# findutils, gnugrep, gnused and systemd with no shell at all. A real job
# proved the first half by exiting 125 on `sh: No such file or directory`. So
# both files name the same string, `services.herd-runner.jobPath` here and
# Environment=PATH= there, and the comment beside that line in the unit has
# the rest — including the four variables that were checked and deliberately
# not added.
#
# ONE DIFFERENCE FROM THE COPIED FILE, real and not a drift to fix:
# `nixos-rebuild switch` restarts the service when this module changes, which
# is a drain — jobs in flight are ended inside --drain-grace and the runner
# comes back. Copying a file over another does nothing until someone runs
# systemctl. Set `restartIfChanged = false` in an override if a rebuild must
# never touch a running build.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.herd-runner;

  # The arguments of the unit's ExecStart line, in the same order. The five
  # that joining.md tells you to change on a different machine are options
  # above; the rest are literal here exactly as they are literal there.
  arguments = [
    "--hub ${cfg.hub}"
    "--name ${cfg.name}"
    "--data-root ${cfg.dataRoot}"
    # Redundant with Delegate=yes and given anyway, for the reason the unit
    # explains (D33): an explicit root that cannot be used refuses the
    # startup, where an absent one quietly falls back to RLIMIT_AS. NixOS
    # puts a system service in system.slice, same as the copied unit, so the
    # path is the same on both paths.
    "--cgroup-root /sys/fs/cgroup/system.slice/herd-runner.service"
    "--job-memory-max ${cfg.jobMemoryMax}"
    "--job-timeout ${toString cfg.jobTimeout}"
    "--job-log-cap 64MiB"
    "--job-disk-budget 20GiB"
    "--job-nice 10"
  ]
  # The two that are absent unless a machine names them, and absent means the
  # flag itself is not on the line: the runner's own default then stands
  # (pids.max 1024, and no io.max line written at all), where a value here
  # would put a figure nobody chose into the unit. `lib.optional` yields an
  # empty list for null, so the ExecStart order above is also the order on the
  # real command line.
  ++ lib.optional (cfg.jobPidsMax != null) "--job-pids-max ${toString cfg.jobPidsMax}"
  ++ lib.optional (cfg.jobIoMax != null) "--job-io-max ${cfg.jobIoMax}"
  ++ [
    "--host-memory ${cfg.hostMemory}"
    "--host-memory-floor ${cfg.hostMemoryFloor}"
    "--host-threads ${toString cfg.hostThreads}"
    "--host-tmpfs 4GiB"
    "--host-tmpfs-path /dev/shm"
    "--health-every 15"
    "--drain-grace 3"
  ]
  ++ cfg.extraArgs;
in
{
  options.services.herd-runner = {
    enable = lib.mkEnableOption "the Herd runner (native, unprivileged)";

    # No default on purpose. The copied unit names
    # /run/current-system/sw/bin/herd-runner, which exists only if the package
    # is in environment.systemPackages — a path that can be missing, and is
    # missing exactly when someone forgot. Here ExecStart is
    # "${cfg.package}/bin/herd-runner", a /nix/store path that the unit's own
    # closure keeps alive: it cannot be absent, and a rebuild that changes the
    # package changes the unit, which is also what restarts the runner.
    # Undefined, this errors by name at evaluation rather than at first start.
    package = lib.mkOption {
      type = lib.types.package;
      example = lib.literalExpression "inputs.herd.packages.x86_64-linux.herd-runner";
      description = ''
        The herd-runner package. The flake exposes it as
        packages.<system>.herd-runner; with an overlay it is pkgs.herd-runner.
      '';
    };

    addToSystemPackages = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Also put the package in environment.systemPackages. ExecStart does not
        need it — it names the store path — but `herd-runner --version` and
        `herd-runner --data-root ... switch off` are run by hand by the
        machine's owner (E-33), and those want it on PATH.
      '';
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "herd";
      description = "The system user the runner runs as.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "herd";
      description = "That user's own group.";
    };

    hub = lib.mkOption {
      type = lib.types.str;
      default = "ws://127.0.0.1:8787";
      description = ''
        The hub's URL. Plain ws:// is right only on the hub's own host, over
        loopback; see the last section of docs/joining.md before changing it.
      '';
    };

    name = lib.mkOption {
      type = lib.types.str;
      default = "server";
      description = ''
        The name the operator approves with `herdctl runner approve <name>`.
        Not derived from networking.hostName: the name is agreed out of band
        and a renamed host must not silently become an unapproved runner.
      '';
    };

    dataRoot = lib.mkOption {
      type = lib.types.str;
      default = "/mnt/ssd_980/herd-runner";
      description = ''
        The one directory this runner may write in (E-45). On the project's
        server this is the ext4 NVMe and not /var/lib, because an io.weight
        reaches a block device and never a ZFS dataset (D120, D126). On a
        machine whose chosen path is ZFS, or btrfs over several devices, the
        IOWeight below is decorative and the runner's startup report says so
        in as many words.
      '';
    };

    tokenFile = lib.mkOption {
      type = lib.types.str;
      default = "/etc/herd/runner-token.env";
      description = ''
        A file holding HERD_TOKEN=..., mode 0400, read by systemd as root
        before it drops to the runner's user (D123). Deliberately a path and
        not a value: a token written into this module would land in
        /nix/store, which is world-readable, and that is invariant 7. It stays
        the owner's to create, outside the generation, and the leading `-`
        below lets the unit start without it so the refusal comes from the hub
        by name.
      '';
    };

    documentation = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = ../docs/joining.md;
      defaultText = lib.literalExpression "../docs/joining.md";
      description = ''
        Installed as /etc/herd/joining.md, which is the file the unit's
        Documentation= names and which nothing in deploy/ installed before
        (D123): `systemctl help herd-runner` worked on neither path and now
        works on this one. The default is a relative path, so it only
        evaluates when this module is imported from a checkout of the Herd
        repository; set it to null if the file was copied out alone.
      '';
    };

    memoryMax = lib.mkOption {
      type = lib.types.str;
      default = "22G";
      description = ''
        The outer bound on everything Herd runs here. The runner reads it at
        startup and admits against it (D119). Lower it and this machine takes
        less; the reasoning for 22G is in deploy/herd-runner.service.
      '';
    };

    jobMemoryMax = lib.mkOption {
      type = lib.types.str;
      default = "8GiB";
      description = "The kernel memory ceiling per job (--job-memory-max).";
    };

    jobTimeout = lib.mkOption {
      type = lib.types.int;
      default = 5400;
      description = ''
        The wall-clock cap on one job, in seconds (--job-timeout). 5400 is
        the figure deploy/herd-runner.service explains and the default here
        so that no machine's behaviour changes by this option existing; a
        consumer whose cold build is long says its own number instead of
        passing one through extraArgs.
      '';
    };

    jobPidsMax = lib.mkOption {
      type = lib.types.nullOr lib.types.int;
      default = null;
      example = 4096;
      description = ''
        How many processes one job may hold at once: `pids.max` in its cgroup
        (--job-pids-max). Null leaves the flag off the line, so the runner's
        own default of 1024 stands — which is deliberate, because a figure in
        the unit should be one somebody chose.
      '';
    };

    jobIoMax = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "rbps=775MB,wbps=435MB,riops=180000,wiops=105000";
      description = ''
        A hard per-second IO ceiling for every job, any subset of `rbps`,
        `wbps`, `riops` and `wiops` (--job-io-max). Null leaves the flag off
        the line and there is then no ceiling at all, only the IOWeight
        below, which shares contended bandwidth and caps nothing (D135).

        This is a promise about the filesystem under `dataRoot` and not a
        hint: `io.max` is per-device, so the runner resolves the device that
        path is on and **refuses to start** when it cannot write the ceiling
        there — a ZFS data root, no cgroup delegation, a path whose device it
        cannot resolve. Setting this on a host whose data root cannot carry
        it is a service that never comes up, by design: a ceiling nobody
        honours is a false promise.
      '';
    };

    hostMemory = lib.mkOption {
      type = lib.types.str;
      default = "20GiB";
      description = "What Herd's jobs may use on this host (--host-memory).";
    };

    hostMemoryFloor = lib.mkOption {
      type = lib.types.str;
      default = "6GiB";
      description = "What stays the host's whatever is queued (E-29).";
    };

    hostThreads = lib.mkOption {
      type = lib.types.int;
      default = 8;
      description = "Threads Herd may use, of the machine's total.";
    };

    vfio = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Give /dev/vfio/* the `vfio` group with a udev rule and put the runner
        in it. Off by default because those nodes are root:root 0600 on a
        stock kernel, which no supplementary group can reach — so this is the
        rule the copied unit's OWNER note asks for, declared.
      '';
    };

    jobPath = lib.mkOption {
      type = lib.types.str;
      default = "/run/wrappers/bin:/run/current-system/sw/bin";
      description = ''
        The service's PATH, which is also every job's PATH: the runner builds
        a job's environment from scratch and carries only PATH, LANG, LC_ALL,
        TZ and TERM through it. Those two directories are the NixOS answer;
        on a host with a different system profile, say so here. Setting it to
        NixOS's own service PATH is not an option worth having — it is
        coreutils, findutils, gnugrep, gnused and systemd, with no shell.

        /run/wrappers/bin comes first, and the order is the point. The
        setuid wrappers live only there — `sudo`, `su`, `mount`, `pkexec`,
        `newuidmap`, `fusermount` — and the system profile carries the same
        names as plain `r-xr-xr-x` binaries that fail at runtime for want of
        the bit. So a job whose PATH is /run/current-system/sw/bin alone
        cannot reach a working one at all, and a job whose PATH has the
        wrappers *after* the profile reaches the broken one first. A job's
        PATH is this string and nothing else, so what is missing or
        mis-ordered here is missing or mis-ordered for every job.
      '';
    };

    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "--capabilities /etc/herd/capabilities.json" ];
      description = ''
        Appended to ExecStart. --capabilities is the one that is deliberately
        absent by default: a file that does not exist refuses the startup, so
        it is added in the same move as writing the file.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    # D78, as far as a module can carry it. Nix *cannot* make membership of
    # `docker` impossible: users.users.<name>.extraGroups and
    # users.groups.docker.members are both list options, so another module —
    # or a later line of the same configuration — appending "docker" merges
    # instead of conflicting, and the evaluation succeeds. These two
    # assertions catch every declarative route, which is what is left to do;
    # neither can see a `usermod -aG docker herd` run by hand on the host.
    # What answers the question on a running machine is `id -nG herd`, which
    # must not print docker, and the runner's own startup line, which warns
    # naming the group (checkable from the journal).
    assertions = [
      {
        assertion = !(builtins.elem "docker" config.users.users.${cfg.user}.extraGroups);
        message = ''
          services.herd-runner: the user ${cfg.user} is in the "docker"
          group. A user that can write the Docker socket is root on its own
          host, and Herd never needs it (D78). Remove it, and check the
          running machine with `id -nG ${cfg.user}`.
        '';
      }
      {
        assertion =
          !(config.users.groups ? docker) || !(builtins.elem cfg.user config.users.groups.docker.members);
        message = ''
          services.herd-runner: users.groups.docker.members lists
          ${cfg.user}. Same thing from the other end (D78).
        '';
      }
    ];

    # `useradd --system herd`, declared. isSystemUser with an explicit group
    # is what NixOS requires of a user that is not a login account: no
    # password, and a uid in the system range below 1000.
    #
    # kvm, render and video are NOT given here on purpose: the unit grants
    # them with SupplementaryGroups, which scopes them to this service's
    # processes, where extraGroups would give them to anything this user ever
    # runs. Same grant in the unit as in the copied file, deliberately not
    # widened by moving it onto the user.
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
      description = "Herd runner";
    };
    # One attribute set and not two definitions of users.groups: a module's
    # config is an ordinary attrset, so `users.groups.x` beside
    # `users.groups = ...` is a duplicate-attribute error, not a merge.
    users.groups = {
      "${cfg.group}" = { };
    }
    // lib.optionalAttrs cfg.vfio { vfio = { }; };

    services.udev.extraRules = lib.mkIf cfg.vfio ''
      # /dev/vfio/* is root:root 0600 on a stock kernel; a supplementary
      # group only means something once a rule puts one on the node.
      SUBSYSTEM=="vfio", GROUP="vfio", MODE="0660"
    '';

    environment.systemPackages = lib.mkIf cfg.addToSystemPackages [ cfg.package ];

    environment.etc = lib.mkIf (cfg.documentation != null) {
      "herd/joining.md".source = cfg.documentation;
    };

    # What `StateDirectory=` used to do, and the reason the copied unit now
    # has to ask the operator for a command: the directory exists, owned by
    # the runner's user, mode 0700, before the service starts — and it is
    # re-asserted at every boot rather than once at install time, so the mode
    # cannot drift either. With the binary's store path, this is the sharpest
    # of the module's advantages over a copied file.
    #
    # Caveat, for the same reason RequiresMountsFor is below: tmpfiles runs
    # after local-fs.target, and /mnt/ssd_980 is mounted `nofail`, so a boot
    # with the NVMe missing would create this directory on the mount point
    # instead. The service still refuses to start — that is what
    # RequiresMountsFor buys — so the result is a stray empty directory and no
    # runner, not a runner silently writing to the pool.
    systemd.tmpfiles.settings."10-herd-runner".${cfg.dataRoot}.d = {
      user = cfg.user;
      group = cfg.group;
      mode = "0700";
    };

    systemd.services.herd-runner = {
      description = "Herd runner (native, unprivileged)";
      documentation = [ "file:///etc/herd/joining.md" ];

      # [Unit] After=/Wants=, and [Install] WantedBy=, which NixOS lifts out
      # of serviceConfig into attributes of their own. Same three lines.
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];

      # The data root is on a filesystem of its own and /mnt/ssd_980 is in
      # fstab with `nofail`, so local-fs.target is reached whether that mount
      # came up or not. Requiring the mount makes a missing disk a service
      # that does not start, rather than a runner on a bare mount point whose
      # filesystem is the ZFS pool, where no IO weight reaches (D120, D126).
      unitConfig.RequiresMountsFor = cfg.dataRoot;

      # A job's PATH is this service's PATH — the runner carries PATH, LANG,
      # LC_ALL, TZ and TERM into a job's otherwise built-from-scratch
      # environment and nothing else — and NixOS's own answer has no shell in
      # it: `path` is coreutils, findutils, gnugrep, gnused and systemd, so
      # `sh -c '...'` would fail to exec exactly as it did under the copied
      # unit's default. So the value is stated, the same string on both paths.
      #
      # mkForce rather than a definition, because NixOS always defines
      # environment.PATH itself (the `path` option's own `apply` appends those
      # five packages, so it is never empty and never undefined). Overriding
      # it is the only way to say an exact value, and an exact value is what
      # keeps this module and deploy/herd-runner.service from drifting. `path`
      # is left empty for the same reason: everything on PATH comes from the
      # one string.
      path = [ ];
      environment.PATH = lib.mkForce cfg.jobPath;

      serviceConfig = {
        Type = "simple";
        User = cfg.user;
        Group = cfg.group;

        # Group membership, not privilege: /dev/kvm is root:kvm 0660 and
        # /dev/dri/renderD128 is root:render 0660.
        SupplementaryGroups = [
          "kvm"
          "render"
          "video"
        ] ++ lib.optional cfg.vfio "vfio";

        # The leading `-` is the copied unit's, and means the same: start
        # without it so the refusal comes from the hub by name.
        EnvironmentFile = "-${cfg.tokenFile}";

        ExecStart = "${cfg.package}/bin/herd-runner ${lib.concatStringsSep " " arguments}";

        # cgroup v2 delegation, the point of the unit. NixOS renders this as
        # `Delegate=true`, which systemd parses as the same boolean as `yes`;
        # the effect — this unit's cgroup directory, cgroup.procs and
        # cgroup.subtree_control chowned to User= — is identical.
        Delegate = true;

        # A job hitting its memory.max is a cgroup OOM kill under this unit,
        # and the default OOMPolicy=stop would take the runner down with the
        # job the ceiling exists for.
        OOMPolicy = "continue";

        MemoryAccounting = true;
        MemoryMax = cfg.memoryMax;
        MemorySwapMax = 0;

        # Herd's whole subtree yields when the host is contended. IOWeight
        # bites only because the data root is on a block device whose cost
        # model blk-iocost knows; it is a share of contended bandwidth and
        # never a ceiling, and it covers nothing a job writes outside the data
        # root by absolute path (D120, D126). The one hard IO lever is
        # `io.max`, per job and per device: `jobIoMax` above (D135).
        CPUWeight = 50;
        IOWeight = 50;

        # A drain is the runner ending its own jobs and reporting it; the
        # default control-group kill mode would SIGTERM them behind its back.
        KillMode = "mixed";
        KillSignal = "SIGTERM";
        TimeoutStopSec = 10;

        # E-52: back after a crash, and back after a reboot.
        Restart = "always";
        RestartSec = "5s";

        # A build opens a lot of files.
        LimitNOFILE = 65536;

        # Deliberately not set, each for the reason written out in
        # deploy/herd-runner.service: NoNewPrivileges and
        # CapabilityBoundingSet would break E-46's privileged helpers, and
        # ProtectSystem, PrivateTmp, PrivateDevices and ProtectHome each build
        # a mount namespace, which is exactly what E-47 says this runner must
        # not have. NixOS adds none of them on its own, so "absent here" and
        # "absent there" are the same unit.
      };
    };
  };
}
