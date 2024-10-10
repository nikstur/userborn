use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{bail, Context, Result};

use crate::{fs::atomic_write, passwd::Passwd};

/// A locked and invalid password.
const PASSWORD_LOCKED_AND_INVALID: &str = "!*";

#[derive(Clone)]
pub struct Entry {
    name: String,
    /// The hashed password for the user.
    ///
    /// This password is in the format produced by libcxcrypt's `crypt(3)`.
    password: String,
    last_password_change: String,
    minimum_password_age: String,
    maximum_password_age: String,
    password_warning_period: String,
    password_inactivity_period: String,
    account_expiration_date: String,
    reserved: String,
}

impl Entry {
    /// Create a new /etc/shadow entry.
    pub fn new(name: String, hashed_password: Option<String>) -> Self {
        Self {
            name,
            password: hashed_password.unwrap_or(PASSWORD_LOCKED_AND_INVALID.into()),
            last_password_change: "1".into(),
            minimum_password_age: String::new(),
            maximum_password_age: String::new(),
            password_warning_period: String::new(),
            password_inactivity_period: String::new(),
            account_expiration_date: String::new(),
            reserved: String::new(),
        }
    }

    /// Update an /etc/shadow entry.
    pub fn update(&mut self, password: Option<String>) {
        if let Some(password) = password {
            if self.password != password {
                log::info!("Updating password of user {}...", self.name,);
                self.password = password;
            };
        };
    }

    /// Lock the account by resetting its password.
    ///
    /// After locking, a user will not be able to login with a unix password anymore.
    pub fn lock_account(&mut self) {
        self.password = PASSWORD_LOCKED_AND_INVALID.into();
    }

    /// Read an entry from a single line from /etc/shadow.
    ///
    /// Whenever a field in this line doesn't exist or cannot be parsed, returns `None`.
    fn from_line(line: &str) -> Option<Self> {
        if line.starts_with('#') {
            return None;
        }
        let mut fields = line.splitn(9, ':');
        Some(Self {
            name: fields.next()?.into(),
            password: fields.next()?.into(),
            last_password_change: fields.next()?.into(),
            minimum_password_age: fields.next()?.into(),
            maximum_password_age: fields.next()?.into(),
            password_warning_period: fields.next()?.into(),
            password_inactivity_period: fields.next()?.into(),
            account_expiration_date: fields.next()?.into(),
            reserved: fields.next()?.into(),
        })
    }

    fn to_line(&self) -> String {
        [
            self.name.as_str(),
            self.password.as_str(),
            self.last_password_change.as_str(),
            self.minimum_password_age.as_str(),
            self.maximum_password_age.as_str(),
            self.password_warning_period.as_str(),
            self.password_inactivity_period.as_str(),
            self.account_expiration_date.as_str(),
            self.reserved.as_str(),
        ]
        .join(":")
    }

    pub fn uses_secure_hash(&self) -> bool {
        password_hash_is_secure(&self.password)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn password(&self) -> &str {
        &self.password
    }
}

#[derive(Default)]
pub struct Shadow(BTreeMap<String, Entry>);

impl Shadow {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let file = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read {:?}.", path.as_ref()))?;

        Ok(Self::from_buffer(&file))
    }

    fn from_buffer(s: &str) -> Self {
        let mut entries = BTreeMap::new();
        for line in s.lines() {
            if let Some(e) = Entry::from_line(line) {
                entries.insert(e.name.clone(), e.clone());
            } else {
                log::warn!("Skipping shadow line because it cannot be parsed: {line}.");
            }
        }
        Self(entries)
    }

    /// Write the shadow database to a file.
    ///
    /// Sort the entries by their UIDs in the passwd database.
    pub fn to_file_sorted(&self, passwd: &Passwd, path: impl AsRef<Path>) -> Result<()> {
        atomic_write(path, self.to_buffer_sorted(passwd), 0o000)
    }

    /// Write the shadow database to a string buffer.
    ///
    /// Sort the entries by their UIDs in the passwd database.
    pub fn to_buffer_sorted(&self, passwd: &Passwd) -> String {
        let passwd_entries = passwd.entries();
        let mut s = String::new();

        for passwd_entry in passwd_entries {
            let name = passwd_entry.name();
            if let Some(shadow_entry) = self.get(name) {
                s.push_str(&shadow_entry.to_line());
                s.push('\n');
            } else {
                // This should only happen if the DB was somehow manually tampered with.
                log::warn!("Passwd DB contains entry for {name} that is not in Shadow DB");
            };
        }
        s
    }

    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.0.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Entry> {
        self.0.get_mut(name)
    }

    pub fn insert(&mut self, entry: &Entry) -> Result<()> {
        if self.0.contains_key(&entry.name) {
            bail!("User {} already exists in shadow database", entry.name);
        }

        self.0.entry(entry.name.clone()).or_insert(entry.clone());

        Ok(())
    }

