//! Recent flight samples used to lag the dragon's neck, head and tail behind its body.

/// Number of samples retained. Mirrors vanilla `DragonFlightHistory.LENGTH`.
pub const LENGTH: usize = 64;
const MASK: i32 = 63;

/// One tick of flight history. Mirrors vanilla `DragonFlightHistory.Sample`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragonFlightSample {
    /// The dragon's height on that tick.
    pub y: f64,
    /// The dragon's yaw on that tick, in degrees.
    pub y_rot: f32,
}

/// A fixed ring of the dragon's recent height and yaw.
///
/// Mirrors vanilla `DragonFlightHistory`. The render-only members are dropped:
/// `copyFrom` serves the client's entity-replacement path, and the partial-ticks
/// `get` overload interpolates between samples for rendering.
#[derive(Debug, Clone)]
pub struct DragonFlightHistory {
    samples: [DragonFlightSample; LENGTH],
    /// Index of the newest sample, or negative before the first `record`.
    head: i32,
}

impl DragonFlightHistory {
    /// Creates an empty history.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            samples: [DragonFlightSample { y: 0.0, y_rot: 0.0 }; LENGTH],
            head: -1,
        }
    }

    /// Records this tick's height and yaw.
    ///
    /// The first sample fills the whole ring, so the tail and head offsets do not
    /// read zeroed history on the tick the dragon spawns.
    pub fn record(&mut self, y: f64, y_rot: f32) {
        let sample = DragonFlightSample { y, y_rot };
        if self.head < 0 {
            self.samples.fill(sample);
        }

        self.head += 1;
        if self.head == LENGTH as i32 {
            self.head = 0;
        }
        self.samples[self.head as usize] = sample;
    }

    /// Returns the sample recorded `delay` ticks ago.
    ///
    /// The index is masked rather than reduced, matching vanilla's
    /// `samples[head - delay & 63]`: for a negative operand Java's `&` wraps around
    /// the ring, where a remainder would not.
    #[must_use]
    pub const fn get(&self, delay: i32) -> DragonFlightSample {
        self.samples[((self.head - delay) & MASK) as usize]
    }
}

impl Default for DragonFlightHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{DragonFlightHistory, LENGTH};

    #[test]
    fn the_first_sample_fills_the_whole_ring() {
        let mut history = DragonFlightHistory::new();

        history.record(64.0, 90.0);

        // Otherwise the tail, which reads up to 16 ticks back, would trail through
        // zeroed history for the dragon's first second alive.
        for delay in 0..LENGTH as i32 {
            assert_eq!(history.get(delay).y, 64.0);
        }
    }

    #[test]
    fn get_walks_backwards_through_recent_samples() {
        let mut history = DragonFlightHistory::new();
        for tick in 0..5 {
            history.record(f64::from(tick), tick as f32);
        }

        assert_eq!(history.get(0).y, 4.0);
        assert_eq!(history.get(1).y, 3.0);
        assert_eq!(history.get(4).y, 0.0);
    }

    #[test]
    fn the_ring_wraps_by_mask_not_remainder() {
        let mut history = DragonFlightHistory::new();
        // Two full laps, so `head` is small while the reads run past zero.
        for tick in 0..(LENGTH * 2) {
            history.record(tick as f64, 0.0);
        }

        // `head` is now 63; reading 63 back must reach the oldest retained sample
        // rather than panicking on a negative index.
        assert_eq!(history.get(0).y, (LENGTH * 2 - 1) as f64);
        assert_eq!(history.get(63).y, (LENGTH * 2 - 64) as f64);
    }

    #[test]
    fn wrapping_reads_stay_in_bounds_right_after_a_lap() {
        let mut history = DragonFlightHistory::new();
        // `head` wraps to 0 here, so every non-zero delay goes negative before masking.
        for tick in 0..=LENGTH {
            history.record(tick as f64, 0.0);
        }

        assert_eq!(history.get(0).y, LENGTH as f64);
        assert_eq!(history.get(1).y, (LENGTH - 1) as f64);
    }
}
