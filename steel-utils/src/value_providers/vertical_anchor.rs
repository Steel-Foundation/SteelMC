use serde::{Deserialize, Deserializer, de::Error as _};

/// A vertical anchor resolving to a world Y coordinate given the dimension
/// bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAnchor {
    /// Absolute Y coordinate.
    Absolute(i32),
    /// `min_y + offset`.
    AboveBottom(i32),
    /// `min_y + height - 1 - offset` (i.e. `max_y - offset`).
    BelowTop(i32),
    /// `sea_level + offset`.
    RelativeToSeaLevel(i32),
}

impl VerticalAnchor {
    /// Resolve this anchor to a world Y coordinate.
    ///
    /// Matches vanilla's `VerticalAnchor.resolveY(WorldGenerationContext)`.
    #[must_use]
    pub const fn resolve_y(self, min_y: i32, height: i32) -> i32 {
        match self {
            Self::Absolute(y) => y,
            Self::AboveBottom(offset) => min_y + offset,
            Self::BelowTop(offset) => min_y + height - 1 - offset,
            Self::RelativeToSeaLevel(_) => {
                panic!("VerticalAnchor::RelativeToSeaLevel needs a sea level parameter")
            }
        }
    }
}

impl<'de> Deserialize<'de> for VerticalAnchor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            #[serde(default)]
            absolute: Option<i32>,
            #[serde(default)]
            above_bottom: Option<i32>,
            #[serde(default)]
            below_top: Option<i32>,
            #[serde(default)]
            relative_to_sea_level: Option<i32>,
        }
        let raw = Raw::deserialize(d)?;
        match (
            raw.absolute,
            raw.above_bottom,
            raw.below_top,
            raw.relative_to_sea_level,
        ) {
            (Some(y), None, None, None) => Ok(Self::Absolute(y)),
            (None, Some(o), None, None) => Ok(Self::AboveBottom(o)),
            (None, None, Some(o), None) => Ok(Self::BelowTop(o)),
            (None, None, None, Some(o)) => Ok(Self::RelativeToSeaLevel(o)),
            (None, None, None, None) => Err(D::Error::custom(
                "VerticalAnchor requires exactly one of absolute/above_bottom/below_top/relative_to_sea_level",
            )),
            _ => Err(D::Error::custom(
                "VerticalAnchor must have exactly one of absolute/above_bottom/below_top/relative_to_sea_level",
            )),
        }
    }
}
