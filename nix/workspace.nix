# The whole workspace, built once in release: `herd-hub`, `herd-runner` and
# `herdctl` in one derivation (M16).
#
# Why one derivation and not three. The three binaries share `herd-proto` and
# the same dependency tree — tokio, tungstenite, clap. Three
# `buildRustPackage` calls, one per `-p`, would compile that tree three times
# for the same bytes. The flake splits this output into the three packages a
# caller asks for instead, so each `packages.<system>.<binary>` is one binary
# with its own closure, and the build behind them happens once.
#
# nixpkgs' stable rust, no overlay: D12, which packaging does not change.
{
  lib,
  rustPlatform,
  version,
  src,
}:
rustPlatform.buildRustPackage {
  pname = "herd";
  inherit version src;

  # Vendored from the lock file we already commit, rather than a `cargoHash`
  # to keep in step with it by hand. No new flake input: no crane, no naersk
  # (D12's reasoning — an input is a thing to trust and to update).
  cargoLock.lockFile = ../Cargo.lock;

  # The suite is `./scripts/test.sh` in the dev shell, and that is deliberate
  # rather than an omission. Two reasons. The tests spawn the real binaries,
  # signal them and bind loopback sockets, and some of them need cgroup v2
  # delegation the build sandbox does not have — which D27 answers with a
  # skip log printed *after* `cargo test`, something a derivation that only
  # succeeds or fails cannot report. And a release-mode rebuild of every test
  # binary doubles the cost of a package whose job is to ship three
  # executables. `nix flake check` therefore proves these build; the suite
  # proves they work.
  doCheck = false;

  meta = {
    description = "Herd: distributed build and test execution — hub, runner and requester CLI";
    platforms = lib.platforms.linux;
  };
}
