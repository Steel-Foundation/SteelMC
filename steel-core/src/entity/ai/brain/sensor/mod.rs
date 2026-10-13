//! Brain sensors: what a mob perceives, written into its memories.

#[cfg(test)]
mod tests;

use super::context::BrainContext;
use super::memory::MemoryModuleTypeRef;

/// Vanilla `Sensor.DEFAULT_SCAN_RATE`.
pub const DEFAULT_SCAN_RATE: i32 = 20;

/// A periodic scan that writes what the mob perceives into memory.
///
/// Mirrors vanilla `Sensor`.
pub trait Sensor: Send {
    /// The memories this sensor writes.
    fn requires(&self) -> &[MemoryModuleTypeRef] {
        &[]
    }

    /// How many ticks between scans.
    fn scan_rate(&self) -> i32 {
        DEFAULT_SCAN_RATE
    }

    /// Runs one scan.
    fn do_tick(&mut self, context: &mut BrainContext<'_>);
}

/// A sensor plus its countdown to the next scan.
pub struct WrappedSensor {
    sensor: Box<dyn Sensor>,
    scan_rate: i32,
    time_to_tick: i32,
}

impl WrappedSensor {
    /// Wraps `sensor` with a random offset into its first scan.
    ///
    /// Vanilla `Sensor.randomlyDelayStart`
    ///
    /// # Panics
    ///
    /// Panics if the sensor's scan rate is not positive.
    #[must_use]
    pub fn new(sensor: Box<dyn Sensor>) -> Self {
        let scan_rate = sensor.scan_rate();

        Self {
            scan_rate,
            time_to_tick: rand::random_range(0..scan_rate),
            sensor,
        }
    }

    /// The memories this sensor writes.
    #[must_use]
    pub fn requires(&self) -> &[MemoryModuleTypeRef] {
        self.sensor.requires()
    }

    /// Counts down, scanning when the countdown runs out.
    pub fn tick(&mut self, context: &mut BrainContext<'_>) {
        self.time_to_tick -= 1;
        if self.time_to_tick > 0 {
            return;
        }

        self.time_to_tick = self.scan_rate;
        self.sensor.do_tick(context);
    }
}
