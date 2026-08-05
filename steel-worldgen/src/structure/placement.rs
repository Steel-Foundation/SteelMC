//! Structure placement. Determines which chunks are valid for structure
//! generation — vanilla's `StructurePlacement` hierarchy.

use glam::IVec3;
use rayon::prelude::*;
use steel_utils::BlockPos;
use steel_utils::ChunkPos;
use steel_utils::Identifier;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;

const PARALLEL_RING_SNAP_THRESHOLD: usize = 8;

/// How structures are spread within their grid cell. Vanilla's `RandomSpreadType`.
#[derive(Debug, Clone, Copy)]
pub enum SpreadType {
    /// Uniform random within range.
    Linear,
    /// Average of two uniform samples (center-biased).
    Triangular,
}

impl SpreadType {
    /// Offset in `[0, limit)`.
    pub fn evaluate(self, rng: &mut LegacyRandom, limit: i32) -> i32 {
        match self {
            Self::Linear => rng.next_i32_bounded(limit),
            // Vanilla: `(nextInt(limit) + nextInt(limit)) / 2`.
            #[expect(
                clippy::manual_midpoint,
                reason = "midpoint would change overflow vs vanilla"
            )]
            Self::Triangular => (rng.next_i32_bounded(limit) + rng.next_i32_bounded(limit)) / 2,
        }
    }
}

/// Vanilla's `StructurePlacement.FrequencyReductionMethod`. Variants differ in
/// seeding/RNG strategy for historical compatibility.
#[derive(Debug, Clone, Copy, Default)]
pub enum FrequencyReductionMethod {
    /// Seeds with salt, uses `next_f32`.
    #[default]
    Default,
    /// Pillager outpost legacy.
    LegacyType1,
    /// Hardcoded salt `10_387_320`.
    LegacyType2,
    /// Uses `next_f64` instead of `next_f32`.
    LegacyType3,
}

impl FrequencyReductionMethod {
    /// Args match vanilla's `FrequencyReducer`:
    /// `(levelSeed, placementSalt, chunkX, chunkZ, frequency)`.
    #[must_use]
    pub fn should_generate(
        self,
        seed: i64,
        salt: i32,
        source_x: i32,
        source_z: i32,
        probability: f32,
    ) -> bool {
        let mut rng = LegacyRandom::from_seed(0);
        match self {
            Self::Default => {
                rng.set_large_feature_with_salt(seed, salt, source_x, source_z);
                rng.next_f32() < probability
            }
            Self::LegacyType1 => {
                let cx = source_x >> 4;
                let cz = source_z >> 4;
                rng.set_seed(i64::from(cx ^ (cz << 4)) ^ seed);
                rng.next_i32();
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "vanilla: truncates reciprocal probability to i32 bound"
                )]
                let bound = (1.0_f32 / probability) as i32;
                rng.next_i32_bounded(bound) == 0
            }
            Self::LegacyType2 => {
                rng.set_large_feature_with_salt(seed, source_x, source_z, 10_387_320);
                rng.next_f32() < probability
            }
            Self::LegacyType3 => {
                rng.set_large_feature_seed(seed, source_x, source_z);
                rng.next_f64() < f64::from(probability)
            }
        }
    }
}

/// Vanilla's `StructurePlacement.ExclusionZone`.
#[derive(Debug, Clone)]
pub struct ExclusionZone {
    /// Structure set to check against.
    pub other_set: Identifier,
    /// Radius in chunks.
    pub chunk_count: i32,
}

/// Java's `Math.round(double)` — half-up toward +∞. Rust's `f64::round()` rounds
/// -0.5 → -1; Java rounds -0.5 → 0.
fn java_round(v: f64) -> i32 {
    (v + 0.5).floor() as i32
}

