// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! Segment planner: split `total` bytes into 1..=16 ranges.

/// A byte range `[start, end]` inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub index: usize,
    pub start: u64,
    pub end: u64,
}

impl Segment {
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}

/// Clamp requested connections to 1..=16.
pub fn clamp_connections(n: u64) -> u64 {
    n.clamp(1, 16)
}

/// Plan segments. Returns a single full-range segment when the total
/// is unknown or the server lacks Range support.
pub fn plan(total: Option<u64>, requested: u64, supports_ranges: bool) -> Vec<Segment> {
    let total = match total {
        Some(t) if t > 0 => t,
        _ => return vec![Segment { index: 0, start: 0, end: 0 }],
    };
    if !supports_ranges {
        return vec![Segment { index: 0, start: 0, end: total - 1 }];
    }
    let n = clamp_connections(requested).min(total);
    let base = total / n;
    let rem = total % n;
    let mut out = Vec::with_capacity(n as usize);
    let mut start = 0;
    for i in 0..n {
        let len = base + if i < rem { 1 } else { 0 };
        out.push(Segment { index: i as usize, start, end: start + len - 1 });
        start += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_split() {
        let segs = plan(Some(1000), 4, true);
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0], Segment { index: 0, start: 0, end: 249 });
        assert_eq!(segs[3], Segment { index: 3, start: 750, end: 999 });
        assert_eq!(segs.iter().map(|s| s.len()).sum::<u64>(), 1000);
    }

    #[test]
    fn uneven_split_covers_all() {
        let segs = plan(Some(100), 3, true);
        assert_eq!(segs.iter().map(|s| s.len()).sum::<u64>(), 100);
        assert_eq!(segs.last().unwrap().end, 99);
    }

    #[test]
    fn fallback_cases() {
        assert_eq!(plan(None, 8, true).len(), 1);
        assert_eq!(plan(Some(500), 8, false).len(), 1);
        assert_eq!(plan(Some(3), 8, true).len(), 3);
        assert_eq!(plan(Some(100), 99, true).len(), 16);
    }
}
