use std::{collections::BTreeMap, fmt::Write as _};

use anyhow::{Result, bail};

use crate::FromBuffer;

/// 31-bit ceiling on the subordinate id space.
///
/// Per <https://systemd.io/UIDS-GIDS/> ids above this are liable to hit signedness bugs in
/// various userspace tools, so the auto allocator never hands out a range that extends past it.
const SUBID_MAX: u64 = 0x8000_0000;

/// A half-open subordinate id interval `[start, start + count)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: u64,
    pub count: u64,
}

impl Range {
    fn end(self) -> u64 {
        self.start.saturating_add(self.count)
    }

    /// Two half-open intervals overlap iff each one's start lies strictly before the other's
    /// end.
    fn overlaps(self, other: Range) -> bool {
        self.start < other.end() && other.start < self.end()
    }
}

/// Borrowed view of a `sub{u,g}id` line: owner name plus range.
#[derive(Debug, Clone, Copy)]
pub struct EntryRef<'a> {
    /// Owner name, or a numeric uid rendered as a string (both valid per `subuid(5)`).
    pub name: &'a str,
    pub range: Range,
}

fn parse_line(line: &str) -> Option<(&str, Range)> {
    if line.starts_with('#') {
        return None;
    }
    let mut fields = line.splitn(3, ':');
    let name = fields.next()?;
    let range = Range {
        start: fields.next()?.parse().ok()?,
        count: fields.next()?.parse().ok()?,
    };
    Some((name, range))
}

/// The pair of subordinate id databases (`/etc/subuid` and `/etc/subgid`).
#[derive(Default)]
pub struct SubIds {
    pub uid: SubId,
    pub gid: SubId,
}

impl SubIds {
    /// Find an existing auto-allocated range for `name`.
    ///
    /// Auto ranges are written identically to both files, matching `usermod --add-subuids
    /// --add-subgids`. Looking in both lets us restore a range that survived in only one
    /// file instead of allocating a fresh one and breaking existing containers. If both are
    /// present but disagree we warn and prefer `subuid`.
    pub fn auto_range(&self, name: &str, count: u64) -> Option<Range> {
        let u = self.uid.auto_range(name, count);
        let g = self.gid.auto_range(name, count);
        if u.is_some() && g.is_some() && u != g {
            log::warn!(
                "Auto subordinate id range for {name} differs between subuid and subgid. \
                 Using the subuid range for both."
            );
        }
        u.or(g)
    }
}

/// In-memory representation of `/etc/subuid` or `/etc/subgid`.
///
/// Unlike the passwd/group databases an owner may legitimately have multiple entries, so this is
/// keyed by name to a list of ranges rather than a flat map.
///
/// Existing entries for owners not present in the config are preserved so that subordinate id
/// ranges can never be reassigned to a different owner across generations. This mirrors how
/// Userborn treats UIDs and GIDs.
#[derive(Default)]
pub struct SubId {
    entries: BTreeMap<String, Vec<Range>>,
}

impl SubId {
    /// Render the database in a stable order: by lowest start, then by name.
    ///
    /// This keeps the file byte-identical across runs as long as the set of ranges is unchanged,
    /// regardless of config iteration order.
    pub fn to_buffer(&self) -> String {
        let mut owners: Vec<_> = self.entries.iter().collect();
        owners.sort_by(|(an, ar), (bn, br)| {
            let amin = ar.iter().map(|r| r.start).min().unwrap_or(u64::MAX);
            let bmin = br.iter().map(|r| r.start).min().unwrap_or(u64::MAX);
            amin.cmp(&bmin).then_with(|| an.cmp(bn))
        });

        let mut out = String::new();
        for (name, ranges) in owners {
            let mut ranges = ranges.clone();
            ranges.sort_by_key(|r| r.start);
            for r in ranges {
                let _ = writeln!(out, "{name}:{}:{}", r.start, r.count);
            }
        }
        out
    }

    pub fn ranges(&self, name: &str) -> &[Range] {
        self.entries.get(name).map_or(&[], Vec::as_slice)
    }

    /// Replace all ranges for `name`.
    pub fn set(&mut self, name: &str, ranges: Vec<Range>) {
        if ranges.is_empty() {
            self.entries.remove(name);
        } else {
            self.entries.insert(name.to_owned(), ranges);
        }
    }

    /// Find an existing auto-style range (one with exactly the expected count) for `name`.
    pub fn auto_range(&self, name: &str, count: u64) -> Option<Range> {
        self.ranges(name).iter().find(|r| r.count == count).copied()
    }

