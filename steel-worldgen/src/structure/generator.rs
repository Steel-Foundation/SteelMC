//! Shared structure placement/selection engine.

use std::{iter, slice};

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::REGISTRY;
use steel_registry::biome::BiomeRef;
use steel_registry::structure::StructureRef;
use steel_registry::template_pool::{TemplateData, TemplatePoolData};
use steel_registry::vanilla_template_pools::{vanilla_template_pools, vanilla_templates};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BlockPos, ChunkPos, Identifier};

use crate::biomes::BiomeSourceKind;
use crate::structure::desert_pyramid::DesertPyramidStructure;
use crate::structure::end_city::EndCityStructure;
use crate::structure::fortress::NetherFortressStructure;
use crate::structure::igloo::IglooStructure;
use crate::structure::jigsaw::JigsawStructure;
use crate::structure::jungle_temple::JungleTempleStructure;
use crate::structure::mansion::WoodlandMansionStructure;
use crate::structure::mineshaft::MineshaftStructure;
use crate::structure::nether_fossil::NetherFossilStructure;
use crate::structure::ocean_monument::OceanMonumentStructure;
use crate::structure::ocean_ruin::OceanRuinStructure;
use crate::structure::placement::{
    PlacementKind, StructurePlacement, StructureSelectionEntry, StructureSet,
    generate_ring_positions, load_vanilla_structure_sets,
};
use crate::structure::ruined_portal::RuinedPortalStructure;
use crate::structure::shipwreck::ShipwreckStructure;
use crate::structure::single_piece::BuriedTreasureStructure;
use crate::structure::stronghold::StrongholdStructure;
use crate::structure::swamp_hut::SwampHutStructure;
use crate::structure::{GenerationStub, Structure, StructureGenerationContext, StructureStart};

const VANILLA_FLAT_RING_POSITION_SEED: i64 = 0;

/// Biome operations needed while building `ChunkGeneratorStructureState`.
pub trait StructureBiomeProvider {
    /// Every biome this provider can produce.
    fn possible_biomes(&self) -> FxHashSet<Identifier>;

    /// Vanilla's `BiomeSource.findBiomeHorizontal(findClosest=false, skipSteps=1)`.
    fn find_biome_horizontal(
        &self,
        origin_x: i32,
        origin_z: i32,
        search_radius: i32,
        allowed: &dyn Fn(&Identifier) -> bool,
        rng: &mut LegacyRandom,
    ) -> Option<(i32, i32)>;
}

impl StructureBiomeProvider for BiomeSourceKind {
    fn possible_biomes(&self) -> FxHashSet<Identifier> {
        BiomeSourceKind::possible_biomes(self)
    }

    fn find_biome_horizontal(
        &self,
        origin_x: i32,
        origin_z: i32,
        search_radius: i32,
        allowed: &dyn Fn(&Identifier) -> bool,
        rng: &mut LegacyRandom,
    ) -> Option<(i32, i32)> {
        BiomeSourceKind::find_biome_horizontal(
            self,
            origin_x,
            origin_z,
            search_radius,
            &|biome| allowed(&biome.key),
            rng,
        )
    }
}

/// Fixed-biome provider used by flat generation settings.
pub struct FixedStructureBiomeProvider {
    biome: BiomeRef,
}

impl FixedStructureBiomeProvider {
    /// Creates a fixed-biome provider.
    #[must_use]
    pub const fn new(biome: BiomeRef) -> Self {
        Self { biome }
    }
}

impl StructureBiomeProvider for FixedStructureBiomeProvider {
    fn possible_biomes(&self) -> FxHashSet<Identifier> {
        FxHashSet::from_iter([self.biome.key.clone()])
    }

    fn find_biome_horizontal(
        &self,
        origin_x: i32,
        origin_z: i32,
        search_radius: i32,
        allowed: &dyn Fn(&Identifier) -> bool,
        rng: &mut LegacyRandom,
    ) -> Option<(i32, i32)> {
        if !allowed(&self.biome.key) {
            return None;
        }

        let noise_center_x = origin_x >> 2;
        let noise_center_z = origin_z >> 2;
        let noise_radius = search_radius >> 2;
        let mut result = None;
        let mut found = 0;
        for z in -noise_radius..=noise_radius {
            for x in -noise_radius..=noise_radius {
                if result.is_none() || rng.next_i32_bounded(found + 1) == 0 {
                    result = Some(((noise_center_x + x) << 2, (noise_center_z + z) << 2));
                }
                found += 1;
            }
        }
        result
    }
}

