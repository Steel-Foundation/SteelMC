//! Builder for [`BlockPattern`].
//!
//! A pattern is a stack of "aisles" (Y layers). Each aisle is a list of strings:
//! chars index X, strings index Z. The first `aisle()` fixes width and depth;
//! subsequent calls add Y layers.

use rustc_hash::FxHashMap;

use super::{BlockPattern, BlockPredicate};

/// See module docs for the layer/aisle convention.
#[derive(Default)]
pub struct BlockPatternBuilder {
    aisles: Vec<Vec<String>>,
    symbols: FxHashMap<char, BlockPredicate>,
}

/// Errors produced by [`BlockPatternBuilder::build`].
#[derive(Debug, thiserror::Error)]
pub enum BlockPatternBuildError {
    /// No aisles were added.
    #[error("pattern has no aisles")]
    Empty,
    /// An aisle has different dimensions than the first aisle.
    #[error(
        "aisle {aisle} has inconsistent dimensions (expected {expected_w}x{expected_d}, got {actual_w}x{actual_d})"
    )]
    InconsistentDimensions {
        /// Y index of the offending aisle.
        aisle: usize,
        /// Expected width (X).
        expected_w: usize,
        /// Expected depth (Z).
        expected_d: usize,
        /// Actual width.
        actual_w: usize,
        /// Actual depth.
        actual_d: usize,
    },
    /// A char in an aisle has no registered symbol.
    #[error("char '{char}' (at aisle {aisle}, x={x}, z={z}) has no registered symbol")]
    UnknownSymbol {
        /// The unregistered char.
        char: char,
        /// Y index.
        aisle: usize,
        /// X index.
        x: usize,
        /// Z index.
        z: usize,
    },
}

impl BlockPatternBuilder {
    /// Creates an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one Y layer. Strings index Z; chars index X. All aisles must share dimensions.
    #[must_use]
    pub fn aisle(mut self, rows: &[&str]) -> Self {
        self.aisles
            .push(rows.iter().map(|r| (*r).to_string()).collect());
        self
    }

    /// Maps a char to a predicate.
    #[must_use]
    pub fn symbol(mut self, char: char, predicate: BlockPredicate) -> Self {
        self.symbols.insert(char, predicate);
        self
    }

    /// Validates and finalizes the pattern.
    ///
    /// # Errors
    /// See [`BlockPatternBuildError`].
    pub fn build(self) -> Result<BlockPattern, BlockPatternBuildError> {
        let first = self.aisles.first().ok_or(BlockPatternBuildError::Empty)?;
        let depth = first.len();
        let width = first.first().map_or(0, |s| s.chars().count());

        for (y, aisle) in self.aisles.iter().enumerate() {
            let aisle_d = aisle.len();
            let aisle_w = aisle.first().map_or(0, |s| s.chars().count());
            if aisle_d != depth || aisle.iter().any(|r| r.chars().count() != width) {
                return Err(BlockPatternBuildError::InconsistentDimensions {
                    aisle: y,
                    expected_w: width,
                    expected_d: depth,
                    actual_w: aisle_w,
                    actual_d: aisle_d,
                });
            }
        }

        let height = self.aisles.len();
        let mut cells: Vec<BlockPredicate> = Vec::with_capacity(width * height * depth);
        for (y, aisle) in self.aisles.iter().enumerate() {
            for (z, row) in aisle.iter().enumerate() {
                for (x, c) in row.chars().enumerate() {
                    cells.push(self.symbols.get(&c).cloned().ok_or(
                        BlockPatternBuildError::UnknownSymbol { char: c, aisle: y, x, z },
                    )?);
                }
            }
        }

        Ok(BlockPattern::from_raw(
            width as u32,
            height as u32,
            depth as u32,
            cells.into_boxed_slice(),
        ))
    }
}
