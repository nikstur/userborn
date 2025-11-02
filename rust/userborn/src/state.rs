use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;

const STATE_FILE_NAME: &str = "userborn.state";

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ManagedEntities {
    pub users: HashSet<String>,
    pub groups: HashSet<String>,
}

pub struct StateManager {
    state_file_path: PathBuf,
}

impl StateManager {
    pub fn new(directory: &str) -> Self {
        Self {
            state_file_path: Path::new(directory).join(STATE_FILE_NAME),
        }
    }

    pub fn load_managed_entities(&self) -> Result<ManagedEntities> {
        if !self.state_file_path.exists() {
            return Ok(ManagedEntities::default());
        }

        let mut file = File::open(&self.state_file_path)
            .with_context(|| format!("Failed to open state file: {:?}", self.state_file_path))?;
        
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .with_context(|| "Failed to read state file")?;

        let managed: ManagedEntities = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse state file: {:?}. The file may be corrupted. Delete it to reset state or fix the JSON syntax.", self.state_file_path))?;

        Ok(managed)
    }

    pub fn save_managed_entities(&self, config: &Config) -> Result<()> {
        let managed = ManagedEntities {
            users: config.users.iter().map(|u| u.name.clone()).collect(),
            groups: config.groups.iter().map(|g| g.name.clone()).collect(),
        };
        
        let json = serde_json::to_string_pretty(&managed)
            .with_context(|| "Failed to serialize managed entities")?;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.state_file_path)
            .with_context(|| format!("Failed to create/open state file: {:?}", self.state_file_path))?;

        file.write_all(json.as_bytes())
            .with_context(|| "Failed to write state file")?;

        log::debug!("Saved managed entities to state file: {:?}", self.state_file_path);
        Ok(())
    }
}

pub struct OwnershipDiff {
    pub users_to_manage: HashSet<String>,
    pub groups_to_manage: HashSet<String>,
    pub users_to_remove: HashSet<String>,
    pub groups_to_remove: HashSet<String>,
}

impl OwnershipDiff {
    pub fn compute(previous: &ManagedEntities, config: &Config) -> Self {
        let current_users: HashSet<String> = config.users.iter().map(|u| u.name.clone()).collect();
        let current_groups: HashSet<String> = config.groups.iter().map(|g| g.name.clone()).collect();

        let users_to_remove = previous.users.difference(&current_users).cloned().collect();
        let groups_to_remove = previous.groups.difference(&current_groups).cloned().collect();

        Self {
            users_to_manage: current_users,
            groups_to_manage: current_groups,
            users_to_remove,
            groups_to_remove,
        }
    }

    pub fn has_changes(&self, previous: &ManagedEntities) -> bool {
        self.users_to_manage != previous.users 
            || self.groups_to_manage != previous.groups
    }

    pub fn log_changes(&self, previous: &ManagedEntities) {
        if !self.has_changes(previous) {
            log::info!("No ownership changes detected.");
            return;
        }

        log::info!("Ownership changes detected:");

        for user in &self.users_to_manage {
            if !previous.users.contains(user) {
                log::info!("  + Taking ownership of user: {}", user);
            } else {
                log::info!("  ~ Managing user: {}", user);
            }
        }

        for user in &self.users_to_remove {
            log::info!("  - Removing managed user: {}", user);
        }

        for group in &self.groups_to_manage {
            if !previous.groups.contains(group) {
                log::info!("  + Taking ownership of group: {}", group);
            } else {
                log::info!("  ~ Managing group: {}", group);
            }
        }

        for group in &self.groups_to_remove {
            log::info!("  - Removing managed group: {}", group);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Group as ConfigGroup, User as ConfigUser, Password};

    fn make_user(name: &str) -> ConfigUser {
        ConfigUser {
            is_normal: true,
            name: name.to_string(),
            uid: None,
            group: None,
            description: None,
            home: None,
            shell: None,
            password: Password {
                password: None,
                hashed_password: None,
                hashed_password_file: None,
                initial_password: None,
                initial_hashed_password: None,
            },
        }
    }

    fn make_group(name: &str) -> ConfigGroup {
        ConfigGroup {
            is_normal: true,
            name: name.to_string(),
            gid: None,
            members: Default::default(),
        }
    }

    #[test]
    fn test_no_changes() {
        let previous = ManagedEntities {
            users: ["user1"].iter().map(|s| s.to_string()).collect(),
            groups: ["group1"].iter().map(|s| s.to_string()).collect(),
        };

        let config = Config {
            users: vec![make_user("user1")],
            groups: vec![make_group("group1")],
        };

        let diff = OwnershipDiff::compute(&previous, &config);
        assert!(!diff.has_changes(&previous));
    }

    #[test]
    fn test_user_added() {
        let previous = ManagedEntities::default();

        let config = Config {
            users: vec![make_user("user1")],
            groups: vec![],
        };

        let diff = OwnershipDiff::compute(&previous, &config);
        assert!(diff.has_changes(&previous));
        assert!(diff.users_to_manage.contains("user1"));
        assert!(diff.users_to_remove.is_empty());
    }

    #[test]
    fn test_user_removed() {
        let previous = ManagedEntities {
            users: ["user1"].iter().map(|s| s.to_string()).collect(),
            groups: Default::default(),
        };

        let config = Config {
            users: vec![],
            groups: vec![],
        };

        let diff = OwnershipDiff::compute(&previous, &config);
        assert!(diff.has_changes(&previous));
        assert!(diff.users_to_remove.contains("user1"));
        assert!(!diff.users_to_manage.contains("user1"));
    }
}