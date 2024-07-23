use std::collections::BTreeSet;

use anyhow::{bail, Result};

/// Allocate a new UID/GID.
///
/// Normal users/groups get an ID in the range from 1000 to 29999 (inclusive).
///
/// System users/groups get an ID in the range from 1 to 999 (inclusive).
///
/// Fails if there are no unused IDs in the respective ranges.
pub fn allocate_id(already_allocated_ids: &BTreeSet<u32>, is_normal: bool) -> Result<u32> {
    if is_normal {
        for candidate in 1000u32..30000 {
            if !already_allocated_ids.contains(&candidate) {
                return Ok(candidate);
            }
        }
    } else {
        for candidate in (1u32..1000).rev() {
            if !already_allocated_ids.contains(&candidate) {
                return Ok(candidate);
            }
        }
    };
    bail!("Failed to allocated new UID")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_allocate_id(
        already_allocated_ids: impl IntoIterator<Item = u32>,
        is_normal_user: bool,
        expected: u32,
    ) -> Result<()> {
        let uids = already_allocated_ids.into_iter().collect::<BTreeSet<u32>>();
        let allocated = allocate_id(&uids, is_normal_user)?;
        assert_eq!(allocated, expected);
        Ok(())
    }

    #[test]
    fn allocate_uid_system() -> Result<()> {
        check_allocate_id([0, 999, 997], false, 998)?;
        check_allocate_id(2..1000, false, 1)?;
        assert!(check_allocate_id(1..1000, false, 1).is_err());
        Ok(())
    }

    #[test]
    fn allocate_uid_normal() -> Result<()> {
        // First UID should be 1000
        check_allocate_id([], true, 1000)?;
        assert!(check_allocate_id(999..30000, true, 1).is_err());
        Ok(())
    }
}
