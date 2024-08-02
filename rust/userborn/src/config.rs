use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
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
    #[serde(flatten)]
    pub password: Password,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Password {
    pub password: Option<String>,
    pub hashed_password: Option<String>,
    pub hashed_password_file: Option<String>,
    pub initial_password: Option<String>,
    pub initial_hashed_password: Option<String>,
}

#[derive(Deserialize, Debug)]
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
    pub members: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default)]
    pub users: Vec<User>,
    #[serde(default)]
    pub groups: Vec<Group>,
}

impl Config {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path)?;
        Self::from_reader(file)
    }

    fn from_reader(reader: impl Read) -> Result<Self> {
        serde_json::from_reader(reader).context("Failed to parse config")
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
                }
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
