use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{bail, Context, Result};

use crate::{fs::atomic_write, id};

/// Password for /etc/passwd indicating that the actual password is stored in /etc/shadow.
const PASSWORD_IN_SHADOW: &str = "x";

#[derive(Clone)]
pub struct Entry {
    name: String,
    password: String,
    uid: u32,
    gid: u32,
    gecos: String,
    directory: String,
    shell: String,
}

impl Entry {
    /// Create a new /etc/passwd entry.
    ///
    /// The password is always set to `x` because the actual password hash is stored in
    /// /etc/shadow.
    pub fn new(
        name: String,
        uid: u32,
        gid: u32,
        gecos: String,
        directory: String,
        shell: String,
    ) -> Self {
        Self {
            name,
            password: PASSWORD_IN_SHADOW.into(),
            uid,
            gid,
            gecos,
            directory,
            shell,
        }
    }

    /// Update an /etc/passwd entry.
    pub fn update(
        &mut self,
        gid: Option<u32>,
        gecos: Option<String>,
        directory: Option<String>,
        shell: Option<String>,
    ) {
        if let Some(gid) = gid {
            if self.gid != gid {
                log::info!(
                    "Updating primary group (GID) of user {} from {} to {gid}...",
                    self.name,
                    self.gid,
                );
                self.gid = gid;
            };
        }
        if let Some(gecos) = gecos {
            if self.gecos != gecos {
                log::info!(
                    "Updating gecos of user {} from {} to {gecos}...",
                    self.name,
                    self.gecos,
                );
                self.gecos = gecos;
            };
        }
        if let Some(directory) = directory {
            if self.directory != directory {
                log::info!(
                    "Updating home directory of user {} from {} to {directory}...",
                    self.name,
                    self.directory,
                );
                self.directory = directory;
            }
        }
        if let Some(shell) = shell {
            if self.shell != shell {
                log::info!(
                    "Updating shell of user {} from {} to {shell}...",
                    self.name,
                    self.shell,
                );
                self.shell = shell;
            };
        }
    }

    /// Read an entry from a single line from /etc/shadow.
    ///
    /// Whenever a field in this line doesn't exist or cannot be parsed, returns `None`.
    fn from_line(line: &str) -> Option<Self> {
        if line.starts_with('#') {
            return None;
        }
        let mut fields = line.splitn(7, ':');
        Some(Self {
            name: fields.next()?.into(),
            password: fields.next()?.into(),
            uid: fields.next()?.parse::<u32>().ok()?,
            gid: fields.next()?.parse::<u32>().ok()?,
            gecos: fields.next()?.into(),
            directory: fields.next()?.into(),
            shell: fields.next()?.into(),
        })
    }

    fn to_line(&self) -> String {
        [
            self.name.as_str(),
            self.password.as_str(),
            self.uid.to_string().as_str(),
            self.gid.to_string().as_str(),
            self.gecos.as_str(),
            self.directory.as_str(),
            self.shell.as_str(),
        ]
        .join(":")
    }

    /// Describe the entry in a human readable way.
    pub fn describe(&self) -> String {
        format!("{} with UID {}", self.name, self.uid)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Default)]
pub struct Passwd {
    /// Entries of /etc/passwd keyed by UID.
    entries: BTreeMap<u32, Entry>,
    /// Mapping of names to UIDs.
    uids: BTreeMap<String, u32>,
}

impl Passwd {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let file = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read {:?}.", path.as_ref()))?;

        Ok(Self::from_buffer(&file))
    }

    pub fn from_buffer(s: &str) -> Self {
        let mut entries = BTreeMap::new();
        let mut uids = BTreeMap::new();
        for line in s.lines() {
            if let Some(e) = Entry::from_line(line) {
                entries.insert(e.uid, e.clone());
                uids.insert(e.name.clone(), e.uid);
            } else {
                log::warn!("Skipping passwd line because it cannot be parsed: {line}.");
            }
        }
        Self { entries, uids }
    }

    pub fn to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        atomic_write(path, self.to_buffer(), 0o644)
    }

    pub fn to_buffer(&self) -> String {
        let mut s = String::new();
        for entry in self.entries.values() {
            s.push_str(&entry.to_line());
            s.push('\n');
        }
        s
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Entry> {
        let uid = self.uids.get(name);
        uid.and_then(|uid| self.entries.get_mut(uid))
    }

    /// Insert a new entry.
    ///
    /// This will fail if a user with the UID or name already exists.
    pub fn insert(&mut self, entry: &Entry) -> Result<()> {
        if self.entries.contains_key(&entry.uid) {
            bail!("User with UID {} already exists", entry.uid);
        }

        if self.uids.contains_key(&entry.name) {
            bail!("User {} already exists", entry.name);
        }

        self.entries.entry(entry.uid).or_insert(entry.clone());
        self.uids.insert(entry.name.clone(), entry.uid);

        Ok(())
    }

    /// Allocate a new (i.e. unused) UID.
    ///
    /// Returns `Err` if it cannot allocate a new UID because all in the range are already used.
    pub fn allocate_uid(&self, is_normal: bool) -> Result<u32> {
        let allocated_uids = self.entries.keys().copied().collect::<BTreeSet<u32>>();
        id::allocate(&allocated_uids, is_normal)
    }

    pub fn entries(&self) -> Vec<&Entry> {
        self.entries.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use expect_test::expect;
    use indoc::indoc;

    #[test]
    fn sort() {
        let buffer = indoc! {"
            fwupd-refresh:x:999:999::/var/empty:/run/current-system/sw/bin/nologin
            root:x:0:0:System administrator:/root:/run/current-system/sw/bin/bash
            nobody:x:65534:65534:Unprivileged account (don't use!):/var/empty:/run/current-system/sw/bin/nologin
            gary:x:1000:1000:Gary ,,,:/home/gary:/bin/bash
            messagebus:x:4:4:D-Bus system message bus daemon user:/run/dbus:/run/current-system/sw/bin/nologin
        "};
        let passwd = Passwd::from_buffer(buffer);
        let recreated_buffer = passwd.to_buffer();

        let expected = expect![[r#"
            root:x:0:0:System administrator:/root:/run/current-system/sw/bin/bash
            messagebus:x:4:4:D-Bus system message bus daemon user:/run/dbus:/run/current-system/sw/bin/nologin
            fwupd-refresh:x:999:999::/var/empty:/run/current-system/sw/bin/nologin
            gary:x:1000:1000:Gary ,,,:/home/gary:/bin/bash
            nobody:x:65534:65534:Unprivileged account (don't use!):/var/empty:/run/current-system/sw/bin/nologin
        "#]];
        expected.assert_eq(&recreated_buffer);
    }

    #[test]
    fn skip_comments_and_broken_lines() {
        let buffer = indoc! {"
            :fwupd-refresh:x:999:999::/var/empty:/run/current-system/sw/bin/nologin
            nobody:x:65534:65534:Unprivileged account (don't use!):/var/empty:/run/current-system/sw/bin/nologin
            # Comment
        "};
        let group = Passwd::from_buffer(buffer);
        let recreated_buffer = group.to_buffer();

        let expected = expect![[r"
            nobody:x:65534:65534:Unprivileged account (don't use!):/var/empty:/run/current-system/sw/bin/nologin
        "]];
        expected.assert_eq(&recreated_buffer);
    }
}
