use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::subid;

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// Whether the user is a "normal" or a "system" user
    #[serde(default)]
    pub is_normal: bool,
    /// The name of the user
    pub name: String,
    /// The UID of the user
    pub uid: Option<u32>,
    /// The primary group of the user.
    ///
    /// This can either be the name of the user or the GID.
    pub group: Option<String>,
    /// The description of the user
    pub description: Option<String>,
    /// The home directory of the user
    pub home: Option<String>,
    /// The shell of the user
    pub shell: Option<String>,
    /// Whether to automatically allocate a subordinate UID/GID range for this user.
    #[serde(default)]
    pub auto_sub_id_range: bool,
    /// Explicit subordinate UID ranges for this user.
    #[serde(default)]
    pub sub_uid_ranges: Vec<subid::Range>,
    /// Explicit subordinate GID ranges for this user.
    #[serde(default)]
    pub sub_gid_ranges: Vec<subid::Range>,
    #[serde(flatten)]
    pub password: Password,
}

impl User {
    pub fn has_sub_id_config(&self) -> bool {
        self.auto_sub_id_range || !self.sub_uid_ranges.is_empty() || !self.sub_gid_ranges.is_empty()
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Password {
    pub password: Option<String>,
    pub hashed_password: Option<String>,
    pub hashed_password_file: Option<String>,
    pub initial_password: Option<String>,
    pub initial_hashed_password: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Group {
    /// Whether the group is a "normal" or a "system" group
    #[serde(default)]
    pub is_normal: bool,
    /// The name of the group
    pub name: String,
    /// The GID of the users primary group
    pub gid: Option<u32>,
    /// The members of this group
    #[serde(default)]
    pub members: BTreeSet<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub users: Vec<User>,
    #[serde(default)]
    pub groups: Vec<Group>,
}

impl Config {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let contents = fs::read(&path)
            .with_context(|| format!("Failed to read {}", path.as_ref().display()))?;
        serde_json::from_slice(&contents).context("Failed to parse config")
    }

    pub fn user_names(&self) -> HashSet<String> {
        self.users.iter().map(|u| u.name.clone()).collect()
    }

    pub fn group_names(&self) -> HashSet<String> {
        self.groups.iter().map(|g| g.name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config() -> Result<()> {
        let value = serde_json::json!({
            "users": [
                {
                    "isNormal": true,
                    "name": "normalo",
                    "home": "/home/normalo",
                    "shell": "/bin/bash",
                    "password": "insecure",
                },
                {
                    "isNormal": false,
                    "name": "sysuser",
                    "home": "/home/sysuser",
                    "shell": "/bin/bash",
                },
                {
                    "name": "barebones",
                },
                {
                    "isNormal": true,
                    "name": "hassubids",
                    "autoSubIdRange": true,
                    "subUidRanges": [ { "start": 200_000, "count": 131_072 } ],
                },
            ],
            "groups": [
                {
                    "name": "wheel",
                    "members": [ "normalo", "barebones" ],
                },
                {
                    "name": "barebones",
                },
            ],
        });

        serde_json::from_value::<Config>(value)?;
        Ok(())
    }
}
