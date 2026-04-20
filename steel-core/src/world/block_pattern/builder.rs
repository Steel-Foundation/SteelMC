//! Builder for [`BlockPattern`].
//!
//! Patterns are described as a stack of "aisles" (Y layers). Each aisle is a
//! list of strings; chars in the string are X positions, strings within an
//! aisle are Z positions. The first `aisle()` call fixes width (X) and depth (Z);
//! every subsequent call adds another Y layer.
//!
//! Example (5×5 flat pattern, 1 Y layer):
//! ```ignore
//! BlockPatternBuilder::new()
//!     .aisle(&["?vvv?", ">???<", ">???<", ">???<", "?^^^?"])
//!     .symbol('?', BlockPredicate::Any)
//!     .symbol('v', frame_eye(Direction::North))
//!     // ...
//!     .build()
//!     .expect("valid pattern");
//! ```

use rustc_hash::FxHashMap;

use super::{BlockPattern, BlockPredicate};

/// Builder for [`BlockPattern`]. See module docs for the layer/aisle convention.
pub struct BlockPatternBuilder {
    /// Each aisle is a Y layer; outer Vec indexes Y, inner Vec indexes Z, String chars index X.
    aisles: Vec<Vec<String>>,
    /// Char → predicate map.
    symbols: FxHashMap<char, BlockPredicate>,
}

/// Errors that can occur while building a [`BlockPattern`].
#[derive(Debug, thiserror::Error)]
pub enum BlockPatternBuildError {
    /// No aisles were added before [`BlockPatternBuilder::build`].
    #[error("pattern has no aisles")]
    Empty,
    /// An aisle had inconsistent row widths or different depth than the first aisle.
    #[error(
        "aisle {aisle} has inconsistent dimensions (expected {expected_w}x{expected_d}, got {actual_w}x{actual_d})"
    )]
    InconsistentDimensions {
        /// Index of the offending aisle.
        aisle: usize,
        /// Expected width (X).
        expected_w: usize,
        /// Expected depth (Z).
        expected_d: usize,
        /// Actual width found.
        actual_w: usize,
        /// Actual depth found.
        actual_d: usize,
    },
    /// A char appeared in an aisle but no [`BlockPatternBuilder::symbol`] was registered for it.
    #[error("char '{char}' (at aisle {aisle}, x={x}, z={z}) has no registered symbol")]
    UnknownSymbol {
        /// The unregistered char.
        char: char,
        /// Y position of the offending cell.
        aisle: usize,
        /// X position of the offending cell.
        x: usize,
        /// Z position of the offending cell.
        z: usize,
    },
}

impl Default for BlockPatternBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockPatternBuilder {
    /// Creates an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            aisles: Vec::new(),
            symbols: FxHashMap::default(),
        }
    }

    /// Adds one Y layer to the pattern. Each string is a row along Z; chars are X positions.
    /// All aisles must have identical dimensions; this is validated at [`Self::build`] time.
    #[must_use]
    pub fn aisle(mut self, rows: &[&str]) -> Self {
        self.aisles
            .push(rows.iter().map(|r| (*r).to_string()).collect());
        self
    }

    /// Maps a char in the aisle grid to a predicate.
    #[must_use]
    pub fn symbol(mut self, char: char, predicate: BlockPredicate) -> Self {
        self.symbols.insert(char, predicate);
        self
    }

    /// Validates and finalizes the pattern.
    ///
    /// # Errors
    /// Returns [`BlockPatternBuildError`] if the pattern is empty, has inconsistent
    /// dimensions, or references an unregistered char.
    pub fn build(self) -> Result<BlockPattern, BlockPatternBuildError> {
        if self.aisles.is_empty() {
            return Err(BlockPatternBuildError::Empty);
        }

        let depth = self.aisles[0].len();
        let width = self.aisles[0].first().map(|s| s.chars().count()).unwrap_or(0);

        for (y, aisle) in self.aisles.iter().enumerate() {
            let aisle_d = aisle.len();
            let aisle_w = aisle.first().map(|s| s.chars().count()).unwrap_or(0);
            let consistent = aisle_d == depth
                && aisle.iter().all(|row| row.chars().count() == width);
            if !consistent {
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
                    let predicate = self.symbols.get(&c).cloned().ok_or(
                        BlockPatternBuildError::UnknownSymbol {
                            char: c,
                            aisle: y,
                            x,
                            z,
                        },
                    )?;
                    cells.push(predicate);
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
