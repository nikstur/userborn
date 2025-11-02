mod config;
mod fs;
mod group;
mod id;
mod passwd;
mod password;
mod shadow;
mod state;

use std::{
    collections::{BTreeSet, HashSet},
    io::Write,
    process::ExitCode,
};

use anyhow::{anyhow, Context, Result};
use log::{Level, LevelFilter};

use config::Config;
use group::Group;
use passwd::Passwd;
use password::HashedPassword;
use shadow::Shadow;
use state::{OwnershipDiff, StateManager};

/// Fallback path to the nologin binary.
///
/// This is used when `USERBORN_NO_LOGIN_PATH` is not set during runtime and
/// `USERBORN_NO_LOGIN_DEFAULT_PATH` hasn't been set during compilation.
const NO_LOGIN_FALLBACK: &str = "/run/current-system/sw/bin/nologin";
/// Default path to the nologin binary.
///
/// This can be configured via a compile-time environment variable.
const NO_LOGIN_DEFAULT: Option<&'static str> = option_env!("USERBORN_NO_LOGIN_DEFAULT_PATH");
const DEFAULT_DIRECTORY: &str = "/etc";

fn main() -> ExitCode {
    // Setup the logger to use the kernel's `printk()` scheme so that systemd can interpret the
    // levels.
    env_logger::builder()
        .format(|buf, record| {
            writeln!(
                buf,
                "<{}>{}",
                match record.level() {
                    Level::Error => 3,
                    Level::Warn => 4,
                    Level::Info => 6,
                    Level::Debug | Level::Trace => 7,
                },
                record.args()
            )
        })
        .filter(None, LevelFilter::Info)
        .init();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log::error!("{err:#}.");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .ok_or(anyhow!("No config provided"))?;
    let directory = std::env::args().nth(2).unwrap_or(DEFAULT_DIRECTORY.into());

    let config = Config::from_file(config_path)?;

    let group_path = format!("{directory}/group");
    let passwd_path = format!("{directory}/passwd");
    let shadow_path = format!("{directory}/shadow");

    let mut group_db = Group::from_file(&group_path).unwrap_or_default();
    let mut passwd_db = Passwd::from_file(&passwd_path).unwrap_or_default();
    let mut shadow_db = Shadow::from_file(&shadow_path).unwrap_or_default();

    // Check if stateful mode is enabled via environment variable
    let stateful_mode = std::env::var("USERBORN_STATEFUL")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    if stateful_mode {
        log::info!("Running in stateful mode");

        // Initialize state manager
        let state_manager = StateManager::new(&directory);

        // Load previously managed entities
        let previous_managed = state_manager
            .load_managed_entities()
            .context("Failed to load managed entities state")?;

        // Compute ownership changes
        let diff = OwnershipDiff::compute(&previous_managed, &config);

        // Log what changes we detected
        diff.log_changes(&previous_managed);

        // Apply updates with stateful behavior
        update_users_and_groups_diff(
            &config,
            &mut group_db,
            &mut passwd_db,
            &mut shadow_db,
            Some(&diff),
        );

        // Save the current managed entities
        state_manager
            .save_managed_entities(&config)
            .context("Failed to save managed entities state")?;
    } else {
        log::info!("Running in stateless mode");

        // Use stateless behavior
        update_users_and_groups_diff(&config, &mut group_db, &mut passwd_db, &mut shadow_db, None);
    }

    warn_about_weak_password_hashes(&shadow_db);

    log::debug!("Persisting files to disk...");
    // We should skip this if the files haven't actually changed
    // We should create backup files with an `-` appended to the filename.
    group_db.to_file(group_path)?;
    passwd_db.to_file(passwd_path)?;
    shadow_db.to_file_sorted(&passwd_db, shadow_path)?;

    Ok(())
}