/// Vanilla's `ChunkGeneratorStructureState.generateRingPositions`. Positions in
/// chunk coords. If `snap_biome` is provided it gets `(block_x, block_z, &mut rng)`
/// and returns `Some((snapped_block_x, snapped_block_z))` to snap, or `None` to keep raw.
#[must_use]
pub fn generate_ring_positions<F>(
    seed: i64,
    distance: i32,
    spread: i32,
    count: i32,
    snap_biome: Option<&F>,
    thread_pool: &rayon::ThreadPool,
) -> Vec<ChunkPos>
where
    F: Fn(i32, i32, &mut LegacyRandom) -> Option<(i32, i32)> + Sync,
{
    use std::f64::consts::TAU;

    struct RingCandidate {
        initial_x: i32,
        initial_z: i32,
        forked: LegacyRandom,
    }

    fn map_candidate<F>(candidate: RingCandidate, snap: &F) -> ChunkPos
    where
        F: Fn(i32, i32, &mut LegacyRandom) -> Option<(i32, i32)> + Sync,
    {
        let RingCandidate {
            initial_x,
            initial_z,
            mut forked,
        } = candidate;

        // sectionToBlockCoord(x, 8) = x * 16 + 8; snap result is blocks → >> 4.
        if let Some((sx, sz)) = snap(initial_x * 16 + 8, initial_z * 16 + 8, &mut forked) {
            ChunkPos::new(sx >> 4, sz >> 4)
        } else {
            ChunkPos::new(initial_x, initial_z)
        }
    }

    if count == 0 {
        return vec![];
    }

    let mut rng = LegacyRandom::from_seed(seed as u64);
    let mut candidates = Vec::with_capacity(count as usize);
    let mut angle = rng.next_f64() * TAU;
    let mut spread = spread;
    let mut position_in_circle = 0;
    let mut circle = 0;

    let distance_f = f64::from(distance);

    for i in 0..count {
        let dist = (4.0 + 6.0 * f64::from(circle)) * distance_f
            + (rng.next_f64() - 0.5) * distance_f * 2.5;
        let initial_x = java_round(angle.cos() * dist);
        let initial_z = java_round(angle.sin() * dist);

        // Vanilla forks the RNG for async biome search; preserve the exact fork order,
        // then parallelize only the snap work over those precomputed forked states.
        candidates.push(RingCandidate {
            initial_x,
            initial_z,
            forked: rng.fork(),
        });

        angle += TAU / f64::from(spread);
        position_in_circle += 1;
        if position_in_circle == spread {
            circle += 1;
            position_in_circle = 0;
            spread += 2 * spread / (circle + 1);
            spread = spread.min(count - i);
            angle += rng.next_f64() * TAU;
        }
    }

    match snap_biome {
        Some(snap) if candidates.len() >= PARALLEL_RING_SNAP_THRESHOLD => {
            thread_pool.install(|| {
                candidates
                    .into_par_iter()
                    .map(|candidate| map_candidate(candidate, snap))
                    .collect()
            })
        }
        Some(snap) => candidates
            .into_iter()
            .map(|candidate| map_candidate(candidate, snap))
            .collect(),
        None => candidates
            .into_iter()
            .map(|candidate| ChunkPos::new(candidate.initial_x, candidate.initial_z))
            .collect(),
    }
}

/// Kind-specific placement parameters.
#[derive(Debug, Clone)]
pub enum PlacementKind {
    /// Vanilla's `RandomSpreadStructurePlacement`.
    RandomSpread {
        /// Chunk spacing between grid-cell centers.
        spacing: i32,
        /// Minimum chunk separation within a cell.
        separation: i32,
        /// Offset computation within the cell.
        spread_type: SpreadType,
    },
    /// Vanilla's `ConcentricRingsStructurePlacement` (strongholds).
    ConcentricRings {
        /// Base distance between rings (chunks).
        distance: i32,
        /// Positions per ring.
        spread: i32,
        /// Total structure positions.
        count: i32,
        /// Preferred snap biomes.
        preferred_biomes: Vec<Identifier>,
    },
}

/// Structure placement configuration.
#[derive(Debug, Clone)]
pub struct StructurePlacement {
    /// Unique seed modifier.
    pub salt: i32,
    /// Probability of generating on a placement-chunk. 1.0 = always.
    pub frequency: f32,
    /// Frequency-reduction method.
    pub frequency_reduction_method: FrequencyReductionMethod,
    /// Optional exclusion zone against another structure set.
    pub exclusion_zone: Option<ExclusionZone>,
    /// Block offset from the placement chunk used by `/locate`.
    pub locate_offset: IVec3,
    /// Kind-specific parameters.
    pub kind: PlacementKind,
}

impl StructurePlacement {
    /// Locate result block position for a valid placement chunk.
    #[must_use]
    pub const fn locate_pos(&self, chunk_pos: ChunkPos) -> BlockPos {
        BlockPos::new(
            chunk_pos.0.x * 16 + self.locate_offset.x,
            self.locate_offset.y,
            chunk_pos.0.y * 16 + self.locate_offset.z,
        )
    }

    /// Valid placement chunk + frequency check. For `ConcentricRings`,
    /// `ring_positions` must be pre-computed.
    #[must_use]
    pub fn is_structure_chunk(
        &self,
        seed: i64,
        source_x: i32,
        source_z: i32,
        ring_positions: Option<&[ChunkPos]>,
    ) -> bool {
        if !self.is_placement_chunk(seed, source_x, source_z, ring_positions) {
            return false;
        }
        if self.frequency < 1.0
            && !self.frequency_reduction_method.should_generate(
                seed,
                self.salt,
                source_x,
                source_z,
                self.frequency,
            )
        {
            return false;
        }
        true
    }