/// Runtime equivalent of vanilla's `ChunkGeneratorStructureState` plus structure
/// implementation dispatch.
pub struct StructureGenerator {
    seed: i64,
    structure_sets: Vec<(Identifier, StructureSet)>,
    structure_set_indices: FxHashMap<Identifier, usize>,
    structure_data: FxHashMap<Identifier, StructureRef>,
    ring_positions: FxHashMap<Identifier, Vec<ChunkPos>>,
    template_pools: FxHashMap<Identifier, TemplatePoolData>,
    templates: FxHashMap<Identifier, TemplateData>,
    structure_impls: FxHashMap<Identifier, Box<dyn Structure>>,
}

/// Runtime assets required by structure generation beyond the structure-set list.
///
/// Vanilla datapacks let structure sets, template pools, NBT templates, and
/// structure implementation dispatch vary together. Use
/// `StructureGenerator::vanilla_with_structure_sets` only when the set list is
/// custom but all other assets are still vanilla.
pub struct StructureGeneratorAssets {
    template_pools: FxHashMap<Identifier, TemplatePoolData>,
    templates: FxHashMap<Identifier, TemplateData>,
    structure_impls: FxHashMap<Identifier, Box<dyn Structure>>,
}

impl StructureGeneratorAssets {
    /// Creates an explicit structure asset bundle.
    #[must_use]
    pub fn new(
        template_pools: FxHashMap<Identifier, TemplatePoolData>,
        templates: FxHashMap<Identifier, TemplateData>,
        structure_impls: FxHashMap<Identifier, Box<dyn Structure>>,
    ) -> Self {
        Self {
            template_pools,
            templates,
            structure_impls,
        }
    }

    /// Creates an asset bundle from generated vanilla registries and built-in
    /// structure implementation dispatch.
    #[must_use]
    pub fn vanilla() -> Self {
        let template_pools: FxHashMap<_, _> = vanilla_template_pools()
            .into_iter()
            .map(|pool| (pool.key.clone(), pool))
            .collect();
        let templates: FxHashMap<_, _> = vanilla_templates().into_iter().collect();

        Self {
            template_pools,
            templates,
            structure_impls: vanilla_structure_impls(),
        }
    }
}

/// Search plan for vanilla `/locate structure` queries.
#[derive(Debug, Clone)]
pub struct StructureLocatePlan {
    seed: i64,
    placements: Vec<StructureLocatePlacement>,
}

/// A structure placement that can produce the requested structure.
#[derive(Debug, Clone)]
pub struct StructureLocatePlacement {
    placement: StructurePlacement,
    ring_positions: Option<Vec<ChunkPos>>,
}

/// Candidate chunk and locate position for a structure search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructureLocateCandidate {
    /// Chunk that must be generated through `StructureStarts`.
    pub chunk_pos: ChunkPos,
    /// Position reported if the structure is present.
    pub locate_pos: BlockPos,
    scan_id: usize,
    ring_distance_pos: BlockPos,
}