/// Create and update users and groups in the provided databases.
///
/// Doesn't actually write anything to disk, only mutates the databases in memory.
fn update_users_and_groups_diff(
    config: &Config,
    group_db: &mut Group,
    passwd_db: &mut Passwd,
    shadow_db: &mut Shadow,
    diff: Option<&OwnershipDiff>,
) {
    match diff {
        Some(ownership_diff) => {
            // Remove groups that are no longer managed
            for group_name in &ownership_diff.groups_to_remove {
                if let Some(existing_entry) = group_db.get_mut(group_name) {
                    existing_entry.update(BTreeSet::new());
                    log::info!("Emptied previously managed group {}", group_name);
                }
            }

            // Remove users that are no longer managed
            let users_to_remove_refs: BTreeSet<&str> = ownership_diff
                .users_to_remove
                .iter()
                .map(|s| s.as_str())
                .collect();
            lock_users(shadow_db, &users_to_remove_refs, "previously managed");
        }
        None => {
            // Stateless mode: clean up all unmanaged entities
            let groups_in_config: BTreeSet<&str> =
                config.groups.iter().map(|g| g.name.as_str()).collect();
            let users_in_config: BTreeSet<&str> =
                config.users.iter().map(|u| u.name.as_str()).collect();

            // Empty groups not in config
            for entry in group_db.entries_mut() {
                if !groups_in_config.contains(entry.name()) {
                    entry.update(BTreeSet::new());
                }
            }

            // Lock users not in config
            let users_to_lock: BTreeSet<String> = shadow_db
                .entries()
                .into_iter()
                .map(|entry| entry.name().to_owned())
                .filter(|name| !users_in_config.contains(name.as_str()))
                .collect();

            let users_to_lock_refs: BTreeSet<&str> =
                users_to_lock.iter().map(|s| s.as_str()).collect();
            lock_users(shadow_db, &users_to_lock_refs, "");
        }
    }

    // Process all configured groups and users
    process_groups_from_config(config, group_db);
    process_users_from_config(config, group_db, passwd_db, shadow_db);

    // Update implicit primary groups
    let users_to_remove = diff.map(|d| &d.users_to_remove);
    update_implicit_primary_groups(config, group_db, users_to_remove);
}

/// Process all groups from the config, updating existing ones or creating new ones.
fn process_groups_from_config(config: &Config, group_db: &mut Group) {
    for group_config in &config.groups {
        if let Some(existing_entry) = group_db.get_mut(&group_config.name) {
            existing_entry.update(group_config.members.clone());
        } else if let Err(e) = create_group(group_config, group_db) {
            log::error!("Failed to create group {}: {e:#}", group_config.name);
        }
    }
}

/// Process all users from the config, updating existing ones or creating new ones.
fn process_users_from_config(
    config: &Config,
    group_db: &mut Group,
    passwd_db: &mut Passwd,
    shadow_db: &mut Shadow,
) {
    for user_config in &config.users {
        if let Some(existing_entry) = passwd_db.get_mut(&user_config.name) {
            if let Err(e) = update_user(existing_entry, user_config, group_db, shadow_db) {
                log::error!("Failed to update user {}: {e:#}", user_config.name);
            }
        } else if let Err(e) = create_user(user_config, group_db, passwd_db, shadow_db) {
            log::error!("Failed to create user {}: {e:#}", user_config.name);
        }
    }
}

/// Get the set of implicit primary groups (users without explicit group).
fn get_implicit_primary_groups(config: &Config) -> BTreeSet<&str> {
    config
        .users
        .iter()
        .filter(|user_config| user_config.group.is_none())
        .map(|user_config| user_config.name.as_str())
        .collect()
}

/// Update implicit primary groups to contain only their associated user.
/// If users_to_remove is provided, also empty groups for those removed users.
fn update_implicit_primary_groups(
    config: &Config,
    group_db: &mut Group,
    users_to_remove: Option<&HashSet<String>>,
) {
    let implicit_primary_groups = get_implicit_primary_groups(config);

    for entry in group_db.entries_mut() {
        if implicit_primary_groups.contains(entry.name()) {
            let should_be_members = BTreeSet::from([entry.name().to_owned()]);
            if entry.user_list() != &should_be_members {
                entry.update(should_be_members);
                log::debug!("Updated implicit primary group {}", entry.name());
            }
        } else if let Some(users_to_remove) = users_to_remove {
            // In stateful mode, also empty groups for removed users
            if users_to_remove.contains(entry.name()) {
                if !entry.user_list().is_empty() {
                    entry.update(BTreeSet::new());
                    log::debug!(
                        "Emptied implicit primary group for removed user {}",
                        entry.name()
                    );
                }
            }
        }
    }
}

