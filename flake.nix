{
  description = "Local-first org-mode tools for terminals and coding agents";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    git-hooks-nix = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {
    flake-parts,
    nixpkgs,
    rust-overlay,
    git-hooks-nix,
    treefmt-nix,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      imports = [
        git-hooks-nix.flakeModule
        treefmt-nix.flakeModule
      ];

      perSystem = {
        config,
        system,
        ...
      }: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-analyzer"
            "rust-src"
            "rustfmt"
            "clippy"
            "llvm-tools-preview"
          ];
        };
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
        crateVersion = manifest:
          (builtins.fromTOML (builtins.readFile manifest)).package.version;
        mkRustPackage = {
          pname,
          manifest,
          description,
        }:
          rustPlatform.buildRustPackage {
            inherit pname;
            version = crateVersion manifest;
            src = pkgs.lib.cleanSource ./.;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = ["--package=${pname}"];
            cargoTestFlags = ["--package=${pname}"];
            cargoInstallFlags = ["--package=${pname}"];

            meta = {
              inherit description;
              homepage = "https://github.com/johnnybravo-xyz/agd";
              license = pkgs.lib.licenses.gpl3Plus;
              mainProgram = pname;
              platforms = pkgs.lib.platforms.unix;
            };
          };
      in {
        packages = rec {
          agd = mkRustPackage {
            pname = "agd";
            manifest = ./agd/Cargo.toml;
            description = "Local-first command-line tools for org-mode files";
          };
          org-mcp-server = mkRustPackage {
            pname = "org-mcp-server";
            manifest = ./org-mcp-server/Cargo.toml;
            description = "Stdio MCP adapter for org-mode files";
          };
          default = agd;
        };

        pre-commit = {
          check.enable = true;
          settings = {
            src = ./.;
            hooks = {
              alejandra.enable = true;
              rustfmt.enable = true;
              deadnix.enable = true;
              statix.enable = true;
            };
          };
        };

        treefmt = {
          projectRootFile = "flake.nix";
          programs = {
            alejandra.enable = true;
            rustfmt.enable = true;
          };
        };

        devShells.default = pkgs.mkShell {
          shellHook = config.pre-commit.installationScript;
          packages = with pkgs; [
            alejandra
            cargo-deny
            cargo-edit
            cargo-llvm-cov
            cargo-machete
            cargo-nextest
            deadnix
            git
            just
            rustToolchain
            statix
          ];
          RUST_BACKTRACE = "1";
        };
      };
    };
}