    fn is_placement_chunk(
        &self,
        seed: i64,
        source_x: i32,
        source_z: i32,
        ring_positions: Option<&[ChunkPos]>,
    ) -> bool {
        match &self.kind {
            PlacementKind::RandomSpread {
                spacing,
                separation,
                spread_type,
            } => {
                let potential = Self::get_potential_structure_chunk(
                    seed,
                    self.salt,
                    source_x,
                    source_z,
                    *spacing,
                    *separation,
                    *spread_type,
                );
                potential.0.x == source_x && potential.0.y == source_z
            }
            PlacementKind::ConcentricRings { .. } => ring_positions
                .is_some_and(|positions| positions.contains(&ChunkPos::new(source_x, source_z))),
        }
    }

    /// Deterministic structure chunk for the grid cell containing `(source_x, source_z)`.
    #[must_use]
    pub fn get_potential_structure_chunk(
        seed: i64,
        salt: i32,
        source_x: i32,
        source_z: i32,
        spacing: i32,
        separation: i32,
        spread_type: SpreadType,
    ) -> ChunkPos {
        let grid_x = source_x.div_euclid(spacing);
        let grid_z = source_z.div_euclid(spacing);

        let mut rng = LegacyRandom::from_seed(0);
        rng.set_large_feature_with_salt(seed, grid_x, grid_z, salt);

        let limit = spacing - separation;
        let spread_x = spread_type.evaluate(&mut rng, limit);
        let spread_z = spread_type.evaluate(&mut rng, limit);
        ChunkPos::new(grid_x * spacing + spread_x, grid_z * spacing + spread_z)
    }
}

/// A weighted entry in a structure set.
#[derive(Debug, Clone)]
pub struct StructureSelectionEntry {
    /// Structure id (e.g., `minecraft:village_plains`).
    pub structure: Identifier,
    /// Weight.
    pub weight: i32,
}

/// Vanilla's `StructureSet`: weighted structures + one placement.
#[derive(Debug, Clone)]
pub struct StructureSet {
    /// Weighted list of structures.
    pub structures: Vec<StructureSelectionEntry>,
    /// Placement strategy.
    pub placement: StructurePlacement,
}

use steel_registry::structure_set::{
    FrequencyMethodData, PlacementData, SpreadTypeData, StructureSetData,
};

impl From<SpreadTypeData> for SpreadType {
    fn from(data: SpreadTypeData) -> Self {
        match data {
            SpreadTypeData::Linear => Self::Linear,
            SpreadTypeData::Triangular => Self::Triangular,
        }
    }
}

impl From<FrequencyMethodData> for FrequencyReductionMethod {
    fn from(data: FrequencyMethodData) -> Self {
        match data {
            FrequencyMethodData::Default => Self::Default,
            FrequencyMethodData::LegacyType1 => Self::LegacyType1,
            FrequencyMethodData::LegacyType2 => Self::LegacyType2,
            FrequencyMethodData::LegacyType3 => Self::LegacyType3,
        }
    }
}

fn convert_structure_set(data: StructureSetData) -> (Identifier, StructureSet) {
    let structures = data
        .structures
        .into_iter()
        .map(|e| StructureSelectionEntry {
            structure: e.structure,
            weight: e.weight,
        })
        .collect();

    let placement = match data.placement {
        PlacementData::RandomSpread {
            spacing,
            separation,
            spread_type,
            salt,
            frequency,
            frequency_reduction_method,
            exclusion_zone,
            locate_offset,
        } => StructurePlacement {
            salt,
            frequency,
            frequency_reduction_method: frequency_reduction_method.into(),
            exclusion_zone: exclusion_zone.map(|ez| ExclusionZone {
                other_set: ez.other_set,
                chunk_count: ez.chunk_count,
            }),
            locate_offset,
            kind: PlacementKind::RandomSpread {
                spacing,
                separation,
                spread_type: spread_type.into(),
            },
        },
        PlacementData::ConcentricRings {
            distance,
            spread,
            count,
            preferred_biomes,
            salt,
            frequency,
            frequency_reduction_method,
            locate_offset,
        } => StructurePlacement {
            salt,
            frequency,
            frequency_reduction_method: frequency_reduction_method.into(),
            exclusion_zone: None,
            locate_offset,
            kind: PlacementKind::ConcentricRings {
                distance,
                spread,
                count,
                preferred_biomes,
            },
        },
    };

    (
        data.key,
        StructureSet {
            structures,
            placement,
        },
    )
}

