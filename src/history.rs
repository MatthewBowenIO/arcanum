//! Fixed-capacity time series for plots.
//!
//! One sample per poll tick (default 1 s). Values are pushed with the
//! elapsed seconds since the first point so plots show real time.

use std::collections::VecDeque;

pub const CAPACITY: usize = 1800; // 30 min at 1 s

#[derive(Debug, Clone)]
pub struct Ring {
    t: VecDeque<f64>,
    v: VecDeque<f64>,
    t0: Option<f64>,
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}

impl Ring {
    pub fn new() -> Self {
        Self {
            t: VecDeque::with_capacity(CAPACITY),
            v: VecDeque::with_capacity(CAPACITY),
            t0: None,
        }
    }

    pub fn push(&mut self, elapsed_s: f64, value: f64) {
        let t = match self.t0 {
            Some(t0) => elapsed_s - t0,
            None => {
                self.t0 = Some(elapsed_s);
                0.0
            }
        };
        self.t.push_back(t);
        self.v.push_back(value);
        if self.t.len() > CAPACITY {
            self.t.pop_front();
            self.v.pop_front();
        }
    }

    /// Last `n` points as (t, v) pairs for plotting.
    pub fn tail(&self, n: usize) -> Vec<[f64; 2]> {
        let len = self.t.len();
        let start = len.saturating_sub(n);
        self.t
            .iter()
            .skip(start)
            .copied()
            .zip(self.v.iter().skip(start).copied())
            .map(|(t, v)| [t, v])
            .collect()
    }
}

/// Named collection of series owned by the UI.
#[derive(Default)]
pub struct Histories {
    map: std::collections::HashMap<String, Ring>,
    /// Seconds counter derived from snapshot uptime.
    elapsed: f64,
}

impl Histories {
    /// Record everything worth plotting from a snapshot.
    pub fn record(&mut self, uptime_ms: u128) {
        self.elapsed = uptime_ms as f64 / 1000.0;
    }

    fn ring(&mut self, key: &str) -> &mut Ring {
        self.map.entry(key.to_string()).or_default()
    }

    pub fn push(&mut self, key: &str, value: f64) {
        let elapsed = self.elapsed;
        self.ring(key).push(elapsed, value);
    }

    pub fn get(&self, key: &str) -> Option<&Ring> {
        self.map.get(key)
    }
}
