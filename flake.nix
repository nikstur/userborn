{
  description = "Declaratively bear (manage) Linux users and groups";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };

    pre-commit = {
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
      nixpkgs,
      ...
    }:
    let
      eachSystem = nixpkgs.lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
      ];
    in
    {
      packages = eachSystem (
        system:
        (import ./nix/packages { pkgs = nixpkgs.legacyPackages.${system}; })
        // {
          default = self.packages.${system}.userborn;
        }
      );

      checks = eachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          overlayedPkgs = pkgs.extend (_final: _prev: { inherit (self.packages.${system}) userborn; });
        in
        {
          clippy = self.packages.${system}.userborn.overrideAttrs (
            _: previousAttrs: {
              pname = previousAttrs.pname + "-clippy";
              nativeCheckInputs = (previousAttrs.nativeCheckInputs or [ ]) ++ [ pkgs.clippy ];
              checkPhase = "cargo clippy";
            }
          );
          rustfmt = self.packages.${system}.userborn.overrideAttrs (
            _: previousAttrs: {
              pname = previousAttrs.pname + "-rustfmt";
              nativeCheckInputs = (previousAttrs.nativeCheckInputs or [ ]) ++ [ pkgs.rustfmt ];
              checkPhase = "cargo fmt --check";
            }
          );
          pre-commit = inputs.pre-commit.lib.${system}.run {
            src = ./.;
            hooks = {
              nixfmt.enable = true;
              deadnix.enable = true;
              statix.enable = true;
            };
          };
          inherit (overlayedPkgs.nixosTests)
            userborn
            userborn-mutable-users
            userborn-mutable-etc
            userborn-immutable-users
            userborn-immutable-etc
            ;
        }
      );

      devShells = eachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            shellHook = ''
              ${self.checks.${system}.pre-commit.shellHook}
            '';

            packages = [
              pkgs.nixfmt
              pkgs.clippy
              pkgs.rustfmt
              pkgs.cargo-machete
              pkgs.cargo-edit
              pkgs.cargo-bloat
              pkgs.cargo-deny
              pkgs.cargo-cyclonedx
              pkgs.cargo-flamegraph
              pkgs.hyperfine
            ];

            inputsFrom = [ self.packages.${system}.userborn ];

            RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
          };
        }
      );

    };
}