impl StructureLocatePlan {
    /// Returns `true` if this plan has no placements to scan.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.placements.is_empty()
    }

    /// Returns `true` if the plan has any random-spread placements.
    #[must_use]
    pub fn has_random_spread(&self) -> bool {
        self.placements.iter().any(|placement| {
            matches!(
                &placement.placement.kind,
                PlacementKind::RandomSpread { .. }
            )
        })
    }

    /// Ring-placement candidates ordered by vanilla's stronghold distance pre-check.
    #[must_use]
    pub fn ring_candidates(&self, origin: BlockPos) -> Vec<StructureLocateCandidate> {
        let mut candidates = Vec::new();
        for (scan_id, placement) in self.placements.iter().enumerate() {
            let Some(ring_positions) = &placement.ring_positions else {
                continue;
            };
            for &chunk_pos in ring_positions {
                candidates.push(StructureLocateCandidate::new_ring(
                    scan_id,
                    placement.placement.locate_pos(chunk_pos),
                    chunk_pos,
                ));
            }
        }
        candidates.sort_by_key(|candidate| candidate.ring_distance_squared(origin));
        candidates
    }

    /// Random-spread candidates on the square shell at `radius` around `origin`.
    ///
    /// This matches vanilla's `ChunkGenerator.getNearestGeneratedStructure` scan:
    /// each shell step moves by the placement spacing before resolving the
    /// potential structure chunk inside that placement cell.
    #[must_use]
    pub fn random_spread_candidates_at_radius(
        &self,
        origin: BlockPos,
        radius: i32,
    ) -> Vec<StructureLocateCandidate> {
        if radius < 0 {
            return Vec::new();
        }

        let chunk_origin_x = origin.0.x >> 4;
        let chunk_origin_z = origin.0.z >> 4;
        let mut candidates = Vec::new();

        for (scan_id, locate_placement) in self.placements.iter().enumerate() {
            let PlacementKind::RandomSpread {
                spacing,
                separation,
                spread_type,
            } = &locate_placement.placement.kind
            else {
                continue;
            };

            for x in -radius..=radius {
                let x_edge = x == -radius || x == radius;
                for z in -radius..=radius {
                    let z_edge = z == -radius || z == radius;
                    if !x_edge && !z_edge {
                        continue;
                    }

                    let sector_x = chunk_origin_x + *spacing * x;
                    let sector_z = chunk_origin_z + *spacing * z;
                    let chunk_pos = StructurePlacement::get_potential_structure_chunk(
                        self.seed,
                        locate_placement.placement.salt,
                        sector_x,
                        sector_z,
                        *spacing,
                        *separation,
                        *spread_type,
                    );
                    let candidate = StructureLocateCandidate::new(
                        scan_id,
                        locate_placement.placement.locate_pos(chunk_pos),
                        chunk_pos,
                    );
                    candidates.push(candidate);
                }
            }
        }

        candidates
    }
}

impl StructureLocateCandidate {
    const fn new(scan_id: usize, locate_pos: BlockPos, chunk_pos: ChunkPos) -> Self {
        Self {
            chunk_pos,
            locate_pos,
            scan_id,
            ring_distance_pos: locate_pos,
        }
    }

    const fn new_ring(scan_id: usize, locate_pos: BlockPos, chunk_pos: ChunkPos) -> Self {
        Self {
            chunk_pos,
            locate_pos,
            scan_id,
            ring_distance_pos: BlockPos::new(chunk_pos.0.x * 16 + 8, 32, chunk_pos.0.y * 16 + 8),
        }
    }

    /// Group id matching one structure placement scan.
    #[must_use]
    pub const fn scan_id(self) -> usize {
        self.scan_id
    }

    fn ring_distance_squared(&self, origin: BlockPos) -> i64 {
        squared_distance(self.ring_distance_pos, origin)
    }
}

/// Squared block distance using vanilla's three-dimensional `BlockPos.distSqr`.
#[must_use]
pub fn squared_distance(a: BlockPos, b: BlockPos) -> i64 {
    let dx = i64::from(a.0.x) - i64::from(b.0.x);
    let dy = i64::from(a.0.y) - i64::from(b.0.y);
    let dz = i64::from(a.0.z) - i64::from(b.0.z);
    dx * dx + dy * dy + dz * dz
}

fn validate_structure_sets(structure_sets: &[(Identifier, StructureSet)]) {
    for (set_key, set) in structure_sets {
        assert!(
            !set.structures.is_empty(),
            "Structure set {set_key} must have at least one structure"
        );
        for (entry_index, entry) in set.structures.iter().enumerate() {
            assert!(
                entry.weight > 0,
                "Structure set {set_key} entry {entry_index} has non-positive weight {}",
                entry.weight
            );
        }
        assert!(
            !(!set.placement.frequency.is_finite()
                || !(0.0..=1.0).contains(&set.placement.frequency)),
            "Structure set {set_key} has invalid placement frequency {}",
            set.placement.frequency
        );
        if let Some(exclusion) = &set.placement.exclusion_zone
            && exclusion.chunk_count < 0
        {
            panic!(
                "Structure set {set_key} has negative exclusion chunk_count {}",
                exclusion.chunk_count
            );
        }

        match &set.placement.kind {
            PlacementKind::RandomSpread {
                spacing,
                separation,
                ..
            } => {
                assert!(
                    *spacing > 0,
                    "Structure set {set_key} has non-positive spacing {spacing}"
                );
                assert!(
                    *separation >= 0,
                    "Structure set {set_key} has negative separation {separation}"
                );
                assert!(
                    spacing > separation,
                    "Structure set {set_key} has spacing {spacing} <= separation {separation}"
                );
            }
            PlacementKind::ConcentricRings {
                distance,
                spread,
                count,
                ..
            } => {
                assert!(
                    *distance > 0,
                    "Structure set {set_key} has non-positive ring distance {distance}"
                );
                assert!(
                    *spread > 0,
                    "Structure set {set_key} has non-positive ring spread {spread}"
                );
                assert!(
                    *count >= 0,
                    "Structure set {set_key} has negative ring count {count}"
                );
            }
        }
    }
}

