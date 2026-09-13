use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
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
    /// Range for dynamically allocated normal UIDs (login.defs `UID_MIN`/`UID_MAX`).
    #[serde(default)]
    pub normal_uid_range: IdRange,
    /// Range for dynamically allocated normal GIDs (login.defs `GID_MIN`/`GID_MAX`).
    #[serde(default)]
    pub normal_gid_range: IdRange,
    /// Users to manage.
    #[serde(default)]
    pub users: Vec<User>,
    /// Groups to manage.
    #[serde(default)]
    pub groups: Vec<Group>,
}

/// The lowest ID considered "normal" (i.e. not a system ID).
pub const NORMAL_ID_MIN: u32 = 1000;

/// Inclusive range from which normal IDs are dynamically allocated.
#[derive(Deserialize, Debug, Clone, Copy)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct IdRange {
    pub min: u32,
    pub max: u32,
}

impl Default for IdRange {
    fn default() -> Self {
        Self {
            min: NORMAL_ID_MIN,
            max: 29999,
        }
    }
}

impl IdRange {
    pub fn validate(self) -> Result<()> {
        if self.min > self.max {
            bail!("Invalid ID range: min ({}) > max ({})", self.min, self.max);
        }
        if self.min < NORMAL_ID_MIN {
            bail!(
                "Invalid ID range: min ({}) must be at least {NORMAL_ID_MIN}",
                self.min
            );
        }
        Ok(())
    }
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
        let config: Self = serde_json::from_slice(&contents).context("Failed to parse config")?;
        config
            .normal_uid_range
            .validate()
            .context("normalUidRange")?;
        config
            .normal_gid_range
            .validate()
            .context("normalGidRange")?;
        Ok(config)
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
            "normalUidRange": { "min": 30000, "max": 39999 },
            "normalGidRange": { "min": 40000, "max": 49999 },
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

    #[test]
    fn validate_range() {
        assert!(IdRange::default().validate().is_ok());
        assert!(
            IdRange {
                min: 999,
                max: 2000
            }
            .validate()
            .is_err()
        );
        assert!(
            IdRange {
                min: 2000,
                max: 1999
            }
            .validate()
            .is_err()
        );
    }
}
