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
          waybar-llm-bridge = pkgs.rustPlatform.buildRustPackage {
            pname = "waybar-llm-bridge";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [ pkgs.makeWrapper ];

            postInstall = ''
              wrapProgram $out/bin/waybar-llm-bridge \
                --run 'export LLM_BRIDGE_STATE_PATH=''${LLM_BRIDGE_STATE_PATH:-"/run/user/$(id -u)/llm_state.json"}' \
                --run 'export LLM_BRIDGE_SIGNAL=''${LLM_BRIDGE_SIGNAL:-"8"}' \
                --run 'export LLM_BRIDGE_TRANSCRIPT_DIR=''${LLM_BRIDGE_TRANSCRIPT_DIR:-"$HOME/.claude/projects"}'
            '';
          };

          default = self.packages.${system}.waybar-llm-bridge;
        });

      apps = forAllSystems (system:
        let pkgs = mkPkgs system;
        in {
          waybar-llm-bridge = {
            type = "app";
            program = "${self.packages.${system}.waybar-llm-bridge}/bin/waybar-llm-bridge";
          };
          default = self.apps.${system}.waybar-llm-bridge;

          # Demo runner
          demo = {
            type = "app";
            program = toString (pkgs.writeShellScript "llm-waybar-demo" ''
              export PATH="${self.packages.${system}.waybar-llm-bridge}/bin:$PATH"
              export DEMO_BIN="${self.packages.${system}.waybar-llm-bridge}/bin/waybar-llm-bridge"
              exec ${./demo/demo.sh} "$@"
            '');
          };
        });
    };
}
