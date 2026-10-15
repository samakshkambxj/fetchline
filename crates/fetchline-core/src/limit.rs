// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! Token-bucket speed limiter shared across segment threads.
//!
//! `0` means unlimited. Bursts up to one full second of the rate.

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Limiter {
    bytes_per_sec: u64,
    state: Mutex<(f64, Instant)>,
}

impl Limiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        Self { bytes_per_sec, state: Mutex::new((bytes_per_sec as f64, Instant::now())) }
    }

    /// Block until `n` bytes are allowed. Cheap when under budget.
    pub fn consume(&self, n: u64) {
        if self.bytes_per_sec == 0 || n == 0 {
            return;
        }
        let rate = self.bytes_per_sec as f64;
        loop {
            let wait = {
                let mut s = self.state.lock().unwrap();
                let now = Instant::now();
                let elapsed = (now - s.1).as_secs_f64();
                s.0 = (s.0 + elapsed * rate).min(rate);
                s.1 = now;
                if s.0 >= n as f64 {
                    s.0 -= n as f64;
                    return;
                }
                (n as f64 - s.0) / rate
            };
            std::thread::sleep(Duration::from_secs_f64(wait.min(0.05)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_never_blocks() {
        let lim = Limiter::new(0);
        let t = Instant::now();
        lim.consume(10_000_000);
        assert!(t.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn under_budget_is_fast() {
        let lim = Limiter::new(1_000_000);
        let t = Instant::now();
        lim.consume(100);
        assert!(t.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn over_budget_throttles() {
        let lim = Limiter::new(10_000);
        lim.consume(10_000); // drain burst
        let t = Instant::now();
        lim.consume(10_000); // needs ~1s of refill
        assert!(t.elapsed() >= Duration::from_millis(500));
    }
}
