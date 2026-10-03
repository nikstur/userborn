use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result};
use serde::Deserialize;

/// # User
/// The configuration for a single user.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct User {
    /// Whether the user is a "normal" or a "system" user.
    #[serde(default)]
    pub is_normal: bool,
    /// Name of the user.
    pub name: String,
    /// UID of the user.
    pub uid: Option<u32>,
    /// The primary group of the user.
    ///
    /// This can either be the name of the user or the GID.
    pub group: Option<String>,
    /// Description (GECOS) of the user.
    pub description: Option<String>,
    /// Home directory of the user.
    pub home: Option<String>,
    /// Shell of the user.
    pub shell: Option<String>,
    /// Account expiration date (`YYYY-MM-DD`). Unset clears it.
    pub expires: Option<String>,
    /// Whether to automatically allocate a subordinate UID/GID range for this user.
    #[serde(default)]
    pub auto_sub_id_range: bool,
    /// Explicit subordinate UID ranges for this user.
    #[serde(default)]
    pub sub_uid_ranges: Vec<SubIdRange>,
    /// Explicit subordinate GID ranges for this user.
    #[serde(default)]
    pub sub_gid_ranges: Vec<SubIdRange>,
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
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct Password {
    /// Plaintext password.
    pub password: Option<String>,
    /// Hashed password that was created with ``crypt()`` of libxcrypt.
    pub hashed_password: Option<String>,
    /// Path to a file containing a hashed password created with ``crypt()`` of libxcrypt.
    pub hashed_password_file: Option<String>,
    /// Initial plaintext password for the user that won't be applied if a password is already set.
    pub initial_password: Option<String>,
    /// Same as ``initial_password`` but with a hashed password.
    pub initial_hashed_password: Option<String>,
}

/// # Group
/// The configuration for a single group.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct Group {
    /// Whether the group is a "normal" or a "system" group.
    #[serde(default)]
    pub is_normal: bool,
    /// Name of the group.
    pub name: String,
    /// GID of the user's primary group.
    pub gid: Option<u32>,
    /// Members of this group.
    #[serde(default)]
    pub members: BTreeSet<String>,
}

/// # Userborn Configuration
/// Complete configuration for a generation of users and groups.
#[derive(Deserialize, Debug, Clone)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Users to manage.
    #[serde(default)]
    pub users: Vec<User>,
    /// Groups to manage.
    #[serde(default)]
    pub groups: Vec<Group>,
}

/// Range of subordiate IDs to create.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct SubIdRange {
    /// First ID in the range.
    pub start: u64,
    /// Number of consecutive IDs in the range.
    pub count: u64,
}

impl Config {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let contents = fs::read(&path)
            .with_context(|| format!("Failed to read {}", path.as_ref().display()))?;
        serde_json::from_slice(&contents).context("Failed to parse config")
    }

    #[must_use]
    pub fn user_names(&self) -> HashSet<String> {
        self.users.iter().map(|u| u.name.clone()).collect()
    }

    #[must_use]
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
                    "expires": "2030-01-31",
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
