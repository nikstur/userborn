use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{bail, Context, Result};
use indexmap::IndexMap;

use crate::{fs::atomic_write, id::allocate_id};

#[derive(Clone)]
pub struct Entry {
    name: String,
    password: String,
    gid: u32,
    user_list: Vec<String>,
}

impl Entry {
    /// Create a new /etc/group entry.
    pub fn new(name: String, gid: u32, user_list: Vec<String>) -> Self {
        Self {
            name,
            password: "x".into(),
            gid,
            user_list,
        }
    }

    /// Update an /etc/group entry.
    pub fn update(&mut self, user_list: Vec<String>) {
        if self.user_list != user_list {
            log::info!(
                "Updating members of group {} from {:?} to {user_list:?}...",
                self.name,
                self.user_list,
            );
            self.user_list = user_list;
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
            gid: fields.next()?.parse().ok()?,
            user_list: split_group_members(fields.next()?),
        })
    }

    fn to_line(&self) -> String {
        [
            self.name.as_str(),
            self.password.as_str(),
            self.gid.to_string().as_str(),
            join_group_members(&self.user_list).as_str(),
        ]
        .join(":")
    }

    /// Describe the entry in a human readable way.
    pub fn describe(&self) -> String {
        format!("{} with GID {}", self.name, self.gid)
    }

    pub fn gid(&self) -> u32 {
        self.gid
    }
}

/// Split a string containing group members separated by `,` into a list.
fn split_group_members(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(',').map(ToString::to_string).collect()
}

/// Join a list of group members into a string separating each group name with a `,`.
fn join_group_members(v: &[String]) -> String {
    v.join(",")
}

pub struct Group {
    /// Entries of /etc/group keyed by group name.
    entries: IndexMap<String, Entry>,
    /// Already allocated GIDs.
    gids: BTreeSet<u32>,
}

impl Group {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let file = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read {:?}.", path.as_ref()))?;

        Ok(Self::from_buffer(&file))
    }

    fn from_buffer(s: &str) -> Self {
        let mut entries = IndexMap::new();
        let mut gids = BTreeSet::new();
        for line in s.lines() {
            if let Some(e) = Entry::from_line(line) {
                gids.insert(e.gid);
                entries.insert(e.name.clone(), e.clone());
            } else {
                log::warn!("Skipping group line because it cannot be parsed: {line}.")
            }
        }
        Self { entries, gids }
    }

    pub fn to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        atomic_write(path, self.to_buffer(), 0o644)
    }

    pub fn to_buffer(&self) -> String {
        let mut s = String::new();
        for entry in self.entries.values() {
            s.push_str(&entry.to_line());
            s.push('\n')
        }
        s
    }

    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Entry> {
        self.entries.get_mut(name)
    }

    pub fn insert(&mut self, entry: Entry) -> Result<()> {
        if self.gids.contains(&entry.gid) {
            bail!(
                "Group with GID {} already exists in group database",
                entry.gid
            );
        }

        if self.entries.contains_key(&entry.name) {
            bail!("Group {} already exists in group database", entry.name);
        }

        self.entries
            .entry(entry.name.clone())
            .or_insert(entry.clone());
        self.gids.insert(entry.gid);

        Ok(())
    }

    /// Allocate a new (i.e. unused) GID.
    ///
    /// Returns `Err` if it cannot allocate a new GID because all in the range are already used.
    pub fn allocate_gid(&self, is_normal_group: bool) -> Result<u32> {
        allocate_id(&self.gids, is_normal_group)
    }
}

impl Default for Group {
    fn default() -> Self {
        Self {
            entries: IndexMap::new(),
            gids: BTreeSet::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use expect_test::expect;
    use indoc::indoc;

    #[test]
    fn read_and_write_back() {
        let buffer = indoc! {"
            nixbld:x:30000:nixbld1,nixbld10,nixbld11,nixbld12,nixbld13,nixbld14,nixbld15,nixbld16,nixbld17,nixbld18,nixbld19,nixbld2,nixbld20,nixbld21,nixbld22,nixbld23,nixbld24,nixbld25,nixbld26,nixbld27,nixbld28,nixbld29,nixbld3,nixbld30,nixbld31,nixbld32,nixbld4,nixbld5,nixbld6,nixbld7,nixbld8,nixbld9
            messagebus:x:4:
            wheel:x:1:peter
        "};

        let group = Group::from_buffer(buffer);
        let recreated_buffer = group.to_buffer();
        assert_eq!(buffer, recreated_buffer);
    }

    #[test]
    fn skip_comments_and_broken_lines() {
        let buffer = indoc! {"
            # Comment
            piel:::
            wheel:x:1:peter
        "};
        let group = Group::from_buffer(buffer);
        let recreated_buffer = group.to_buffer();

        let expected = expect![[r#"
            wheel:x:1:peter
        "#]];
        expected.assert_eq(&recreated_buffer);
    }
}
