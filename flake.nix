{
  description = "FDF Bridge - Transpiler & Qt Runtime for FDF Applications";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        packages = {
          bridge = pkgs.rustPlatform.buildRustPackage {
            pname = "bridge";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = with pkgs; [ pkg-config ];
            buildInputs = [ ];
            doCheck = false;
          };

          default = self.packages.${system}.bridge;
        };

        devShells = {
          default = pkgs.mkShell {
            name = "bridge-dev";
            nativeBuildInputs = with pkgs; [
              pkg-config cmake ninja rustc cargo
            ];
            buildInputs = with pkgs.qt6; [
              qtbase qtdeclarative
            ];
          };
        };
      });
}
