# Userborn

Declaratively bear (manage) Linux users and groups.

## Features

- Create system (UID < 1000) and normal (UID >= 1000) users.
- Update user (password, description (gecos), home directory,
  shell) and group (members) information.
- Prohibit UID/GID re-use.
- Simple JSON config format.
- Create per-user groups if no explicit primary group is provided.
- Warn about insecure password hashing schemes.

### Where does it run?

It is undeniable that Userborn finds it's origin in NixOS. However, Userborn
has been designed to work on any distro. It is effectively distro-agnostic. It
will run on any Linux.

## Getting Started

To enable Userborn you need to import the module and enable the service:

```nix
services.userborn.enable = true;
```

### Flakes

```nix
# file: flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    userborn = {
      url = "github:nikstur/userborn";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, userborn, ...}: {
    nixosConfigurations = {
      yourHost = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";

        modules = [
          # This is not a complete NixOS configuration and you need to reference
          # your normal configuration here.

          userborn.nixosModules.userborn

          ({ ... }: {
            services.userborn.enable = true;
          })
        ];
      };
    };
  };
}
```


### Niv

```console
$ niv add nikstur/userborn
Adding package userborn
  Writing new sources file
Done: Adding package userborn
```

```nix
# file: configuration.nix
{ ... }:
let
    sources = import ./nix/sources.nix;
    lanzaboote = import sources.lanzaboote;
in
{
  # This is not a complete NixOS configuration and you need to reference
  # your normal configuration here.

  imports = [ lanzaboote.nixosModules.lanzaboote ];

  services.userborn.enable = true;
}
```

### Idempotence

- Never deletes a user or group, only disables them when they are not present
  in the config anymore.
- Never changes the UID of an existing user or the GID of an existing group.

This prohibits UID/GID re-use which is a security issue. The danger of UID/GID
re-use is best illustrated by an example. Imagine the following scenario:

- A new user with the UID 1000 is created. The user creates all sorts of files
  owned by them (via their UID).
- This user is deleted, their UID is freed up.
- A new user (with a differnent username) is created and get's allocated a new
  UID. The allocation algorithm doesn't know that previously a user with the
  UID 1000 existed so it allocates UID 1000 to the new user.
- This user can now access files from a previously existing user because their
  UIDs are the same.

Limitations:

- When you provide a plaintext password in the config (which you really
  shouldn't!), the hashed password is updated each time userborn runs. This can
  be fixed in the future by calling `crypt()` directly (and re-using the
  previos salt) instead of running `mkpasswd` in a subprocess. However, the
  security gains of this would be 0 (because the password is already available
  in plaintext!) and it will only suppress a single log line.
- Userborn can handle comments in the password database files but it will
  silently discard them.

## Comparison With Other Tools for Declarative User Management

### systemd-sysusers

Userborn follows the same spirit as systemd-sysusers and indeed can be viewed
as an adaptation of sysusers to a more specialized system where the service
takes full ownership of the user database (i.e. also changes certain fields of
entries).

Userborn has two key differences from systemd-sysusers:

1. Does not only create system users (UID < 1000) but also normal users. In the
   systemd world, "normal" users wouldn't have an entry in
   `/etc/{group,passwd,shadow}`. Userborn, however affords them one of these
   entries, not because the systemd way is wrong or bad but because this way is
   easier and fully backwards compatible.
2. Takes full ownership of the password database and thus also (destructively)
   changes user entries. For example, it can change passwords, home
   directories, default shell, etc. Please see the [Idempotence
   section](#Idempotence) for details of what Userborn can change and what it
   will never change.

### NixOS `update-users-groups.pl`

Userborn:

1. Doesn't use perl.
2. Runs as a systemd service, not as an activation script.
3. Doesn't rely on a hidden database to track state over the lifetime of a
   system.
4. Supports mounting `/etc` via an (immutable, read-only) overlay.

### Limitations

- Currently doesn't support group passwords (and thus also doesn't support `/etc/gshadow`).
