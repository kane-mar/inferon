// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

//! Named timings for the proxy's pre-dispatch work (ITEM-092 AC5).
//!
//! `proxy_us` says *how long* the proxy took; this says *where*. A regression
//! then names the stage instead of leaving "it got slower" as the whole story.
//!
//! The marks are cheap (one `Instant::now` each) and only ever read at debug
//! level, so they cost nothing on the hot path's output.

use std::time::Instant;

/// Accumulating, named stage timings.
pub struct Stages {
    last: Instant,
    marks: Vec<(&'static str, u64)>,
}

impl Default for Stages {
    fn default() -> Self {
        Self::new()
    }
}

impl Stages {
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
            marks: Vec::new(),
        }
    }

    /// Close the current stage and open the next one.
    pub fn mark(&mut self, name: &'static str) {
        let now = Instant::now();
        let micros = now.duration_since(self.last).as_micros() as u64;
        self.last = now;
        match self.marks.iter_mut().find(|(known, _)| *known == name) {
            Some((_, total)) => *total += micros,
            None => self.marks.push((name, micros)),
        }
    }

    pub fn get(&self, name: &str) -> Option<u64> {
        self.marks
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, micros)| *micros)
    }

    /// Everything attributed so far.
    pub fn total_us(&self) -> u64 {
        self.marks.iter().map(|(_, micros)| *micros).sum()
    }

    /// `auth=12µs policy=3µs route=41µs`.
    pub fn summary(&self) -> String {
        if self.marks.is_empty() {
            return "no stages".to_string();
        }
        self.marks
            .iter()
            .map(|(name, micros)| format!("{name}={micros}µs"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests;
