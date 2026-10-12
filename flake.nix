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
    };
}
