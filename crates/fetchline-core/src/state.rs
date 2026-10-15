// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! state.json v1: serialize, validate, completion check.

use serde::{Deserialize, Serialize};

use crate::segments::Segment;

pub const STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentState {
    pub index: usize,
    pub start: u64,
    pub end: u64,
    pub done: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    pub version: u32,
    pub url: String,
    pub output: String,
    pub total_bytes: Option<u64>,
    pub connections: u64,
    pub segments: Vec<SegmentState>,
    pub etag: Option<String>,
    pub supports_ranges: bool,
    pub sha256: Option<String>,
}

impl DownloadState {
    pub fn fresh(
        url: &str,
        output: &str,
        total: Option<u64>,
        connections: u64,
        segments: &[Segment],
        etag: Option<String>,
        supports_ranges: bool,
    ) -> Self {
        Self {
            version: STATE_VERSION,
            url: url.to_string(),
            output: output.to_string(),
            total_bytes: total,
            connections,
            segments: segments
                .iter()
                .map(|s| SegmentState { index: s.index, start: s.start, end: s.end, done: 0 })
                .collect(),
            etag,
            supports_ranges,
            sha256: None,
        }
    }

    /// Validate loaded state against the spec. Rejects unknown versions,
    /// empty segments, and out-of-range counters.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != STATE_VERSION {
            return Err(format!("unsupported state version {}", self.version));
        }
        if self.segments.is_empty() {
            return Err("no segments".into());
        }
        for s in &self.segments {
            let len = s.end.saturating_sub(s.start) + 1;
            if s.done > len {
                return Err(format!("segment {} over-full", s.index));
            }
        }
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.segments
            .iter()
            .all(|s| s.done == s.end - s.start + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segments::plan;

    #[test]
    fn round_trip_and_complete() {
        let segs = plan(Some(1000), 4, true);
        let mut st = DownloadState::fresh("u", "o", Some(1000), 4, &segs, None, true);
        assert!(st.validate().is_ok());
        assert!(!st.is_complete());
        for s in &mut st.segments {
            s.done = s.end - s.start + 1;
        }
        assert!(st.is_complete());
        let json = serde_json::to_string(&st).unwrap();
        let back: DownloadState = serde_json::from_str(&json).unwrap();
        assert!(back.validate().is_ok() && back.is_complete());
    }

    #[test]
    fn rejects_bad_state() {
        let segs = plan(Some(100), 2, true);
        let mut st = DownloadState::fresh("u", "o", Some(100), 2, &segs, None, true);
        st.version = 99;
        assert!(st.validate().is_err());
    }
}
