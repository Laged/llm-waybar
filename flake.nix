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
                --run 'export LLM_BRIDGE_STATE_PATH=''${LLM_BRIDGE_STATE_PATH:-"''${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/llm_state.json"}' \
                --run 'export LLM_BRIDGE_SOCKET_PATH=''${LLM_BRIDGE_SOCKET_PATH:-"''${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/llm-bridge.sock"}' \
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
              # Copy demo scripts to temp directory to preserve relative paths
              DEMO_TMP=$(mktemp -d)
              trap "rm -rf $DEMO_TMP" EXIT
              cp -r ${./demo}/* "$DEMO_TMP/"
              chmod +x "$DEMO_TMP"/*.sh
              chmod +x "$DEMO_TMP"/scenarios/*.sh
              exec "$DEMO_TMP/demo.sh" "$@"
            '');
          };
        });

      checks = forAllSystems (system:
        let pkgs = mkPkgs system;
        in {
          cargo-test = pkgs.rustPlatform.buildRustPackage {
            pname = "waybar-llm-bridge-test";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            checkPhase = ''cargo test --release'';
            installPhase = "touch $out";
          };

          integration-test = pkgs.runCommand "integration-test" {
            buildInputs = [ self.packages.${system}.waybar-llm-bridge pkgs.jq pkgs.bash ];
          } ''
            export HOME=$(mktemp -d)
            export XDG_RUNTIME_DIR=$(mktemp -d)
            export LLM_BRIDGE_STATE_PATH="$XDG_RUNTIME_DIR/llm_state.json"
            export LLM_BRIDGE_SESSIONS_DIR="$XDG_RUNTIME_DIR/llm_sessions"
            export DEMO_BIN="${self.packages.${system}.waybar-llm-bridge}/bin/waybar-llm-bridge"
            ${pkgs.bash}/bin/bash ${./test-hooks.sh}
            touch $out
          '';
        });

      homeManagerModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.llm-bridge;
        in {
          options.services.llm-bridge = {
            enable = lib.mkEnableOption "LLM Waybar Bridge daemon";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.waybar-llm-bridge;
              description = "The waybar-llm-bridge package to use";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.user.services.llm-bridge = {
              Unit = {
                Description = "LLM Waybar Bridge Daemon";
                After = [ "graphical-session.target" ];
                PartOf = [ "graphical-session.target" ];
              };
              Service = {
                ExecStart = "${cfg.package}/bin/waybar-llm-bridge daemon";
                Restart = "on-failure";
                RestartSec = 1;
              };
              Install.WantedBy = [ "graphical-session.target" ];
            };

            home.packages = [ cfg.package ];
          };
        };
    };
}