fn validate_structure_assets(
    structure_sets: &[(Identifier, StructureSet)],
    structure_data: &FxHashMap<Identifier, StructureRef>,
    structure_impls: &FxHashMap<Identifier, Box<dyn Structure>>,
) {
    for (set_key, set) in structure_sets {
        for entry in &set.structures {
            let structure = structure_data.get(&entry.structure).unwrap_or_else(|| {
                panic!(
                    "Structure set {set_key} references unknown structure {}",
                    entry.structure
                )
            });
            assert!(
                structure_impls.contains_key(&structure.structure_type),
                "Structure set {set_key} references {} with unsupported structure type {}",
                structure.key,
                structure.structure_type
            );
        }
    }
}

impl StructureGenerator {
    /// Creates a structure generator over all vanilla structure sets.
    #[must_use]
    pub fn vanilla(seed: i64, biome_provider: &impl StructureBiomeProvider) -> Self {
        Self::vanilla_with_structure_sets(seed, biome_provider, load_vanilla_structure_sets())
    }

    /// Creates a generator over an explicit structure-set list while keeping all
    /// template pools, templates, and structure implementation dispatch vanilla.
    #[must_use]
    pub fn vanilla_with_structure_sets(
        seed: i64,
        biome_provider: &impl StructureBiomeProvider,
        structure_sets: Vec<(Identifier, StructureSet)>,
    ) -> Self {
        Self::with_assets_for_ring_seed(
            seed,
            seed,
            biome_provider,
            structure_sets,
            StructureGeneratorAssets::vanilla(),
        )
    }

    /// Creates a vanilla superflat structure generator.
    ///
    /// Vanilla superflat uses the level seed for random-spread placement and
    /// structure selection, but always seeds concentric-ring positions with
    /// `0L`.
    #[must_use]
    pub fn vanilla_flat_with_structure_sets(
        seed: i64,
        biome_provider: &impl StructureBiomeProvider,
        structure_sets: Vec<(Identifier, StructureSet)>,
    ) -> Self {
        Self::with_assets_for_ring_seed(
            seed,
            VANILLA_FLAT_RING_POSITION_SEED,
            biome_provider,
            structure_sets,
            StructureGeneratorAssets::vanilla(),
        )
    }

    /// Creates a generator from explicit structure sets and explicit runtime assets.
    #[must_use]
    pub fn with_assets(
        seed: i64,
        biome_provider: &impl StructureBiomeProvider,
        structure_sets: Vec<(Identifier, StructureSet)>,
        assets: StructureGeneratorAssets,
    ) -> Self {
        Self::with_assets_for_ring_seed(seed, seed, biome_provider, structure_sets, assets)
    }