    pub fn entries(&self) -> impl IntoIterator<Item = &Entry> {
        self.0.values()
    }

    pub fn entries_mut(&mut self) -> impl IntoIterator<Item = &mut Entry> {
        self.0.values_mut()
    }
}

/// Determine whether a hashing scheme used in a password is secure.
///
/// Hashing schemes are defined in `crypt(5)`.
///
/// Currently deemed secure schemes:
///
/// - yescrypt ("y")
/// - gost-yescrypt ("gy")
/// - scrypt ("7")
/// - bcrypt ("2b")
///
/// If the passed `password` is not a result of crypt(3), i.e. doens't start with `$`, it is deemed
/// "secure".
fn password_hash_is_secure(password: &str) -> bool {
    // If it's not a hashed password, it is secure.
    if !password.starts_with('$') {
        return true;
    }
    let mut split = password.split('$');
    split.next();
    if let Some(prefix) = split.next() {
        return matches!(prefix, "y" | "gy" | "7" | "2b");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    use expect_test::expect;
    use indoc::indoc;

    #[test]
    fn sort() {
        let passwd_buffer = indoc! {"
            nixbld5:x:5:5:::
            nixbld18:x:18:18:::
            root:x:0:0:::
            gary:x:1000:1000:::
        "};
        let passwd = Passwd::from_buffer(passwd_buffer);

        let buffer = indoc! {"
            nixbld5:!:1::::::
            nixbld18:!:1::::::
            root:$y$j9T$qG.o43YGDIMcN50nQGECv/$sYj8J9xpUsZ75SERZtY4.BMD8kuxXuAcc80L8v4UsI3:19911::::::
            gary:*:16034:0:99999:7:::
        "};
        let shadow = Shadow::from_buffer(buffer);
        let recreated_buffer = shadow.to_buffer_sorted(&passwd);

        let expected = expect![[r#"
            root:$y$j9T$qG.o43YGDIMcN50nQGECv/$sYj8J9xpUsZ75SERZtY4.BMD8kuxXuAcc80L8v4UsI3:19911::::::
            nixbld5:!:1::::::
            nixbld18:!:1::::::
            gary:*:16034:0:99999:7:::
        "#]];
        expected.assert_eq(&recreated_buffer);
    }

    #[test]
    fn skip_comments_and_broken_lines() {
        let passwd_buffer = indoc! {"
            root:x:0:0:::
        "};
        let passwd = Passwd::from_buffer(passwd_buffer);

        let buffer = indoc! {"
            root:$y$j9T$qG.o43YGDIMcN50nQGECv/$sYj8J9xpUsZ75SERZtY4.BMD8kuxXuAcc80L8v4UsI3:19911::::::
            # Comment
            d,smlfsd,füpdfm
        "};
        let shadow = Shadow::from_buffer(buffer);
        let recreated_buffer = shadow.to_buffer_sorted(&passwd);

        let expected = expect![[r"
            root:$y$j9T$qG.o43YGDIMcN50nQGECv/$sYj8J9xpUsZ75SERZtY4.BMD8kuxXuAcc80L8v4UsI3:19911::::::
        "]];
        expected.assert_eq(&recreated_buffer);
    }

    #[test]
    fn identify_secure_hashes() {
        let hashes = [
            ("$y$j9T$igJW2OgjsnJz4.COTGH0G1$TyS4WDmoXAGpE6z1iOl6ndQTKFgSsD8DIbC.mMdVtNC", true), // yescrypt
            ("$gy$j9T$IBb6Ykr9v.cfTkUvculya.$H/vfFxCd69T1CPtm0pkQT3VvzPSOTfUdo76Vf3hNqe2", true), // gost-yescrypt
            ("$7$CU..../....9sY.m8opwNYwaSsudXYhz1$7Ryf.TnjOFvBmzYvt7LLj30W3v48Ow9JpUMx3cA6x.5", true), // scrypt
            ("$2b$05$gCjYP/VsL/uwEc3HSNNSWepVe1YXminE0USq/9cLCAsapoLYfBgOy", true), // bcrypt
            ("$6$f9XzfdtqbfTpRNp6$j2731aaJDfI.SiStmiKkxC.zFbeeb9iBp.e4JHJ1PRAg0bgJPzklIcN8ZHquSzTtGYXxX/YgnZb3L655us6lV0", false), // sha512crypt
            ("!", true), // Not a password
            ("!*", true), // Not a password
            ("*", true), // Not a password
            ("!undefined", true), // Not a password
        ];

        for (hash, expected) in hashes {
            assert_eq!(password_hash_is_secure(hash), expected);
        }
    }
}
