use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path};

use anyhow::{Context, Result};

/// Atomicaly write a buffer into a file.
///
/// This will first write the buffer to the path with a `.tmp` suffix and then move the file to
/// it's actual path.
///
/// This increases the atomicity of the write.
pub fn atomic_write(path: impl AsRef<Path>, buffer: impl AsRef<[u8]>, mode: u32) -> Result<()> {
    let mut i = 0;

    let (mut file, tmp_path) = loop {
        let mut tmp_path = path.as_ref().as_os_str().to_os_string();
        tmp_path.push(format!(".tmp{i}"));

        let res = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .truncate(true)
            .mode(mode)
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