/// Lock user accounts that are in the provided list.
fn lock_users(shadow_db: &mut Shadow, users_to_lock: &BTreeSet<&str>, context: &str) {
    for username in users_to_lock {
        if let Some(existing_entry) = shadow_db.get_mut(username) {
            if context.is_empty() {
                log::info!("Locking account for user {}...", username);
            } else {
                log::info!("Locking account for {} user {}...", context, username);
            }
            existing_entry.lock_account();
        }
    }
}

/// Create a new group entry and add it to the database.
fn create_group(group_config: &config::Group, group_db: &mut Group) -> Result<()> {
    let gid = if let Some(gid) = group_config.gid {
        gid
    } else {
        group_db
            .allocate_gid(group_config.is_normal)
            .context("Failed to allocate new GID")?
    };

    let new_entry = group::Entry::new(group_config.name.clone(), gid, group_config.members.clone());

    let description = new_entry.describe();

    group_db
        .insert(&new_entry)
        .with_context(|| format!("Failed to add group entry {}", group_config.name))?;

    log::info!("Created group {description}.");

    Ok(())
}

/// Create a new user entry and add it to the database.
///
/// Creates an entry both in the passwd and the shadow database.
fn create_user(
    user_config: &config::User,
    group_db: &mut Group,
    passwd_db: &mut Passwd,
    shadow_db: &mut Shadow,
) -> Result<()> {
    log::debug!("Creating new passwd entry for {}...", user_config.name);

    let uid = if let Some(uid) = user_config.uid {
        uid
    } else {
        passwd_db
            .allocate_uid(user_config.is_normal)
            .context("Failed to allocate new UID")?
    };

    let gid = if let Some(ref primary_group) = user_config.group {
        resolve_group(primary_group, group_db)?
    } else {
        // If we cannot re-use the UID as GID (because it's already used), allocate a new GID.
        let gid = if group_db.contains_gid(uid) {
            None
        } else {
            Some(uid)
        };

        // No group was provided so create a new group with the same name of the user and re-use
        // the UID as GID.
        let group_config = config::Group {
            is_normal: user_config.is_normal,
            name: user_config.name.clone(),
            gid,
            members: BTreeSet::from([user_config.name.clone()]),
        };

        create_group(&group_config, group_db)
            .with_context(|| format!("Failed to create group for user {}", user_config.name))?;
        uid
    };

    let new_entry = passwd::Entry::new(
        user_config.name.clone(),
        uid,
        gid,
        user_config.description.clone().unwrap_or_default(),
        user_config.home.clone().unwrap_or_default(),
        user_config.shell.clone().unwrap_or(
            std::env::var("USERBORN_NO_LOGIN_PATH")
                .unwrap_or(NO_LOGIN_DEFAULT.unwrap_or(NO_LOGIN_FALLBACK).into()),
        ),
    );

    let description = new_entry.describe();

    passwd_db.insert(&new_entry).with_context(|| {
        format!(
            "Failed to add entry to passwd database for user {}",
            user_config.name
        )
    })?;

    ensure_shadow(user_config, shadow_db)?;

    log::info!("Created user {description}.");
    Ok(())
}

/// Update an already existing user, directly mutating the passed entry.
fn update_user(
    existing_entry: &mut passwd::Entry,
    user_config: &config::User,
    group_db: &Group,
    shadow_db: &mut Shadow,
) -> Result<()> {
    log::debug!("Updating passwd entry for {}...", user_config.name);

    let gid = user_config.group.as_ref().and_then(|g| {
        if let Ok(gid) = resolve_group(g, group_db) {
            Some(gid)
        } else {
            log::error!(
                "Group {g} doesn't exist. Not updating primary group of user {}.",
                user_config.name
            );
            None
        }
    });

    existing_entry.update(
        gid,
        user_config.description.clone(),
        user_config.home.clone(),
        user_config.shell.clone(),
    );

    ensure_shadow(user_config, shadow_db)?;

    Ok(())
}

