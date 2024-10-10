use std::fs;

use anyhow::{Context, Result};
use xcrypt::{crypt, crypt_gensalt};

use crate::config;

/// A hashed password.
///
/// This is normally derived from a config with `from_config`. The config can contain multiple passwords but only one
/// of them will be actually set.
///
/// This is the order in which they are considered:
///
/// - `hashed_password_file`
/// - `hashed_password`
/// - `password`
/// - `initial_hashed_password`
/// - `initial_password`
///
/// A password above another will "beat" one below and will be used to set the password to the
/// user. The rest are silently discarded.
pub enum HashedPassword {
    /// Password to always set.
    ///
    /// This will override an existing password.
    Override(String),
    /// Initial password.
    ///
    /// This will not be used to override an existing password but only to set a new password when
    /// a new account is created.
    Initial(String),
}

impl HashedPassword {
    pub fn from_config(
        password_config: &config::Password,
        current_password: Option<&str>,
        name: &str,
    ) -> Result<Option<Self>> {
        let hashed_password = if let Some(path) = &password_config.hashed_password_file {
            log::debug!("Using hashedPasswordFile {path:?} for user {name}...");
            let hashed_password = fs::read_to_string(path)
                .with_context(|| format!("Failed to read hashedPasswordFile {path:?}"))?;
            Some(Self::Override(hashed_password.trim().into()))
        } else if let Some(hashed_password) = &password_config.hashed_password {
            log::debug!("Using hashedPassword for user {name}...");
            Some(Self::Override(hashed_password.clone()))
        } else if let Some(raw_password) = &password_config.password {
            log::debug!("Using password for user {name}...");
            log::warn!(
                "User {name} uses a plaintext password. This is inscure and should only be used for testing purposes."
            );
            Some(Self::Override(
                hash_password(raw_password, current_password).context("Failed to hash password")?,
            ))
        } else if let Some(hashed_password) = &password_config.initial_hashed_password {
            log::debug!("Using initialHashedPassword for user {name}...");
            Some(Self::Initial(hashed_password.clone()))
        } else if let Some(raw_password) = &password_config.initial_password {
            log::debug!("Using initialPassword for user {name}...");
            log::warn!(
                "User {name} uses a plaintext password. This is inscure and should only be used for testing purposes."
            );
            Some(Self::Initial(
                hash_password(raw_password, current_password).context("Failed to hash password")?,
            ))
        } else {
            None
        };

        Ok(hashed_password)
    }
}

/// Hash a raw password using `libxcrypt`.
///
/// Optionally takes `current_password` to not change the hash (by means of a new salt) when the
/// actual password hasn't changed.
///
/// This function doesn't need to be particularly secure since the original password cannot be
/// treated as secure as it's passed via a plaintxt config. This is, e.g. why it doesn't zeroize
/// the buffer.
///
/// It only serves to convert a non-secret raw password into a format that is understood by
/// /etc/shadow.
fn hash_password(new_password: &str, current_password: Option<&str>) -> Result<String> {
    if let Some(current) = current_password {
        let hashed_password_result = crypt(new_password, current);

        // If hashing fails (e.g. because the current password is invalid), generate a hash with a
        // new salt.
        if let Ok(hashed_password) = hashed_password_result {
            // If the passwords don't hash the same, generate a hash with a new salt.
            if hashed_password == current {
                return Ok(hashed_password);
            }
        }
    }
    let setting =
        crypt_gensalt(Some("$y$"), 0, None).context("Failed to generate setting for crypt")?;
    Ok(crypt(new_password, &setting)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::bail;

    #[test]
    fn hash_password_from_config_correctly() -> Result<()> {
        let config = config::Password {
            password: Some("hello".into()),
            hashed_password: None,
            hashed_password_file: None,
            initial_password: Some("mellow".into()),
            initial_hashed_password: None,
        };

        let hashed_password = HashedPassword::from_config(&config, None, "test-name")?
            .context("Failed to convert config to HashedPassword")?;

        if let HashedPassword::Override(s) = hashed_password {
            assert!(s.starts_with("$y$"));
        } else {
            bail!("Wrong HashedPassword variant")
        };

        Ok(())
    }

    #[test]
    fn rehash_password_the_same() -> Result<()> {
        let password = "hello";

        let current_password =
            "$y$j9T$qPA34Fz5ALUVSUMv1Ihat.$5mK2beqNNh5QhircGqGFJJZwA9H.vi8vV7E3Mt4oug1";

        let hashed_password = hash_password(password, Some(current_password))?;

        assert_eq!(hashed_password, current_password);

        Ok(())
    }

    #[test]
    fn hash_new_password_differently() -> Result<()> {
        let password = "mello";

        let current_password =
            "$y$j9T$qPA34Fz5ALUVSUMv1Ihat.$5mK2beqNNh5QhircGqGFJJZwA9H.vi8vV7E3Mt4oug1";

        let hashed_password = hash_password(password, Some(current_password))?;

        // Assert that the salt has changed
        let new_password_components = hashed_password.split('$').nth(3);
        let current_password_components = current_password.split('$').nth(3);
        assert_ne!(new_password_components, current_password_components);

        // Assert that the whole hashed password has changed
        assert_ne!(hashed_password, current_password);

        Ok(())
    }

    #[test]
    fn invalid_current_password() -> Result<()> {
        let password = "hello";

        let current_password = "!*";

        let hashed_password = hash_password(password, Some(current_password))?;

        assert_ne!(hashed_password, current_password);
        assert!(hashed_password.starts_with('$'));

        Ok(())
    }
}
