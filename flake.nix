{
  description = "Braille-dot progress bar for statuslines and terminals";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      forAllSystems = nixpkgs.lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          dotbar = pkgs.callPackage ./package.nix { };
          default = self.packages.${system}.dotbar;
        }
      );

      overlays.default = final: prev: {
        dotbar = final.callPackage ./package.nix { };
      };
    };
}
