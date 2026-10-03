#[derive(Debug, Clone, Copy)]
pub(super) struct TickThrottler {
    increment_step: i32,
    threshold: i32,
    count: i32,
}

impl TickThrottler {
    pub(super) const fn new(increment_step: i32, threshold: i32) -> Self {
        Self {
            increment_step,
            threshold,
            count: 0,
        }
    }

    pub(super) const fn increment(&mut self) {
        self.count = self.count.wrapping_add(self.increment_step);
    }

    pub(super) const fn tick(&mut self) {
        if self.count > 0 {
            self.count -= 1;
        }
    }

    pub(super) const fn is_under_threshold(self) -> bool {
        self.threshold <= 0 || self.count < self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::TickThrottler;

    #[test]
    fn threshold_at_or_below_zero_disables_throttling() {
        let mut throttler = TickThrottler::new(20, -20);

        throttler.increment();
        throttler.increment();

        assert!(throttler.is_under_threshold());
    }

    #[test]
    fn increment_reaches_threshold_and_tick_decays() {
        let mut throttler = TickThrottler::new(20, 40);

        throttler.increment();
        assert!(throttler.is_under_threshold());

        throttler.increment();
        assert!(!throttler.is_under_threshold());

        throttler.tick();
        assert!(throttler.is_under_threshold());
    }
}
