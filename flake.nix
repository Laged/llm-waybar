{
  description = "llm-waybar development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "https://flakehub.com/f/nix-community/fenix/0.1";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      mkPkgs = system: import nixpkgs {
        inherit system;
        overlays = [ self.overlays.default ];
      };
    in {
      overlays.default = final: prev: {
        rustToolchain = with fenix.packages.${prev.stdenv.hostPlatform.system};
          combine (with stable; [ rustc cargo clippy rustfmt rust-src ]);
      };

      devShells = forAllSystems (system:
        let pkgs = mkPkgs system;
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustToolchain
              pkg-config
              openssl
              rust-analyzer
            ];

            env = {
              RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
            };
          };
        });

      packages = forAllSystems (system:
        let pkgs = mkPkgs system;
        in {
          hello = pkgs.writeShellScriptBin "hello" ''
            echo "Hello from llm-waybar!"
          '';
          default = self.packages.${system}.hello;
        });

      apps = forAllSystems (system: {
        hello = {
          type = "app";
          program = "${self.packages.${system}.hello}/bin/hello";
        };
        default = self.apps.${system}.hello;
      });
    };
}