/// Resolve a string that can either be a group name or a GID to a proper GID.
///
/// Resolve GID from group name using the group database.
fn resolve_group(s: &str, group_db: &Group) -> Result<u32> {
    if let Ok(uid) = s.parse::<u32>() {
        Ok(uid)
    } else {
        let existing_group_entry = group_db.get(s).ok_or(anyhow!("Group {s} doesn't exist"))?;
        Ok(existing_group_entry.gid())
    }
}

/// Ensure that a shadow entry exists for the provided uses.
///
/// Updates an existing shadow entry or creates a new one.
fn ensure_shadow(user_config: &config::User, shadow_db: &mut Shadow) -> Result<()> {
    if let Some(existing_entry) = shadow_db.get_mut(&user_config.name) {
        log::debug!("Updating shadow entry for {}...", user_config.name);

        let hashed_password = HashedPassword::from_config(
            &user_config.password,
            Some(existing_entry.password()),
            &user_config.name,
        )?
        .and_then(|hashed_password| match hashed_password {
            HashedPassword::Override(s) => Some(s),
            HashedPassword::Initial(_) => None,
        });

        existing_entry.update(hashed_password);
    } else {
        log::debug!("Creating shadow entry for {}...", user_config.name);

        let hashed_password =
            HashedPassword::from_config(&user_config.password, None, &user_config.name)?.map(
                |hashed_password| match hashed_password {
                    HashedPassword::Override(s) | HashedPassword::Initial(s) => s,
                },
            );

        let new_entry = shadow::Entry::new(user_config.name.clone(), hashed_password);

        shadow_db.insert(&new_entry).with_context(|| {
            format!(
                "Failed to add entry to shadow database for user {}",
                user_config.name
            )
        })?;
    }
    Ok(())
}

