{ pkgs }:

{
  userborn = pkgs.callPackage ./userborn.nix { };
  static = pkgs.pkgsStatic.callPackage ./userborn.nix { };
  cross = pkgs.pkgsCross.aarch64-multiplatform.callPackage ./userborn.nix { };
}
