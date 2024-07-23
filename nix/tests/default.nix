{ pkgs, extraBaseModules }:

let
  runTest = module: pkgs.testers.runNixOSTest {
    imports = [ module ];
    globalTimeout = 5 * 60;
    extraBaseModules = {
      imports = builtins.attrValues extraBaseModules;
    };
  };
in
{
  # Lanzaboote
  userborn-mutable-users = runTest ./userborn-mutable-users.nix;
  userborn-immutable-users = runTest ./userborn-immutable-users.nix;
}
