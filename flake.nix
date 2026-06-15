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
        qtBuildInputs = with pkgs.qt6; [ qtbase qtdeclarative qt5compat ];
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

          fdf-app = pkgs.rustPlatform.buildRustPackage {
            pname = "fdf-app";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = with pkgs; [ pkg-config qt6.wrapQtAppsHook ];
            buildInputs = qtBuildInputs;
            doCheck = false;
            buildFeatures = [ "qt" ];
            postInstall = ''
              mv $out/bin/bridge $out/bin/fdf-app
              wrapQtApp $out/bin/fdf-app
            '';
          };

          default = self.packages.${system}.bridge;
        };
      });
}
