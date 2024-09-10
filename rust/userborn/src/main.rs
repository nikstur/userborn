mod config;
mod fs;
mod group;
mod id;
mod passwd;
mod password;
mod shadow;

use std::{collections::BTreeSet, io::Write, process::ExitCode};

use anyhow::{anyhow, Context, Result};
use log::{Level, LevelFilter};

use config::Config;
use group::Group;
use passwd::Passwd;
use password::HashedPassword;
use shadow::Shadow;

/// Path to the nologin binary.
const NO_LOGIN: &str = "/run/current-system/sw/bin/nologin";
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

    update_users_and_groups(&config, &mut group_db, &mut passwd_db, &mut shadow_db);

    warn_about_weak_password_hashes(&shadow_db);

    log::debug!("Persisting files to disk...");
    // We should skip this if the files haven't actually changed
    // We should create backup files with an `-` appended to the file name.
    group_db.to_file(group_path)?;
    passwd_db.to_file(passwd_path)?;
    shadow_db.to_file_sorted(&passwd_db, shadow_path)?;

    Ok(())
}

/// Create and update users and groups in the provided databases.
///
/// Doesn't actually write anything to disk, only mutates the databases in memory.
fn update_users_and_groups(
    config: &Config,
    group_db: &mut Group,
    passwd_db: &mut Passwd,
    shadow_db: &mut Shadow,
) {
    for group_config in &config.groups {
        if let Some(existing_entry) = group_db.get_mut(&group_config.name) {
            existing_entry.update(group_config.members.clone());
        } else if let Err(e) = create_group(group_config, group_db) {
            log::error!("Failed to create group {}: {e:#}", group_config.name);
        };
    }

    let mut users_in_config: BTreeSet<&str> = BTreeSet::new();

    for user_config in &config.users {
        users_in_config.insert(&user_config.name);

        if let Some(existing_entry) = passwd_db.get_mut(&user_config.name) {
            if let Err(e) = update_user(existing_entry, user_config, group_db, shadow_db) {
                log::error!("Failed to update user {}: {e:#}", user_config.name);
            };
        } else if let Err(e) = create_user(user_config, group_db, passwd_db, shadow_db) {
            log::error!("Failed to create user {}: {e:#}", user_config.name);
        };
    }

    // Find users in the shadow DB that are not in the config and disable them.
    for entry in shadow_db.entries_mut() {
        if !users_in_config.contains(entry.name()) {
            log::info!("Locking account for user {}...", entry.name());
            entry.lock_account();
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
            members: vec![user_config.name.clone()],
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
        user_config.shell.clone().unwrap_or(NO_LOGIN.into()),
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

        let hashed_password =
            HashedPassword::from_config(&user_config.password, &user_config.name)?.and_then(
                |hashed_password| match hashed_password {
                    HashedPassword::Override(s) => Some(s),
                    HashedPassword::Initial(_) => None,
                },
            );

        existing_entry.update(hashed_password);
    } else {
        log::debug!("Creating shadow entry for {}...", user_config.name);

        let hashed_password =
            HashedPassword::from_config(&user_config.password, &user_config.name)?.map(
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
    };
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
                    "home": "/home/normalo",
                    "shell": "/bin/bash",
                    "hashedPassword": "$y$j9T$kX/HY3hhcOSAlNLIhIhcL0$6TUZ0NNT18KBynYbuezPnk79TqyzRjH0BTE5h/m6Go7",
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

    #[test]
    fn update_users_and_groups_across_generations() -> Result<()> {
        let mut group_db = Group::default();
        let mut passwd_db = Passwd::default();
        let mut shadow_db = Shadow::default();

        // GEN 0

        update_users_and_groups(&gen0()?, &mut group_db, &mut passwd_db, &mut shadow_db);

        let expected_group = expect![[r#"
            root:x:0:root
            wheel:x:999:normalo
        "#]];
        expected_group.assert_eq(&group_db.to_buffer());

        let expected_passwd = expect![[r"
            root:x:0:0:::/run/current-system/sw/bin/nologin
        "]];
        expected_passwd.assert_eq(&passwd_db.to_buffer());

        let expected_shadow = expect![[r"
            root:!*:1::::::
        "]];
        expected_shadow.assert_eq(&shadow_db.to_buffer_sorted(&passwd_db));

        // GEN 1

        update_users_and_groups(&gen1()?, &mut group_db, &mut passwd_db, &mut shadow_db);

        let expected_group = expect![[r#"
            root:x:0:root
            initial:x:998:initial
            wheel:x:999:normalo,initial
            normalo:x:1000:normalo
        "#]];
        expected_group.assert_eq(&group_db.to_buffer());

        let expected_passwd = expect![[r#"
            root:x:0:0:::/run/current-system/sw/bin/nologin
            initial:x:999:999:::/run/current-system/sw/bin/nologin
            normalo:x:1000:1000::/home/normalo:/bin/bash
        "#]];
        expected_passwd.assert_eq(&passwd_db.to_buffer());

        let expected_shadow = expect![[r#"
            root:!*:1::::::
            initial:$y$j9T$2e5ARUyMfmJ0nW9ZMPFg50$EGgRGQBqq0r/fxRlIRXL86K61o/ESEsIdVZYkyQvyN2:1::::::
            normalo:$y$j9T$kX/HY3hhcOSAlNLIhIhcL0$6TUZ0NNT18KBynYbuezPnk79TqyzRjH0BTE5h/m6Go7:1::::::
        "#]];
        expected_shadow.assert_eq(&shadow_db.to_buffer_sorted(&passwd_db));

        // GEN 2

        update_users_and_groups(&gen2()?, &mut group_db, &mut passwd_db, &mut shadow_db);

        let expected_group = expect![[r#"
            root:x:0:root
            initial:x:998:initial
            wheel:x:999:normalo,initial
            normalo:x:1000:normalo
        "#]];
        expected_group.assert_eq(&group_db.to_buffer());

        let expected_passwd = expect![[r#"
            root:x:0:0::/root:/run/current-system/sw/bin/nologin
            initial:x:999:999:::/run/current-system/sw/bin/nologin
            normalo:x:1000:1000:I'm normal I swear:/home/normalo:/bin/bash
        "#]];
        expected_passwd.assert_eq(&passwd_db.to_buffer());

        let expected_shadow = expect![[r#"
            root:!*:1::::::
            initial:!*:1::::::
            normalo:$y$j9T$CZSAJTLCfrBvcCgvOTY4W1$G7uzyX3O6K.DR8KJLL/oL.8EREPSRTIjBn76SpvcH4A:1::::::
        "#]];
        expected_shadow.assert_eq(&shadow_db.to_buffer_sorted(&passwd_db));

        Ok(())
    }
}
