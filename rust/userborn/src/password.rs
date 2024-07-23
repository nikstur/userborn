use std::{fs, process::Command};

use anyhow::{Context, Result};

use crate::config;

/// A hashed password.
///
/// This is normally derived from a config with `from_config`. The config can contain multiple passwords but only one
/// of them will be actually set.
///
/// This is the order in which they are considered:
///
/// - hashed_password_file
/// - hashed_password
/// - password
/// - initial_hashed_password
/// - initial_password
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
    /// This will not be used to override an existing password but only to set if a new account is
    /// created
    Initial(String),
}

impl HashedPassword {
    pub fn from_config(value: &config::Password) -> Result<Option<Self>> {
        let hashed_password = if let Some(path) = &value.hashed_password_file {
            log::debug!("Using hashedPasswordFile {path:?}...");
            let hashed_password = fs::read_to_string(path)
                .with_context(|| format!("Failed to read hashedPasswordFile {path:?}"))?;
            Some(Self::Override(hashed_password.trim().into()))
        } else if let Some(hashed_password) = &value.hashed_password {
            log::debug!("Using hashedPassword...");
            Some(Self::Override(hashed_password.clone()))
        } else if let Some(raw_password) = &value.password {
            log::debug!("Using password...");
            log::warn!("Using a plaintext password. This is inscure and should only be used for testing purposes.");
            Some(Self::Override(hash_password(raw_password)?))
        } else if let Some(hashed_password) = &value.initial_hashed_password {
            log::debug!("Using initialHashedPassword...");
            Some(Self::Initial(hashed_password.clone()))
        } else if let Some(raw_password) = &value.initial_password {
            log::debug!("Using initialPassword...");
            log::warn!("Using a plaintext password. This is inscure and should only be used for testing purposes.");
            Some(Self::Initial(hash_password(raw_password)?))
        } else {
            None
        };

        Ok(hashed_password)
    }
}

/// Hash a raw password using `mkpasswd`.
///
/// This function doesn't need to be particularly secure since the original password cannot be
/// treated as secure as it's passed via a plaintxt config.
///
/// It only serves to convert a non-secret raw password into a format that is understood by
/// /etc/shadow.
fn hash_password(raw_password: &str) -> Result<String> {
    let output = Command::new("mkpasswd")
        .arg(raw_password)
        .output()
        .context("Failed to run mkpasswd. Most likely, the binary is not on PATH")?;

    if !output.status.success() {
        Err(anyhow::anyhow!("Failed to hash password"))
    } else {
        Ok(String::from_utf8(output.stdout)
            .context("Failed to interpret stdout as a UTF-8 string")?
            .trim()
            .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::bail;

    #[test]
    fn hash_password_correctly() -> Result<()> {
        let config = config::Password {
            password: Some("hello".into()),
            hashed_password: None,
            hashed_password_file: None,
            initial_password: Some("mellow".into()),
            initial_hashed_password: None,
        };

        let hashed_password = HashedPassword::from_config(&config)?
            .context("Failed to convert config to HashedPassword")?;

        if let HashedPassword::Override(s) = hashed_password {
            assert!(s.starts_with("$y$"));
        } else {
            bail!("Wrong HashedPassword variant")
        };

        Ok(())
    }
}