    /// All `(owner, range)` pairs across the database.
    pub fn entries(&self) -> impl Iterator<Item = EntryRef<'_>> {
        self.entries
            .iter()
            .flat_map(|(name, ranges)| ranges.iter().map(move |r| EntryRef { name, range: *r }))
    }
}

impl FromBuffer for SubId {
    fn from_buffer(s: &str) -> Self {
        let mut entries: BTreeMap<String, Vec<Range>> = BTreeMap::new();
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some((name, range)) = parse_line(line) {
                entries.entry(name.to_owned()).or_default().push(range);
            } else {
                log::warn!("Skipping subid line because it cannot be parsed: {line}.");
            }
        }
        Self { entries }
    }
}

/// Allocate a `count`-wide range at or above `base` that does not overlap any of `occupied`.
///
/// `occupied` must be sorted by `start`. Duplicate entries are harmless.
pub fn allocate(base: u64, count: u64, occupied: &[Range]) -> Result<Range> {
    debug_assert!(occupied.is_sorted_by_key(|r| r.start));

    let mut start = base;
    for r in occupied {
        if r.start >= start.saturating_add(count) {
            // This and all later entries start after our probe ends.
            break;
        }
        if (Range { start, count }).overlaps(*r) {
            start = r.end();
        }
    }
    if start.checked_add(count).is_none_or(|end| end > SUBID_MAX) {
        bail!("No free {count}-wide subordinate id range available above {base}");
    }
    Ok(Range { start, count })
}

/// Returns the first pair of ranges that overlap across distinct owners, if any.
///
/// Ranges belonging to the same owner are allowed to overlap.
pub fn find_overlap<'a>(entries: &[EntryRef<'a>]) -> Option<(EntryRef<'a>, EntryRef<'a>)> {
    for (i, a) in entries.iter().enumerate() {
        for b in &entries[i + 1..] {
            if a.name != b.name && a.range.overlaps(b.range) {
                return Some((*a, *b));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use expect_test::expect;
    use indoc::indoc;

    fn range(start: u64, count: u64) -> Range {
        Range { start, count }
    }

    fn entry(name: &str, start: u64, count: u64) -> EntryRef<'_> {
        EntryRef {
            name,
            range: range(start, count),
        }
    }

    #[test]
    fn parse_roundtrip() {
        let buffer = indoc! {"
            alice:100000:65536
            bob:165536:65536
        "};
        let db = SubId::from_buffer(buffer);
        assert_eq!(
            db.ranges("alice"),
            &[Range {
                start: 100_000,
                count: 65536
            }]
        );
        let expected = expect![[r"
            alice:100000:65536
            bob:165536:65536
        "]];
        expected.assert_eq(&db.to_buffer());
    }

    #[test]
    fn skip_comments_and_broken_lines() {
        let buffer = indoc! {"
            # comment

            bad line
            alice:100000:65536
        "};
        let db = SubId::from_buffer(buffer);
        let expected = expect![[r"
            alice:100000:65536
        "]];
        expected.assert_eq(&db.to_buffer());
    }

    #[test]
    fn allocate_skips_occupied() {
        // A huge root range as commonly configured for incus.
        let occ = [range(1_000_000, 1_000_000_000)];
        let r = allocate(100_000, 65536, &occ);
        assert_eq!(r.ok().map(|r| r.start), Some(100_000));
        // Base inside the huge root range -> jumps past it.
        let r = allocate(2_000_000, 65536, &occ);
        assert_eq!(r.ok().map(|r| r.start), Some(1_001_000_000));
    }

    #[test]
    fn allocate_packs_after_existing() {
        let occ = [range(100_000, 65536), range(165_536, 65536)];
        let r = allocate(100_000, 65536, &occ);
        assert_eq!(r.ok().map(|r| r.start), Some(231_072));
    }

    #[test]
    fn allocate_with_base_past_early_entries() {
        // The early-exit must not fire on the first (already-past) entry,
        // because the second one still overlaps the base.
        let occ = [range(100_000, 65536), range(900_000, 200_000)];
        let r = allocate(1_000_000, 65536, &occ);
        assert_eq!(r.ok().map(|r| r.start), Some(1_100_000));
    }

    #[test]
    fn allocate_exhausted() {
        let occ = [range(0, SUBID_MAX)];
        assert!(allocate(100_000, 65536, &occ).is_err());
    }

    #[test]
    fn overlap_detected() {
        let occ = [entry("a", 100_000, 65536), entry("b", 100_100, 65536)];
        assert!(find_overlap(&occ).is_some());
    }

    #[test]
    fn overlap_same_owner_ignored() {
        let occ = [entry("a", 100_000, 65536), entry("a", 100_100, 65536)];
        assert!(find_overlap(&occ).is_none());
    }
}