    fn with_assets_for_ring_seed(
        seed: i64,
        ring_position_seed: i64,
        biome_provider: &impl StructureBiomeProvider,
        structure_sets: Vec<(Identifier, StructureSet)>,
        assets: StructureGeneratorAssets,
    ) -> Self {
        validate_structure_sets(&structure_sets);

        let structure_data: FxHashMap<Identifier, StructureRef> = REGISTRY
            .structures
            .iter()
            .map(|(_, structure)| (structure.key.clone(), structure))
            .collect();
        validate_structure_assets(&structure_sets, &structure_data, &assets.structure_impls);

        let possible_biomes = biome_provider.possible_biomes();
        let structure_sets: Vec<_> = structure_sets
            .into_iter()
            .filter(|(_, set)| {
                set.structures.iter().any(|entry| {
                    structure_data
                        .get(&entry.structure)
                        .is_some_and(|structure| {
                            structure.allowed_biomes.is_empty()
                                || structure
                                    .allowed_biomes
                                    .iter()
                                    .any(|biome| possible_biomes.contains(biome))
                        })
                })
            })
            .collect();

        let structure_set_indices: FxHashMap<Identifier, usize> = structure_sets
            .iter()
            .enumerate()
            .map(|(index, (key, _))| (key.clone(), index))
            .collect();

        let mut ring_positions = FxHashMap::default();
        for (key, set) in &structure_sets {
            if let PlacementKind::ConcentricRings {
                distance,
                spread,
                count,
                preferred_biomes,
            } = &set.placement.kind
            {
                let mut snap =
                    |block_x: i32, block_z: i32, rng: &mut LegacyRandom| -> Option<(i32, i32)> {
                        biome_provider.find_biome_horizontal(
                            block_x,
                            block_z,
                            112,
                            &|biome| preferred_biomes.contains(biome),
                            rng,
                        )
                    };
                let positions = generate_ring_positions(
                    ring_position_seed,
                    *distance,
                    *spread,
                    *count,
                    Some(&mut snap),
                );
                ring_positions.insert(key.clone(), positions);
            }
        }

        Self {
            seed,
            structure_sets,
            structure_set_indices,
            structure_data,
            ring_positions,
            template_pools: assets.template_pools,
            templates: assets.templates,
            structure_impls: assets.structure_impls,
        }
    }

    /// Template pool registry used by structure contexts.
    #[must_use]
    pub const fn template_pools(&self) -> &FxHashMap<Identifier, TemplatePoolData> {
        &self.template_pools
    }

    /// Structure templates used by structure contexts.
    #[must_use]
    pub const fn templates(&self) -> &FxHashMap<Identifier, TemplateData> {
        &self.templates
    }

    /// Builds a detached locate plan for one structure id.
    #[must_use]
    pub fn locate_plan_for_structure(&self, structure: &Identifier) -> Option<StructureLocatePlan> {
        self.locate_plan_for_structures(slice::from_ref(structure))
    }

    /// Builds a detached locate plan for one or more structure ids.
    #[must_use]
    pub fn locate_plan_for_structures(
        &self,
        structures: &[Identifier],
    ) -> Option<StructureLocatePlan> {
        let structures: FxHashSet<Identifier> = structures.iter().cloned().collect();
        let mut placements = Vec::new();
        for (set_key, set) in &self.structure_sets {
            if !set
                .structures
                .iter()
                .any(|entry| structures.contains(&entry.structure))
            {
                continue;
            }

            placements.push(StructureLocatePlacement {
                placement: set.placement.clone(),
                ring_positions: self.ring_positions.get(set_key).cloned(),
            });
        }

        (!placements.is_empty()).then_some(StructureLocatePlan {
            seed: self.seed,
            placements,
        })
    }

    /// Generates structure starts for one chunk.
    pub fn generate_starts_for_chunk(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        mut has_existing_start: impl FnMut(&Identifier) -> bool,
    ) -> Vec<StructureStart> {
        let chunk_x = ctx.chunk_x();
        let chunk_z = ctx.chunk_z();
        let mut generated_keys = FxHashSet::default();
        let mut starts = Vec::new();

        for (set_key, set) in &self.structure_sets {
            if !self.is_structure_chunk_for_set(set_key, chunk_x, chunk_z, &mut Vec::new()) {
                continue;
            }

            if set.structures.iter().any(|entry| {
                generated_keys.contains(&entry.structure) || has_existing_start(&entry.structure)
            }) {
                continue;
            }

            let Some((structure, stub)) = self.select_structure(set, ctx) else {
                continue;
            };

            let start = StructureStart::new(
                structure.key.clone(),
                ChunkPos::new(chunk_x, chunk_z),
                stub.pieces,
                structure.terrain_adjustment,
            );
            if !start.pieces.is_empty() {
                generated_keys.insert(structure.key.clone());
            }
            starts.push(start);
        }

        starts
    }

    fn rings_for_set(&self, set_key: &Identifier) -> Option<&[ChunkPos]> {
        self.ring_positions.get(set_key).map(Vec::as_slice)
    }

