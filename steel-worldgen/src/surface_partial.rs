//! Partial evaluation of a surface rule below the preliminary surface.
//!
//! Below a column's preliminary surface level, most of a vanilla surface rule
//! is decided by the block's Y and its biome's `biome_is` memberships alone.
//! The transpiler emits the rule tree as [`PartialSurfaceRule`], keeping only
//! conditions decidable from those inputs; everything else is
//! [`PartialSurfaceCondition::Opaque`]. Resolving it per biome class and Y
//! tells the caller whether the full rule is needed for a block at all.

/// A surface rule tree reduced to what is decidable below the preliminary surface.
#[derive(Debug)]
pub enum PartialSurfaceRule {
    /// Place the rule's block state at this index.
    Block(usize),
    /// Children in order; the first that places a block wins.
    Sequence(&'static [Self]),
    /// Run the rule when the condition holds.
    Condition(PartialSurfaceCondition, &'static Self),
    /// A result that depends on more than Y and biome (e.g. badlands bands).
    Opaque,
}

/// A surface condition reduced to what is decidable below the preliminary surface.
#[derive(Debug)]
pub enum PartialSurfaceCondition {
    /// `block_y >= min_surface_level`: false for every block resolved here.
    AbovePreliminarySurface,
    /// Membership in the rule's `biome_is` set at this index.
    BiomeSet(usize),
    /// `vertical_gradient`: true at and below the first Y, false at and above
    /// the second, a positional random draw in between.
    VerticalGradient {
        /// Highest Y where the condition always holds.
        true_at_and_below: i32,
        /// Lowest Y where the condition never holds.
        false_at_and_above: i32,
    },
    /// `y_above` without stone depth or surface-depth scaling.
    YAtLeast(i32),
    /// Logical negation.
    Not(&'static Self),
    /// Depends on more than Y and biome (noise, stone depth, water, ...).
    Opaque,
}

/// What a surface rule does to a default block, when decidable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialSurfaceOutcome {
    /// The rule places nothing.
    Keep,
    /// The rule places the block state at this index.
    Place(usize),
    /// The full rule must run for this block.
    Evaluate,
}

impl PartialSurfaceRule {
    /// Resolves the rule for a block below the preliminary surface at `block_y`
    /// whose biome belongs to exactly the `biome_is` sets in `biome_set_mask`.
    ///
    /// Mirrors the generated rule's control flow: a sequence returns its first
    /// placing child, and an undecidable condition only matters when its body
    /// could place something.
    #[must_use]
    pub fn resolve(&self, block_y: i32, biome_set_mask: u64) -> PartialSurfaceOutcome {
        match self {
            Self::Block(index) => PartialSurfaceOutcome::Place(*index),
            Self::Sequence(rules) => rules
                .iter()
                .map(|rule| rule.resolve(block_y, biome_set_mask))
                .find(|outcome| *outcome != PartialSurfaceOutcome::Keep)
                .unwrap_or(PartialSurfaceOutcome::Keep),
            Self::Condition(condition, then_run) => {
                match condition.resolve(block_y, biome_set_mask) {
                    Some(false) => PartialSurfaceOutcome::Keep,
                    Some(true) => then_run.resolve(block_y, biome_set_mask),
                    None => match then_run.resolve(block_y, biome_set_mask) {
                        PartialSurfaceOutcome::Keep => PartialSurfaceOutcome::Keep,
                        _ => PartialSurfaceOutcome::Evaluate,
                    },
                }
            }
            Self::Opaque => PartialSurfaceOutcome::Evaluate,
        }
    }
}

impl PartialSurfaceCondition {
    /// `Some` when the condition is decided by `block_y` and the biome sets.
    #[must_use]
    pub fn resolve(&self, block_y: i32, biome_set_mask: u64) -> Option<bool> {
        match self {
            Self::AbovePreliminarySurface => Some(false),
            Self::BiomeSet(index) => Some(biome_set_mask & 1 << index != 0),
            Self::VerticalGradient {
                true_at_and_below,
                false_at_and_above,
            } => {
                if block_y <= *true_at_and_below {
                    Some(true)
                } else if block_y >= *false_at_and_above {
                    Some(false)
                } else {
                    None
                }
            }
            Self::YAtLeast(anchor) => Some(block_y >= *anchor),
            Self::Not(condition) => condition
                .resolve(block_y, biome_set_mask)
                .map(|value| !value),
            Self::Opaque => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PartialSurfaceCondition as C, PartialSurfaceOutcome as O, PartialSurfaceRule as R,
    };

    /// Shaped like the 26.x overworld rule's top level.
    static RULE: R = R::Sequence(&[
        R::Condition(
            C::VerticalGradient {
                true_at_and_below: -64,
                false_at_and_above: -59,
            },
            &R::Block(0),
        ),
        R::Condition(C::AbovePreliminarySurface, &R::Block(1)),
        R::Condition(
            C::BiomeSet(3),
            &R::Sequence(&[R::Condition(C::Opaque, &R::Block(2))]),
        ),
        R::Condition(
            C::VerticalGradient {
                true_at_and_below: 0,
                false_at_and_above: 8,
            },
            &R::Block(4),
        ),
    ]);

    #[test]
    fn undecidable_branches_force_evaluation_only_when_they_could_place() {
        let sulfur = 1 << 3;
        assert_eq!(RULE.resolve(-64, 0), O::Place(0));
        assert_eq!(RULE.resolve(-62, 0), O::Evaluate); // bedrock gradient draw
        assert_eq!(RULE.resolve(-30, 0), O::Place(4));
        assert_eq!(RULE.resolve(4, 0), O::Evaluate); // deepslate gradient draw
        assert_eq!(RULE.resolve(40, 0), O::Keep);
        assert_eq!(RULE.resolve(40, sulfur), O::Evaluate); // sulfur noise
        assert_eq!(RULE.resolve(-64, sulfur), O::Place(0)); // bedrock comes first
    }
}