/// Loads all vanilla structure sets from the generated registry data.
#[must_use]
pub fn load_vanilla_structure_sets() -> Vec<(Identifier, StructureSet)> {
    use steel_registry::vanilla_structure_sets;
    vanilla_structure_sets::vanilla_structure_sets()
        .into_iter()
        .map(convert_structure_set)
        .collect()
}

#[cfg(test)]
mod tests {
    use steel_utils::random::Random;

    use super::*;

    #[test]
    fn test_spread_type_linear() {
        let mut rng = LegacyRandom::from_seed(42);
        let result = SpreadType::Linear.evaluate(&mut rng, 26);
        // Just verify it's in range
        assert!((0..26).contains(&result));
    }

    #[test]
    fn test_spread_type_triangular() {
        let mut rng = LegacyRandom::from_seed(42);
        let result = SpreadType::Triangular.evaluate(&mut rng, 26);
        assert!((0..26).contains(&result));
    }

    #[test]
    fn test_village_placement_seed_0() {
        // Villages: spacing=34, separation=8, salt=10387312, LINEAR
        let placement = StructurePlacement {
            salt: 10_387_312,
            frequency: 1.0,
            frequency_reduction_method: FrequencyReductionMethod::Default,
            exclusion_zone: None,
            locate_offset: IVec3::ZERO,
            kind: PlacementKind::RandomSpread {
                spacing: 34,
                separation: 8,
                spread_type: SpreadType::Linear,
            },
        };

        // For seed=0, chunk (0,0): grid cell is (0,0)
        // setLargeFeatureWithSalt(0, 0, 0, 10387312)
        // result = 0 + 0 + 0 + 10387312 = 10387312
        let potential = StructurePlacement::get_potential_structure_chunk(
            0,
            10_387_312,
            0,
            0,
            34,
            8,
            SpreadType::Linear,
        );

        // Verify the potential chunk by computing manually
        let mut rng = LegacyRandom::from_seed(0);
        rng.set_large_feature_with_salt(0, 0, 0, 10_387_312);
        let spread_x = rng.next_i32_bounded(26); // 34 - 8
        let spread_z = rng.next_i32_bounded(26);
        assert_eq!(potential, ChunkPos::new(spread_x, spread_z));

        // The structure chunk for (0,0) should only match if we query the
        // exact potential position
        assert!(placement.is_structure_chunk(0, potential.0.x, potential.0.y, None));
        // Some other chunk in the same grid cell should NOT match (unless it
        // happens to be the potential chunk)
        if potential != ChunkPos::new(0, 0) {
            assert!(!placement.is_structure_chunk(0, 0, 0, None));
        }
    }

    #[test]
    fn test_negative_chunk_coords() {
        // Verify grid cell computation works for negative coordinates
        let potential_pos = StructurePlacement::get_potential_structure_chunk(
            0,
            10_387_312,
            -1,
            -1,
            34,
            8,
            SpreadType::Linear,
        );

        // Grid cell for -1 with spacing 34: div_euclid(-1, 34) = -1
        // So the grid cell starts at -1 * 34 = -34
        assert!(potential_pos.0.x >= -34 && potential_pos.0.x < -34 + 26);
        assert!(potential_pos.0.y >= -34 && potential_pos.0.y < -34 + 26);
    }

    #[test]
    fn test_frequency_reduction_skips_at_1() {
        let placement = StructurePlacement {
            salt: 12345,
            frequency: 1.0,
            frequency_reduction_method: FrequencyReductionMethod::Default,
            exclusion_zone: None,
            locate_offset: IVec3::ZERO,
            kind: PlacementKind::RandomSpread {
                spacing: 32,
                separation: 8,
                spread_type: SpreadType::Linear,
            },
        };

        // Find the potential chunk
        let potential = StructurePlacement::get_potential_structure_chunk(
            0,
            12345,
            0,
            0,
            32,
            8,
            SpreadType::Linear,
        );
        // With frequency=1.0, should always pass
        assert!(placement.is_structure_chunk(0, potential.0.x, potential.0.y, None));
    }