    fn is_structure_chunk_for_set(
        &self,
        set_key: &Identifier,
        source_x: i32,
        source_z: i32,
        stack: &mut Vec<Identifier>,
    ) -> bool {
        if stack.iter().any(|key| key == set_key) {
            let chain = stack
                .iter()
                .map(ToString::to_string)
                .chain(iter::once(set_key.to_string()))
                .collect::<Vec<_>>()
                .join(" -> ");
            panic!("Circular structure exclusion zone: {chain}");
        }

        let Some(&set_index) = self.structure_set_indices.get(set_key) else {
            return false;
        };
        let (_, set) = &self.structure_sets[set_index];
        let rings = self.rings_for_set(set_key);
        if !set
            .placement
            .is_structure_chunk(self.seed, source_x, source_z, rings)
        {
            return false;
        }

        stack.push(set_key.clone());
        let excluded = self.is_excluded(&set.placement, source_x, source_z, stack);
        stack.pop();
        !excluded
    }

    fn is_excluded(
        &self,
        placement: &StructurePlacement,
        source_x: i32,
        source_z: i32,
        stack: &mut Vec<Identifier>,
    ) -> bool {
        let Some(exclusion) = &placement.exclusion_zone else {
            return false;
        };

        for dx in (source_x - exclusion.chunk_count)..=(source_x + exclusion.chunk_count) {
            for dz in (source_z - exclusion.chunk_count)..=(source_z + exclusion.chunk_count) {
                if self.is_structure_chunk_for_set(&exclusion.other_set, dx, dz, stack) {
                    return true;
                }
            }
        }
        false
    }

    fn select_structure(
        &self,
        set: &StructureSet,
        ctx: &mut dyn StructureGenerationContext,
    ) -> Option<(StructureRef, GenerationStub)> {
        if set.structures.len() == 1 {
            return self.try_generate_entry(&set.structures[0], ctx);
        }

        let mut rng = LegacyRandom::from_seed(0);
        rng.set_large_feature_seed(self.seed, ctx.chunk_x(), ctx.chunk_z());

        let mut remaining: Vec<&StructureSelectionEntry> = set.structures.iter().collect();
        let mut total_weight: i32 = remaining.iter().map(|entry| entry.weight).sum();

        while !remaining.is_empty() {
            let mut choice = rng.next_i32_bounded(total_weight);
            let mut selected_idx = 0;
            for (idx, entry) in remaining.iter().enumerate() {
                choice -= entry.weight;
                if choice < 0 {
                    selected_idx = idx;
                    break;
                }
            }

            let candidate = remaining[selected_idx];
            if let Some(generated) = self.try_generate_entry(candidate, ctx) {
                return Some(generated);
            }

            total_weight -= candidate.weight;
            remaining.remove(selected_idx);
        }

        None
    }

    fn try_generate_entry(
        &self,
        entry: &StructureSelectionEntry,
        ctx: &mut dyn StructureGenerationContext,
    ) -> Option<(StructureRef, GenerationStub)> {
        let Some(structure) = self.structure_data.get(&entry.structure).copied() else {
            tracing::warn!("Missing structure registry data for {}", entry.structure);
            return None;
        };

        if let Some(structure_impl) = self.structure_impls.get(&structure.structure_type) {
            let mut rng = LegacyRandom::from_seed(0);
            rng.set_large_feature_seed(self.seed, ctx.chunk_x(), ctx.chunk_z());
            return structure_impl
                .find_generation_point(ctx, structure, &mut rng)
                .map(|stub| (structure, stub));
        }

        tracing::warn!(
            "Unknown structure type {:?} for {}, skipping structure start",
            structure.structure_type,
            structure.key
        );
        None
    }
}

