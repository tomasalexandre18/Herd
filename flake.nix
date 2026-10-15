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

          default = herd-hub;
        }
      );
    };
}
