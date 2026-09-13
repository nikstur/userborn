use std::collections::BTreeSet;

use anyhow::{Result, bail};

use crate::config::{IdRange, NORMAL_ID_MIN};

/// Allocate a new UID/GID.
///
/// Normal users/groups get an ID from `normal_range` (by default 1000 to 29999 inclusive).
///
/// System users/groups get an ID in the range from 1 to 999 (inclusive).
///
/// Fails if there are no unused IDs in the respective ranges.
pub fn allocate(
    already_allocated_ids: &BTreeSet<u32>,
    is_normal: bool,
    normal_range: IdRange,
) -> Result<u32> {
    if is_normal {
        for candidate in normal_range.min..=normal_range.max {
            if !already_allocated_ids.contains(&candidate) {
                return Ok(candidate);
            }
        }
    } else {
        for candidate in (1u32..NORMAL_ID_MIN).rev() {
            if !already_allocated_ids.contains(&candidate) {
                return Ok(candidate);
            }
        }
    }
    bail!("Failed to allocated new UID/GID")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_allocate_id(
        already_allocated_ids: impl IntoIterator<Item = u32>,
        is_normal: bool,
        expected: u32,
    ) -> Result<()> {
        check_allocate_id_in_range(
            already_allocated_ids,
            is_normal,
            IdRange::default(),
            expected,
        )
    }

    fn check_allocate_id_in_range(
        already_allocated_ids: impl IntoIterator<Item = u32>,
        is_normal: bool,
        normal_range: IdRange,
        expected: u32,
    ) -> Result<()> {
        let uids = already_allocated_ids.into_iter().collect::<BTreeSet<u32>>();
        let allocated = allocate(&uids, is_normal, normal_range)?;
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

    #[test]
    fn allocate_uid_custom_range() -> Result<()> {
        let range = IdRange {
            min: 30000,
            max: 30001,
        };
        check_allocate_id_in_range([1000], true, range, 30000)?;
        check_allocate_id_in_range([30000], true, range, 30001)?;
        assert!(check_allocate_id_in_range([30000, 30001], true, range, 0).is_err());
        // The custom range only applies to normal IDs.
        check_allocate_id_in_range([], false, range, 999)?;
        Ok(())
    }
}
