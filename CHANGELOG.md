# Changelog

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