    #[test]
    fn test_load_vanilla_structure_sets() {
        let sets = load_vanilla_structure_sets();
        assert_eq!(sets.len(), 20);

        // Verify villages loaded correctly from datapack
        let (key, villages) = sets
            .iter()
            .find(|(k, _)| &*k.path == "villages")
            .expect("villages structure set must be present");
        assert_eq!(&*key.namespace, "minecraft");
        assert_eq!(villages.structures.len(), 5);
        if let PlacementKind::RandomSpread {
            spacing,
            separation,
            spread_type: _,
        } = &villages.placement.kind
        {
            assert_eq!(*spacing, 34);
            assert_eq!(*separation, 8);
        } else {
            panic!("Expected RandomSpread for villages");
        }
        assert_eq!(villages.placement.salt, 10_387_312);

        // Buried treasure is the vanilla placement that uses a non-zero locate offset.
        let (_, buried_treasures) = sets
            .iter()
            .find(|(k, _)| &*k.path == "buried_treasures")
            .expect("buried_treasures structure set must be present");
        assert_eq!(
            buried_treasures.placement.locate_offset,
            IVec3::new(9, 0, 9)
        );

        // Verify strongholds use ConcentricRings
        let (_, strongholds) = sets
            .iter()
            .find(|(k, _)| &*k.path == "strongholds")
            .expect("strongholds structure set must be present");
        assert!(matches!(
            strongholds.placement.kind,
            PlacementKind::ConcentricRings { .. }
        ));

        // Verify pillager outposts have exclusion zone
        let (_, outposts) = sets
            .iter()
            .find(|(k, _)| &*k.path == "pillager_outposts")
            .expect("pillager_outposts structure set must be present");
        let ez = outposts
            .placement
            .exclusion_zone
            .as_ref()
            .expect("pillager_outposts has an exclusion zone");
        assert_eq!(&*ez.other_set.path, "villages");
        assert_eq!(ez.chunk_count, 10);
    }

    #[test]
    fn test_concentric_rings_with_positions() {
        let placement = StructurePlacement {
            salt: 0,
            frequency: 1.0,
            frequency_reduction_method: FrequencyReductionMethod::Default,
            exclusion_zone: None,
            locate_offset: IVec3::ZERO,
            kind: PlacementKind::ConcentricRings {
                distance: 32,
                spread: 3,
                count: 128,
                preferred_biomes: vec![],
            },
        };

        let positions = vec![ChunkPos::new(10, 20), ChunkPos::new(-5, 15)];

        assert!(placement.is_structure_chunk(0, 10, 20, Some(&positions)));
        assert!(placement.is_structure_chunk(0, -5, 15, Some(&positions)));
        assert!(!placement.is_structure_chunk(0, 0, 0, Some(&positions)));

        // Without positions, always false
        assert!(!placement.is_structure_chunk(0, 10, 20, None));
    }

    #[test]
    fn test_generate_ring_positions_strongholds() {
        // Strongholds: distance=32, spread=3, count=128
        let thread_pool = rayon::ThreadPoolBuilder::default()
            .build()
            .expect("Couldn't create a new thread pool.");
        let positions = generate_ring_positions::<
            fn(i32, i32, &mut LegacyRandom) -> Option<(i32, i32)>,
        >(0, 32, 3, 128, None, &thread_pool);
        assert_eq!(positions.len(), 128);

        // First ring should be roughly 4*32 = 128 chunks from origin
        // (with some jitter)
        let first = positions[0];
        let dist = (f64::from(first.0.x).powi(2) + f64::from(first.0.y).powi(2)).sqrt();
        assert!(
            dist > 80.0 && dist < 200.0,
            "First stronghold at distance {dist}, expected ~128"
        );

        // All positions should be unique
        let mut unique = positions.clone();
        unique.sort_by_key(|p| (p.0.x, p.0.y));
        unique.dedup_by_key(|p| (p.0.x, p.0.y));
        assert_eq!(
            unique.len(),
            positions.len(),
            "Ring positions should be unique"
        );

        // Deterministic: same seed produces same positions
        let positions2 = generate_ring_positions::<
            fn(i32, i32, &mut LegacyRandom) -> Option<(i32, i32)>,
        >(0, 32, 3, 128, None, &thread_pool);
        assert_eq!(positions, positions2);
    }

    #[test]
    fn test_generate_ring_positions_zero_count() {
        let thread_pool = rayon::ThreadPoolBuilder::default()
            .build()
            .expect("Couldn't create a new thread pool.");
        let positions = generate_ring_positions::<
            fn(i32, i32, &mut LegacyRandom) -> Option<(i32, i32)>,
        >(0, 32, 3, 0, None, &thread_pool);
        assert!(positions.is_empty());
    }

    #[test]
    fn test_java_round() {
        assert_eq!(java_round(0.5), 1);
        assert_eq!(java_round(-0.5), 0);
        assert_eq!(java_round(1.5), 2);
        assert_eq!(java_round(-1.5), -1);
        assert_eq!(java_round(2.3), 2);
        assert_eq!(java_round(-2.3), -2);
    }
}
