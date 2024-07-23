{ lib, ... }:

let
  rootPassword = "$y$j9T$p6OI0WN7.rSfZBOijjRdR.$xUOA2MTcB48ac.9Oc5fz8cxwLv1mMqabnn333iOzSA6";
  updatedRootPassword = "$y$j9T$G0/cN658V3USk9E/J/rC1.$p4jFnkCPTOIiieAAHh5uYX4NebE2Cl6Bh5N1I3mLnNC";
  normaloPassword = "hello";
  newNormaloPassword = "$y$j9T$p6OI0WN7.rSfZBOijjRdR.$xUOA2MTcB48ac.9Oc5fz8cxwLv1mMqabnn333iOzSA6";
in

{

  name = "userborn-mutable-users";

  meta.maintainers = with lib.maintainers; [ nikstur ];

  nodes.machine = { pkgs, ... }: {
    services.userborn.enable = true;

    # Prerequisites
    system.etc.overlay.enable = true;
    boot.initrd.systemd.enable = true;

    # Read this password file at runtime from outside the Nix store.
    environment.etc."rootpw.secret".text = rootPassword;

    users = {
      mutableUsers = true;
      users = {
        # Override the empty root password set by the test instrumentation.
        root.hashedPasswordFile = lib.mkForce "/etc/rootpw.secret";

        normalo = {
          isNormalUser = true;
          initialPassword = normaloPassword;
        };
      };
    };

    specialisation.new-generation.configuration = {
      users.users = {
        root = {
          # Forcing this to null simulates removing the config value in a new
          # generation.
          hashedPasswordFile = lib.mkOverride 9 null;
          hashedPassword = updatedRootPassword;
        };
        new-normalo = {
          isNormalUser = true;
          initialHashedPassword = newNormaloPassword;
        };
      };
    };
  };

  testScript = ''
    machine.wait_for_unit("userborn.service")

    with subtest("Correct mode on the password files"):
      assert machine.succeed("stat -c '%a' /etc/passwd") == "644\n"
      assert machine.succeed("stat -c '%a' /etc/group") == "644\n"
      assert machine.succeed("stat -c '%a' /etc/shadow") == "0\n"

    with subtest("root user has correct password"):
      print(machine.succeed("getent passwd root"))
      assert "${rootPassword}" in machine.succeed("getent shadow root"), "root user password is not correct"

    with subtest("normalo user is created"):
      print(machine.succeed("getent passwd normalo"))
      assert machine.succeed("stat -c '%U' /home/normalo") == "normalo\n"


    machine.succeed("/run/current-system/specialisation/new-generation/bin/switch-to-configuration switch")


    with subtest("root user password is updated"):
      print(machine.succeed("getent passwd root"))
      assert "${updatedRootPassword}" in machine.succeed("getent shadow root"), "root user password is not updated"

    with subtest("new-normalo user is created after switching to new generation"):
      print(machine.succeed("getent passwd new-normalo"))
      assert machine.succeed("stat -c '%U' /home/new-normalo") == "new-normalo\n"
      assert "${newNormaloPassword}" in machine.succeed("getent shadow new-normalo"), "new-normalo user password is not correct"
  '';
}
