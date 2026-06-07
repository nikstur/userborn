# Changelog

## 0.6.0 (Unreleased)

- Removed the ability to configure the path to the `nologin` binary during
  runtime via the env variable `USERBORN_NO_LOGIN_PATH`. Instead, configure the
  path during compilation of Userborn via `USERBORN_NO_LOGIN_DEFAULT_PATH`.
- The file mode, uid, and gid of /etc/{group,passwd,shadow} are now retained if
  the files already exist. If the files don't exist, they are created with the
  same permissions as before. This enables setups where `/etc/shadow` is owned
  by the `shadow` group with mode `640` and `unix_chkpwd` only has the `setgid`
  bit. Most notably, this is the default on Debian/Ubuntu derivatives.
- Userborn now manages `/etc/subuid` and `/etc/subgid`. Per-user explicit
  ranges (`subUidRanges`, `subGidRanges`) are written verbatim, and
  `autoSubIdRange` allocates a stable, non-overlapping range that is
  preserved across generations. Like UIDs and GIDs, existing subordinate id
  entries are never removed so a range cannot be reassigned to a different
  owner. Cross-owner overlap is logged as a warning, or refused outright
  when `strictSubIdOverlap` is set in the config.

## 0.5.0

- Groups that were removed from the config are now emptied (all their users are
  removed from it). This makes the behaviour consistent with the way we treat
  users. They're never removed (to avoid GID re-use) but effectively disabled.
- Mutable users are now fully supported. Previously, Userborn would disable
  all users and drain all groups that were not in it's current config. Now, if
  mutable users are enabled via `USERBORN_MUTABLE_USERS`, only users/groups
  that were in the previous Userborn config are disabled/drained.

## 0.4.0

- Group memberships in /etc/users are now forcibly unique and alphabetically
  sorted, even if they weren't sorted in the config.
- Update xcrypt to 0.3.1. Now Userborn supports 32 bit.

## 0.3.0

- Userborn now calls `libxcrypt` directly via the `xcrypt` crate instead of
  shelling out to `mkpasswd`. This enables us to not change the password hash
  when a plaintext password is provided. We now check whether the password from
  the config matches the hashed password and then re-use the salt instead of
  generating a new salt. Please note that this changes nothing about the
  security posture of Userborn. If you provide a plaintext password to
  Userborn, there is nothing Userborn can do to protect it from leaking.
- You can now configure the path to the `nologin` binary via the compile-time
  environment variable `USERBORN_NO_LOGIN_DEFAULT_PATH` and the runtime
  variable `USERBORN_NO_LOGIN_PATH`. These values are used when no explicit
  shell is provided in the user config.

## 0.2.0

- /etc/{group,passwd,shadow} are now sorted by GID/UID. This follows the
  behaviour of systemd-sysusers, update-users-groups.pl and generally what the
  shadow package does, most notably `pwck --sort`.
