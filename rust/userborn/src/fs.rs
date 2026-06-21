use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, chown},
    path::Path,
};

use anyhow::{Context, Result};

pub struct Rights {
    mode: u32,
    ownership: Option<Ownership>,
}

struct Ownership {
    uid: u32,
    gid: u32,
}

impl Rights {
    pub fn from_mode(mode: u32) -> Self {
        Self {
            mode,
            ownership: None,
        }
    }
}

/// Read a file to a string.
///
/// Returns the rights (mode, permissions) of this file.
///
/// This can then later be used to write the file with the same rights it used to have.
pub fn read_to_string(path: impl AsRef<Path>) -> Result<(String, Rights)> {
    let mut file = fs::File::open(path.as_ref())
        .with_context(|| format!("Failed to read {}.", path.as_ref().display()))?;

    let metadata = file
        .metadata()
        .with_context(|| format!("Failed to read metadata from {}.", path.as_ref().display()))?;
    let rights = Rights {
        mode: metadata.permissions().mode(),
        ownership: Some(Ownership {
            uid: metadata.uid(),
            gid: metadata.gid(),
        }),
    };

    let mut buf = String::new();
    let _ = file
        .read_to_string(&mut buf)
        .with_context(|| format!("Failed to read string from {}.", path.as_ref().display()))?;

    Ok((buf, rights))
}

/// Atomically write a buffer into a file.
///
/// This will first write the buffer to the path with a `.tmp` suffix and then move the file to
/// its actual path.
///
/// This increases the atomicity of the write.
pub fn atomic_write(
    path: impl AsRef<Path>,
    buffer: impl AsRef<[u8]>,
    rights: &Rights,
) -> Result<()> {
    let mut i = 0;

    let (mut file, tmp_path) = loop {
        let mut tmp_path = path.as_ref().as_os_str().to_os_string();
        tmp_path.push(format!(".tmp{i}"));

        let res = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .truncate(true)
            .mode(rights.mode)
            .open(&tmp_path);
        match res {
            Ok(file) => break (file, tmp_path),
            Err(err) => {
                if err.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(err).context(format!(
                        "Failed to open temporary file {}",
                        tmp_path.display()
                    ));
                }
            }
        }
        i += 1;
    };

    if let Some(ref ownership) = rights.ownership {
        chown(&tmp_path, Some(ownership.uid), Some(ownership.gid)).with_context(|| {
            format!("Failed to chmod the temporary file {}", tmp_path.display())
        })?;
    }
    file.write_all(buffer.as_ref())
        .with_context(|| format!("Failed to write to {}", tmp_path.display()))?;
    file.sync_all()
        .with_context(|| format!("Failed to sync the temporary file {}", tmp_path.display()))?;

    fs::rename(&tmp_path, &path).with_context(|| {
        format!(
            "Failed to rename {} to {}",
            tmp_path.display(),
            path.as_ref().display()
        )
    })?;

    Ok(())
}