fn vanilla_structure_impls() -> FxHashMap<Identifier, Box<dyn Structure>> {
    let mut structures: FxHashMap<Identifier, Box<dyn Structure>> = FxHashMap::default();
    let mut reg = |key: &'static str, structure: Box<dyn Structure>| {
        structures.insert(Identifier::vanilla_static(key), structure);
    };

    reg("jigsaw", Box::new(JigsawStructure));
    reg("nether_fossil", Box::new(NetherFossilStructure));
    reg("fortress", Box::new(NetherFortressStructure));
    reg("end_city", Box::new(EndCityStructure));
    reg("woodland_mansion", Box::new(WoodlandMansionStructure));
    reg("ocean_monument", Box::new(OceanMonumentStructure));
    reg("mineshaft", Box::new(MineshaftStructure));
    reg("desert_pyramid", Box::new(DesertPyramidStructure));
    reg("jungle_temple", Box::new(JungleTempleStructure));
    reg("swamp_hut", Box::new(SwampHutStructure));
    reg("buried_treasure", Box::new(BuriedTreasureStructure));
    reg("shipwreck", Box::new(ShipwreckStructure));
    reg("igloo", Box::new(IglooStructure));
    reg("ocean_ruin", Box::new(OceanRuinStructure));
    reg("stronghold", Box::new(StrongholdStructure));
    reg("ruined_portal", Box::new(RuinedPortalStructure));

    structures
}

#[cfg(test)]
mod tests {
    use steel_registry::{test_support::init_test_registry, vanilla_biomes};

    use crate::structure::placement::{
        ExclusionZone, FrequencyReductionMethod, PlacementKind, SpreadType,
    };

    use super::*;

    fn random_spread_plan(locate_offset: [i32; 3]) -> StructureLocatePlan {
        StructureLocatePlan {
            seed: 0,
            placements: vec![StructureLocatePlacement {
                placement: StructurePlacement {
                    salt: 10_387_312,
                    frequency: 1.0,
                    frequency_reduction_method: FrequencyReductionMethod::Default,
                    exclusion_zone: None,
                    locate_offset,
                    kind: PlacementKind::RandomSpread {
                        spacing: 32,
                        separation: 8,
                        spread_type: SpreadType::Linear,
                    },
                },
                ring_positions: None,
            }],
        }
    }

    fn every_chunk_placement(excludes: Option<Identifier>) -> StructurePlacement {
        StructurePlacement {
            salt: 0,
            frequency: 1.0,
            frequency_reduction_method: FrequencyReductionMethod::Default,
            exclusion_zone: excludes.map(|other_set| ExclusionZone {
                other_set,
                chunk_count: 0,
            }),
            locate_offset: [0, 0, 0],
            kind: PlacementKind::RandomSpread {
                spacing: 1,
                separation: 0,
                spread_type: SpreadType::Linear,
            },
        }
    }

    fn test_structure_set(placement: StructurePlacement) -> StructureSet {
        StructureSet {
            structures: vec![StructureSelectionEntry {
                structure: Identifier::new("test", "placeholder"),
                weight: 1,
            }],
            placement,
        }
    }

    fn generator_with_sets(sets: Vec<(Identifier, StructureSet)>) -> StructureGenerator {
        let structure_set_indices = sets
            .iter()
            .enumerate()
            .map(|(index, (key, _))| (key.clone(), index))
            .collect();

        StructureGenerator {
            seed: 0,
            structure_sets: sets,
            structure_set_indices,
            structure_data: FxHashMap::default(),
            ring_positions: FxHashMap::default(),
            template_pools: FxHashMap::default(),
            templates: FxHashMap::default(),
            structure_impls: FxHashMap::default(),
        }
    }

    #[test]
    fn vanilla_assets_cover_vanilla_structure_sets() {
        init_test_registry();
        let biome_provider = FixedStructureBiomeProvider::new(&vanilla_biomes::PLAINS);
        let _ = StructureGenerator::vanilla_with_structure_sets(
            0,
            &biome_provider,
            load_vanilla_structure_sets(),
        );
    }

    #[test]
    #[should_panic(expected = "non-positive spacing")]
    fn constructor_rejects_invalid_random_spread_spacing() {
        let biome_provider = FixedStructureBiomeProvider::new(&vanilla_biomes::PLAINS);
        let sets = vec![(
            Identifier::new("test", "invalid"),
            StructureSet {
                structures: vec![StructureSelectionEntry {
                    structure: Identifier::new("test", "placeholder"),
                    weight: 1,
                }],
                placement: StructurePlacement {
                    salt: 0,
                    frequency: 1.0,
                    frequency_reduction_method: FrequencyReductionMethod::Default,
                    exclusion_zone: None,
                    locate_offset: [0, 0, 0],
                    kind: PlacementKind::RandomSpread {
                        spacing: 0,
                        separation: 0,
                        spread_type: SpreadType::Linear,
                    },
                },
            },
        )];

        let _ = StructureGenerator::with_assets(
            0,
            &biome_provider,
            sets,
            StructureGeneratorAssets::new(
                FxHashMap::default(),
                FxHashMap::default(),
                FxHashMap::default(),
            ),
        );
    }

