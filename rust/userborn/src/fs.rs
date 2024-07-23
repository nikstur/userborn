use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path};

use anyhow::{Context, Result};

/// Atomicaly write a buffer into a file.
///
/// This will first write the buffer to the path with a `.tmp` suffix and then move the file to
/// it's actual path.
///
/// This increases the atomicity of the write.
pub fn atomic_write(path: impl AsRef<Path>, buffer: impl AsRef<[u8]>, mode: u32) -> Result<()> {
    let mut tmp_path = path.as_ref().as_os_str().to_os_string();
    tmp_path.push(".tmp");

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(&tmp_path)
        .with_context(|| format!("Failed to open {tmp_path:?}"))?;

    file.write_all(buffer.as_ref())
        .with_context(|| format!("Failed to write to {tmp_path:?}"))?;

    fs::rename(&tmp_path, &path)
        .with_context(|| format!("Failed to rename {tmp_path:?} to {:?}", path.as_ref()))?;

    Ok(())
}
