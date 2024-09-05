{
  description = "Declaratively bear (manage) Linux users and groups";

  inputs = {

    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    systems.url = "github:nix-systems/default";

    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };

    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };

    pre-commit-hooks-nix = {
      url = "github:cachix/pre-commit-hooks.nix";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-compat.follows = "flake-compat";
      };
    };

  };

  outputs =
    inputs@{
      self,
      flake-parts,
      systems,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = import systems;

      imports = [ inputs.pre-commit-hooks-nix.flakeModule ];

      perSystem =
        {
          config,
          system,
          pkgs,
          lib,
          ...
        }:
        {
          packages = {
            userborn = pkgs.userborn.overrideAttrs {
              src = lib.sourceFilesBySuffices ./rust/userborn [
                ".rs"
                ".toml"
                ".lock"
              ];
              sourceRoot = null;
            };
            default = config.packages.userborn;
          };

          checks =
            let
              overlayedPkgs = pkgs.extend (final: prev: { inherit (config.packages) userborn; });
            in
            {
              clippy = config.packages.userborn.overrideAttrs (
                _: previousAttrs: {
                  pname = previousAttrs.pname + "-clippy";
                  nativeCheckInputs = (previousAttrs.nativeCheckInputs or [ ]) ++ [ pkgs.clippy ];
                  checkPhase = "cargo clippy";
                }
              );
              rustfmt = config.packages.userborn.overrideAttrs (
                _: previousAttrs: {
                  pname = previousAttrs.pname + "-rustfmt";
                  nativeCheckInputs = (previousAttrs.nativeCheckInputs or [ ]) ++ [ pkgs.rustfmt ];
                  checkPhase = "cargo fmt --check";
                }
              );
              inherit (overlayedPkgs.nixosTests)
                userborn
                userborn-mutable-users
                userborn-mutable-etc
                userborn-immutable-users
                userborn-immutable-etc
                ;
            };

          pre-commit = {
            check.enable = true;

            settings = {
              hooks = {
                nixfmt-rfc-style.enable = true;
                statix.enable = true;
              };
            };
          };

          devShells.default = pkgs.mkShell {
            shellHook = ''
              ${config.pre-commit.installationScript}
            '';

            packages = [
              pkgs.niv
              pkgs.nixfmt-rfc-style
              pkgs.clippy
              pkgs.rustfmt
              pkgs.cargo-machete
              pkgs.cargo-edit
              pkgs.cargo-bloat
              pkgs.cargo-deny
              pkgs.cargo-cyclonedx
            ];

            inputsFrom = [ config.packages.userborn ];

            RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
          };

        };
    };
}
