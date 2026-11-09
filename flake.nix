{
  inputs.nixpkgs.url = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.zst";

  outputs = {
    self,
    nixpkgs,
  }: let
    systems = ["x86_64-linux" "aarch64-linux" "aarch64-darwin"];
    forEachSystem = nixpkgs.lib.genAttrs systems;
    pkgsForEach = nixpkgs.legacyPackages;
  in {
    nixosModules = {
      eh = import ./nix/modules/nixos.nix self;
      default = self.nixosModules.eh;
    };

    packages = forEachSystem (system: {
      eh = pkgsForEach.${system}.callPackage ./nix/package.nix {};
      default = self.packages.${system}.eh;
    });

    devShells = forEachSystem (system: {
      default = pkgsForEach.${system}.callPackage ./nix/shell.nix {};
    });

    hydraJobs = self.packages;
  };
}
