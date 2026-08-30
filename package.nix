# Standalone derivation, callPackage-able from nixpkgs or the flake:
#   pkgs.callPackage ./package.nix { }
{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "dotbar";
  version = (lib.importTOML ./Cargo.toml).package.version;

  src = lib.fileset.toSource {
    root = ./.;
    # Only what the build actually reads, so doc/demo/packaging churn
    # doesn't invalidate the store hash.
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./tests
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;

  meta = {
    description = "Braille-dot progress bar for statuslines and terminals";
    homepage = "https://github.com/tlehman/dotbar";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "dotbar";
  };
}
