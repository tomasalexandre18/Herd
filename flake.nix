{
  description = "Herd - distributed build/test execution for the hub, runners and herdctl";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems =
        f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      inherit (nixpkgs) lib;

      # Read once, from the one place that already states it: the workspace.
      # Every package and the image's tag take it from here, so a release
      # cannot ship an image tagged with a version the binary inside it does
      # not answer to.
      version = (lib.importTOML ./Cargo.toml).workspace.package.version;

      # What a build of the binaries actually needs. Narrowed so that editing
      # `docs/` or this comment does not rebuild Rust: the crates, the
      # workspace manifest and the lock, and nothing else.
      src = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          ./Cargo.toml
          ./Cargo.lock
          ./herd-proto
          ./herd-hub
          ./herd-runner
          ./herdctl
        ];
      };
    in
    {
      # Every tool used to build, test and lint Herd comes from here; nothing is
      # installed globally (CLAUDE.md, D1). Rust is nixpkgs' stable toolchain.
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          name = "herd-dev";
          packages = [
            pkgs.rustc
            pkgs.cargo
            pkgs.clippy
            pkgs.rustfmt
          ];
          env.RUST_BACKTRACE = "1";
        };

        # `skopeo`, which copies the archive `packages.hub-image` builds
        # straight into a registry without a docker daemon in the middle.
        # The release workflow no longer uses it: since D162 the image ships
        # as an archive attached to the GitHub release and nothing is pushed
        # anywhere. This shell is what is left of that path, and it is the
        # whole of it — whoever wants the image in a registry of their own
        # runs one command against the archive they downloaded:
        #
        #   nix develop .#release --command skopeo --insecure-policy copy \
        #     docker-archive:herd-hub-image-<version>.tar.gz \
        #     docker://<registry>/herd-hub:<version>
        #
        # Deliberately a command a person runs and not a step in the
        # workflow: a publication that is not in the release path cannot
        # fail a release, which is the one lesson the three tags D162 names
        # paid for. `--insecure-policy` means "consult no containers
        # signature policy", which is what `docker push` also does; it says
        # nothing about transport, and TLS is untouched.
        #
        # It comes from the flake and not from `apt` for the reason D12
        # gives about every other tool: pinned nixpkgs. It is not in the
        # default shell, because nobody developing Herd needs it.
        release = pkgs.mkShell {
          name = "herd-release";
          packages = [ pkgs.skopeo ];
        };
      });

      packages = forAllSystems (
        pkgs:
        let
          # The three binaries, compiled once together: see nix/workspace.nix
          # for why that is one derivation and not three.
          workspace = pkgs.callPackage ./nix/workspace.nix { inherit version src; };

          # One binary, on its own, with its own closure. A copy and not a
          # symlink: a link into the workspace output would pull all three
          # binaries into anything that depends on one of them — the hub's
          # image above all.
          binary =
            name:
            pkgs.runCommand "${name}-${version}"
              {
                meta = workspace.meta // {
                  mainProgram = name;
                };
                passthru = {
                  inherit workspace version;
                };
              }
              ''
                mkdir -p "$out/bin"
                cp ${workspace}/bin/${name} "$out/bin/${name}"
              '';

          herd-hub = binary "herd-hub";
        in
        {
          inherit herd-hub;
          herd-runner = binary "herd-runner";
          herdctl = binary "herdctl";

          # `herd-hub:<version>`, which is how the deployment names it.
          hub-image = pkgs.callPackage ./nix/hub-image.nix {
            hub = herd-hub;
            inherit version;
          };

          # The release artefacts: the same three binaries, linked statically
          # against musl so that they run on a machine that has never seen
          # Nix. The packages above are linked against glibc out of the Nix
          # store — `ldd herd-runner` names
          # `/nix/store/…-glibc-2.42-67/lib64/ld-linux-x86-64.so.2` — so they
          # cannot be copied onto the Arch box or the WSL2 PC that are joining
          # the herd, which is what a release has to be able to do.
          #
          # `pkgsStatic` is nixpkgs' own musl cross set, so the toolchain is
          # still the one the flake pins (D12) and no second toolchain is
          # installed to cut a release. It costs a download and no extra
          # build: the musl `rustc`, `cargo`, `gcc` and `binutils` are all in
          # the public cache — 141 MiB, measured — and only this derivation is
          # compiled.
          #
          # All three binaries, including the hub, which ships as a container
          # image today (nix/hub-image.nix): the compile is the same one, so a
          # static hub for whoever wants to run it outside Docker is free.
          # One derivation and not three for the reason nix/workspace.nix
          # gives.
          #
          # `.version` is read off this derivation by
          # `.github/workflows/release.yml` to name the artefacts, so the
          # version in a release's file names comes from `Cargo.toml`'s
          # `workspace.package.version` by the same route as the image's tag
          # and can never be a figure somebody typed.
          static = pkgs.pkgsStatic.callPackage ./nix/workspace.nix {
            inherit version src;
          };

          default = herd-hub;
        }
      );
    };
}
