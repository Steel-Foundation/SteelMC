/// Parameters for creating a noise generator.
#[derive(Debug, Clone)]
pub struct NoiseParameters {
    /// The first octave level.
    pub first_octave: i32,
    /// Amplitude multipliers for each octave.
    pub amplitudes: Vec<f64>,
}

impl NoiseParameters {
    /// Create new noise parameters.
    #[must_use]
    pub const fn new(first_octave: i32, amplitudes: Vec<f64>) -> Self {
        Self {
            first_octave,
            amplitudes,
        }
    }
}

/// Rarity value mapper for cave generation.
///
/// Used at runtime by transpiled `WeirdScaledSampler` code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RarityValueMapper {
    /// Mapper type `"type_1"` for tunnels.
    Tunnels,
    /// Mapper type `"type_2"` for caves.
    Caves,
}

impl RarityValueMapper {
    /// Get the scaling factor for this mapper based on rarity value.
    ///
    /// From vanilla `NoiseRouterData.QuantizedSpaghettiRarity`.
    #[must_use]
    pub fn get_values(self, rarity: f64) -> f64 {
        match self {
            Self::Tunnels => {
                if rarity < -0.5 {
                    0.75
                } else if rarity < 0.0 {
                    1.0
                } else if rarity < 0.5 {
                    1.5
                } else {
                    2.0
                }
            }
            Self::Caves => {
                if rarity < -0.75 {
                    0.5
                } else if rarity < -0.5 {
                    0.75
                } else if rarity < 0.5 {
                    1.0
                } else if rarity < 0.75 {
                    2.0
                } else {
                    3.0
                }
            }
        }
    }
}
