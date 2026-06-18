/// Generates vanilla container IDs in the inclusive range 1..=100.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ContainerCounter {
    current: u8,
}

impl ContainerCounter {
    #[must_use]
    pub(super) const fn new() -> Self {
        Self { current: 0 }
    }

    pub(super) const fn next(&mut self) -> u8 {
        self.current = (self.current % 100) + 1;
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::ContainerCounter;

    #[test]
    fn starts_at_one() {
        let mut counter = ContainerCounter::new();

        assert_eq!(counter.next(), 1);
    }

    #[test]
    fn wraps_after_one_hundred() {
        let mut counter = ContainerCounter::new();

        for expected in 1..=100 {
            assert_eq!(counter.next(), expected);
        }
        assert_eq!(counter.next(), 1);
    }
}
