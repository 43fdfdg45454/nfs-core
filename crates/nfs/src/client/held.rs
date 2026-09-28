//! The locks a file holds, as ranges: what a restarted server is asked to give back (reclaim).

use crate::ops::lock::LockKind;

/// Locked ranges, `start..end` (end u64::MAX: to the end of the file), none overlapping.
#[derive(Debug, Default)]
pub struct Held(Vec<(u64, u64, LockKind)>);

impl Held {
    /// `start..end` locked as `kind`, or unlocked with `None`: what overlaps it is cut.
    pub fn set(&mut self, start: u64, end: u64, kind: Option<LockKind>) {
        let mut kept = Vec::new();
        for &(s, e, k) in &self.0 {
            if e <= start || s >= end {
                kept.push((s, e, k));
                continue;
            }
            if s < start {
                kept.push((s, start, k));
            }
            if e > end {
                kept.push((end, e, k));
            }
        }
        kept.extend(kind.map(|k| (start, end, k)));
        self.0 = kept;
    }

    pub fn ranges(&self) -> Vec<(u64, u64, LockKind)> {
        self.0.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The end of a lock of `length` bytes from `offset` (length u64::MAX: to the end of the file).
pub fn end(offset: u64, length: u64) -> u64 {
    if length == u64::MAX { u64::MAX } else { offset.saturating_add(length) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LockKind::{Read, Write};

    #[test]
    fn unlocking_the_middle_splits_and_relocking_replaces() {
        let mut held = Held::default();
        held.set(0, 100, Some(Write));
        held.set(40, 60, None);
        assert_eq!(held.ranges(), vec![(0, 40, Write), (60, 100, Write)]);
        held.set(30, 70, Some(Read));
        assert_eq!(held.ranges(), vec![(0, 30, Write), (70, 100, Write), (30, 70, Read)]);
        held.set(0, u64::MAX, None);
        assert!(held.is_empty());
    }
}
