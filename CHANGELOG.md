# Changelog

## 0.2.0

- /etc/{group,passwd,shadow} are now sorted by GID/UID. This follows the
  behaviour of systemd-sysusers, update-users-groups.pl and generally what the
  shadow package does, most notably `pwck --sort`.
