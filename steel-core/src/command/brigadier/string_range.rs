//! UTF-16 command input ranges.

/// A half-open range measured in UTF-16 code units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StringRange {
    start: usize,
    end: usize,
}

impl StringRange {
    /// Creates an empty range at `position`.
    pub(crate) const fn at(position: usize) -> Self {
        Self {
            start: position,
            end: position,
        }
    }

    /// Creates a range between two UTF-16 positions.
    pub(crate) const fn between(start: usize, end: usize) -> Self {
        assert!(start <= end, "command range start must not exceed its end");
        Self { start, end }
    }

    /// Creates the smallest range containing both inputs.
    pub(crate) const fn encompassing(first: Self, second: Self) -> Self {
        Self {
            start: if first.start < second.start {
                first.start
            } else {
                second.start
            },
            end: if first.end > second.end {
                first.end
            } else {
                second.end
            },
        }
    }

    /// Returns the inclusive start position.
    pub(crate) const fn start(self) -> usize {
        self.start
    }

    /// Returns the exclusive end position.
    pub(crate) const fn end(self) -> usize {
        self.end
    }

    /// Returns whether the range contains no UTF-16 code units.
    pub(crate) const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Returns the range length in UTF-16 code units.
    pub(crate) const fn len(self) -> usize {
        self.end - self.start
    }
}