/// Emit warnings for user entries that use weak password hashing schemes.
fn warn_about_weak_password_hashes(shadow_db: &Shadow) {
    for entry in shadow_db.entries() {
        if !entry.uses_secure_hash() {
            log::warn!("User {} uses an insecure password hashing scheme. Update their password as soon as possible.", entry.name());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use expect_test::expect;

    fn gen0() -> Result<Config> {
        Ok(serde_json::from_value(serde_json::json!({
            "users": [
                {
                    "name": "root",
                    "uid": 0,
                },
                {
                    "isNormal": true,
                    "name": "normalo",
                    "home": "/home/normalo",
                    "shell": "/bin/bash",
                    "hashedPassword": "$y$j9T$BOO.gstYxWh8Lw.njfytQ/$K4sN06nBh0qFGegFS0hn5YkEOzzrr7woGHlSiUuCqS4", // "hello"
                },
            ],
            "groups": [
                {
                    "name": "wheel",
                    "members": [ "normalo", ],
                },
            ],
        }))?)
    }

    fn gen1() -> Result<Config> {
        Ok(serde_json::from_value(serde_json::json!({
            "users": [
                {
                    "name": "root",
                    "uid": 0,
                },
                {
                    "isNormal": true,
                    "name": "normalo",
                    // This should update the shell to zsh
                    "shell": "/bin/zsh",
                    // This shouldn't change the hash as it hashes the same as the existing
                    // password
                    "password": "hello",
                },
                {
                    "isNormal": false,
                    "name": "initial",
                    "initialHashedPassword": "$y$j9T$2e5ARUyMfmJ0nW9ZMPFg50$EGgRGQBqq0r/fxRlIRXL86K61o/ESEsIdVZYkyQvyN2",
                },
            ],
            "groups": [
                {
                    "name": "wheel",
                    "members": [ "normalo", "initial" ],
                },
            ],
        }))?)
    }

    fn gen2() -> Result<Config> {
        Ok(serde_json::from_value(serde_json::json!({
            "users": [
                {
                    "name": "root",
                    "uid": 0,
                    "home": "/root",
                    // This shouldn't apply. The user should stay disabled.
                    "initialHashedPassword": "$y$j9T$IMBPYrUksH4dZME8IQZPZ0$J3P/05qML9xZYHhkkIv3rNvXOAyb.tN56dJo8lTf0TA",
                },
                {
                    // The users should keep the previous values even though they aren't present
                    // here anymore.
                    "name": "normalo",
                    "description": "I'm normal I swear",
                    // This should change the password
                    "hashedPassword": "$y$j9T$CZSAJTLCfrBvcCgvOTY4W1$G7uzyX3O6K.DR8KJLL/oL.8EREPSRTIjBn76SpvcH4A",
                },
                // initial user should still exist even though we remove them from the config
            ],
            // wheel group should still exist even though we remove it from the config
        }))?)
    }

    /// Generic test function that runs across generations with either stateful or stateless mode
    fn test_across_generations_generic(use_stateful_mode: bool) -> Result<()> {
        use crate::state::{OwnershipDiff, StateManager};
        use tempfile::TempDir;

        // Explicitly set this because the expected values depend on this.
        std::env::set_var("USERBORN_NO_LOGIN_PATH", NO_LOGIN_FALLBACK);

        let mut group_db = Group::default();
        let mut passwd_db = Passwd::default();
        let mut shadow_db = Shadow::default();

        // Add unmanaged system user to test stateful mode
        let system_user = passwd::Entry::new(
            "system_user".to_string(),
            5000,
            5000,
            "System".to_string(),
            "/var/empty".to_string(),
            "/bin/false".to_string(),
        );
        passwd_db.insert(&system_user)?;
        shadow_db.insert(&shadow::Entry::new(
            "system_user".to_string(),
            Some("$y$j9T$system.hash$test".to_string()),
        ))?;

        // Set up state management
        let temp_dir = if use_stateful_mode {
            Some(TempDir::new().unwrap())
        } else {
            None
        };

        let state_manager = temp_dir
            .as_ref()
            .map(|td| StateManager::new(td.path().to_str().unwrap()));

        {
            let config = gen0()?;
            if let Some(ref sm) = state_manager {
                let previous_managed = sm.load_managed_entities()?;
                let diff = OwnershipDiff::compute(&previous_managed, &config);
                update_users_and_groups_diff(
                    &config,
                    &mut group_db,
                    &mut passwd_db,
                    &mut shadow_db,
                    Some(&diff),
                );
                sm.save_managed_entities(&config)?;
            } else {
                update_users_and_groups_diff(
                    &config,
                    &mut group_db,
                    &mut passwd_db,
                    &mut shadow_db,
                    None,
                );
            }
        }

        let expected_group_gen0 = expect![[r#"
            root:x:0:root
            wheel:x:999:normalo
            normalo:x:1000:normalo
        "#]];
        expected_group_gen0.assert_eq(&group_db.to_buffer());

        let expected_passwd_gen0 = expect![[r#"
            root:x:0:0:::/run/current-system/sw/bin/nologin
            normalo:x:1000:1000::/home/normalo:/bin/bash
            system_user:x:5000:5000:System:/var/empty:/bin/false
        "#]];
        expected_passwd_gen0.assert_eq(&passwd_db.to_buffer());

        let system_user_password = if use_stateful_mode {
            "$y$j9T$system.hash$test"
        } else {
            "!*"
        };
        let expected_shadow_gen0 = format!(
            "root:!*:1::::::\nnormalo:$y$j9T$BOO.gstYxWh8Lw.njfytQ/$K4sN06nBh0qFGegFS0hn5YkEOzzrr7woGHlSiUuCqS4:1::::::\nsystem_user:{}:1::::::",
            system_user_password
        );
        assert_eq!(
            shadow_db.to_buffer_sorted(&passwd_db).trim(),
            expected_shadow_gen0
        );

        {
            let config = gen1()?;
            if let Some(ref sm) = state_manager {
                let previous_managed = sm.load_managed_entities()?;
                let diff = OwnershipDiff::compute(&previous_managed, &config);
                update_users_and_groups_diff(
                    &config,
                    &mut group_db,
                    &mut passwd_db,
                    &mut shadow_db,
                    Some(&diff),
                );
                sm.save_managed_entities(&config)?;
            } else {
                update_users_and_groups_diff(
                    &config,
                    &mut group_db,
                    &mut passwd_db,
                    &mut shadow_db,
                    None,
                );
            }
        }

        let expected_group_gen1 = expect![[r#"
            root:x:0:root
            initial:x:998:initial
            wheel:x:999:initial,normalo
            normalo:x:1000:normalo
        "#]];
        expected_group_gen1.assert_eq(&group_db.to_buffer());

        let expected_passwd_gen1 = expect![[r#"
            root:x:0:0:::/run/current-system/sw/bin/nologin
            initial:x:999:999:::/run/current-system/sw/bin/nologin
            normalo:x:1000:1000::/home/normalo:/bin/zsh
            system_user:x:5000:5000:System:/var/empty:/bin/false
        "#]];
        expected_passwd_gen1.assert_eq(&passwd_db.to_buffer());

        // Shadow differs only by system_user lock status (checked above)
        let system_user_password = if use_stateful_mode {
            "$y$j9T$system.hash$test"
        } else {
            "!*"
        };
        let expected_shadow_gen1 = format!(
            "root:!*:1::::::\ninitial:$y$j9T$2e5ARUyMfmJ0nW9ZMPFg50$EGgRGQBqq0r/fxRlIRXL86K61o/ESEsIdVZYkyQvyN2:1::::::\nnormalo:$y$j9T$BOO.gstYxWh8Lw.njfytQ/$K4sN06nBh0qFGegFS0hn5YkEOzzrr7woGHlSiUuCqS4:1::::::\nsystem_user:{}:1::::::",
            system_user_password
        );
        assert_eq!(
            shadow_db.to_buffer_sorted(&passwd_db).trim(),
            expected_shadow_gen1
        );

        {
            let config = gen2()?;
            if let Some(ref sm) = state_manager {
                let previous_managed = sm.load_managed_entities()?;
                let diff = OwnershipDiff::compute(&previous_managed, &config);
                update_users_and_groups_diff(
                    &config,
                    &mut group_db,
                    &mut passwd_db,
                    &mut shadow_db,
                    Some(&diff),
                );
                sm.save_managed_entities(&config)?;
            } else {
                update_users_and_groups_diff(
                    &config,
                    &mut group_db,
                    &mut passwd_db,
                    &mut shadow_db,
                    None,
                );
            }
        }

        let expected_group_gen2 = expect![[r#"
                root:x:0:root
                initial:x:998:
                wheel:x:999:
                normalo:x:1000:normalo
            "#]];
        expected_group_gen2.assert_eq(&group_db.to_buffer());

        let expected_passwd_gen2 = expect![[r#"
            root:x:0:0::/root:/run/current-system/sw/bin/nologin
            initial:x:999:999:::/run/current-system/sw/bin/nologin
            normalo:x:1000:1000:I'm normal I swear:/home/normalo:/bin/zsh
            system_user:x:5000:5000:System:/var/empty:/bin/false
        "#]];
        expected_passwd_gen2.assert_eq(&passwd_db.to_buffer());

        let system_user_password = if use_stateful_mode {
            "$y$j9T$system.hash$test"
        } else {
            "!*"
        };
        let expected_shadow_gen2 = format!(
            "root:!*:1::::::\ninitial:!*:1::::::\nnormalo:$y$j9T$CZSAJTLCfrBvcCgvOTY4W1$G7uzyX3O6K.DR8KJLL/oL.8EREPSRTIjBn76SpvcH4A:1::::::\nsystem_user:{}:1::::::",
            system_user_password
        );
        assert_eq!(
            shadow_db.to_buffer_sorted(&passwd_db).trim(),
            expected_shadow_gen2
        );

        Ok(())
    }

    #[test]
    fn update_users_and_groups_across_generations() -> Result<()> {
        test_across_generations_generic(false)
    }

    #[test]
    fn update_users_and_groups_across_generations_stateful() -> Result<()> {
        test_across_generations_generic(true)
    }
}