    #[test]
    fn exclusion_zone_checks_other_set_interactions() {
        let a_key = Identifier::new("test", "a");
        let b_key = Identifier::new("test", "b");
        let c_key = Identifier::new("test", "c");
        let generator = generator_with_sets(vec![
            (
                a_key.clone(),
                test_structure_set(every_chunk_placement(Some(b_key.clone()))),
            ),
            (
                b_key.clone(),
                test_structure_set(every_chunk_placement(Some(c_key.clone()))),
            ),
            (c_key, test_structure_set(every_chunk_placement(None))),
        ]);

        assert!(generator.is_structure_chunk_for_set(&a_key, 0, 0, &mut Vec::new()));
        assert!(!generator.is_structure_chunk_for_set(&b_key, 0, 0, &mut Vec::new()));
    }

    #[test]
    #[should_panic(expected = "Circular structure exclusion zone")]
    fn circular_exclusion_zones_fail_loudly() {
        let a_key = Identifier::new("test", "a");
        let b_key = Identifier::new("test", "b");
        let generator = generator_with_sets(vec![
            (
                a_key.clone(),
                test_structure_set(every_chunk_placement(Some(b_key.clone()))),
            ),
            (
                b_key,
                test_structure_set(every_chunk_placement(Some(a_key.clone()))),
            ),
        ]);

        let _ = generator.is_structure_chunk_for_set(&a_key, 0, 0, &mut Vec::new());
    }

    #[test]
    fn random_spread_candidates_follow_vanilla_shell_order() {
        let plan = random_spread_plan([0, 0, 0]);
        let origin = BlockPos::new(8, 64, 8);
        let candidates = plan.random_spread_candidates_at_radius(origin, 1);

        let expected: Vec<ChunkPos> = (-1..=1)
            .flat_map(|x| {
                (-1..=1).filter_map(move |z| {
                    let is_edge = x == -1 || x == 1 || z == -1 || z == 1;
                    is_edge.then(|| {
                        StructurePlacement::get_potential_structure_chunk(
                            0,
                            10_387_312,
                            x * 32,
                            z * 32,
                            32,
                            8,
                            SpreadType::Linear,
                        )
                    })
                })
            })
            .collect();

        let actual: Vec<ChunkPos> = candidates
            .iter()
            .map(|candidate| candidate.chunk_pos)
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn random_spread_candidates_use_locate_offset() {
        let plan = random_spread_plan([9, 0, 9]);
        let origin = BlockPos::new(0, 64, 0);
        let candidate = plan.random_spread_candidates_at_radius(origin, 0)[0];

        assert_eq!(
            candidate.locate_pos,
            BlockPos::new(
                candidate.chunk_pos.0.x * 16 + 9,
                0,
                candidate.chunk_pos.0.y * 16 + 9
            )
        );
    }

    #[test]
    fn ring_candidates_are_ordered_by_vanilla_distance_probe() {
        let plan = StructureLocatePlan {
            seed: 0,
            placements: vec![StructureLocatePlacement {
                placement: StructurePlacement {
                    salt: 0,
                    frequency: 1.0,
                    frequency_reduction_method: FrequencyReductionMethod::Default,
                    exclusion_zone: None,
                    locate_offset: [0, 0, 0],
                    kind: PlacementKind::ConcentricRings {
                        distance: 32,
                        spread: 3,
                        count: 2,
                        preferred_biomes: Vec::new(),
                    },
                },
                ring_positions: Some(vec![ChunkPos::new(10, 0), ChunkPos::new(1, 0)]),
            }],
        };

        let candidates = plan.ring_candidates(BlockPos::new(0, 64, 0));
        assert_eq!(candidates[0].chunk_pos, ChunkPos::new(1, 0));
        assert_eq!(candidates[1].chunk_pos, ChunkPos::new(10, 0));
    }
}
