{
  description = "A TUI WiFi manager for Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      forAllSystems = fn: nixpkgs.lib.genAttrs [ "x86_64-linux" ] (system:
        fn nixpkgs.legacyPackages.${system}
      );
    in {
      packages = forAllSystems (pkgs: rec {
        weefee = pkgs.rustPlatform.buildRustPackage {
          pname = "weefee";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.dbus pkgs.glib pkgs.networkmanager ];
        };
        default = weefee;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo rustc rust-analyzer rustfmt clippy
            pkg-config networkmanager glib dbus
          ];
          RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
        };
      });
    };
}
