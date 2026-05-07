//! Shared structure placement/selection engine.

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::REGISTRY;
use steel_registry::biome::BiomeRef;
use steel_registry::structure::StructureRef;
use steel_registry::template_pool::{TemplateData, TemplatePoolData};
use steel_registry::vanilla_template_pools::{vanilla_template_pools, vanilla_templates};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{ChunkPos, Identifier};

use crate::chunk::chunk_access::ChunkAccess;
use crate::world::structure::end_city::EndCityStructure;
use crate::world::structure::fortress::NetherFortressStructure;
use crate::world::structure::igloo::IglooStructure;
use crate::world::structure::jigsaw::JigsawStructure;
use crate::world::structure::mansion::WoodlandMansionStructure;
use crate::world::structure::mineshaft::MineshaftStructure;
use crate::world::structure::nether_fossil::NetherFossilStructure;
use crate::world::structure::ocean_monument::OceanMonumentStructure;
use crate::world::structure::ocean_ruin::OceanRuinStructure;
use crate::world::structure::placement::{
    PlacementKind, StructureSelectionEntry, StructureSet, generate_ring_positions,
    load_vanilla_structure_sets,
};
use crate::world::structure::ruined_portal::RuinedPortalStructure;
use crate::world::structure::shipwreck::ShipwreckStructure;
use crate::world::structure::single_piece::{BuriedTreasureStructure, SinglePieceStructure};
use crate::world::structure::stronghold::StrongholdStructure;
use crate::world::structure::{
    GenerationStub, Structure, StructureGenerationContext, StructureStart,
};
use crate::worldgen::BiomeSourceKind;

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
    structure_data: FxHashMap<Identifier, StructureRef>,
    ring_positions: Vec<(Identifier, Vec<ChunkPos>)>,
    template_pools: FxHashMap<Identifier, TemplatePoolData>,
    templates: FxHashMap<Identifier, TemplateData>,
    structures: FxHashMap<Identifier, Box<dyn Structure>>,
}

impl StructureGenerator {
    /// Creates a structure generator over all vanilla structure sets.
    #[must_use]
    pub fn vanilla(seed: i64, biome_provider: &impl StructureBiomeProvider) -> Self {
        Self::new(seed, biome_provider, load_vanilla_structure_sets())
    }

    /// Creates a structure generator over an explicit structure-set list.
    #[must_use]
    pub fn new(
        seed: i64,
        biome_provider: &impl StructureBiomeProvider,
        structure_sets: Vec<(Identifier, StructureSet)>,
    ) -> Self {
        let structure_data: FxHashMap<Identifier, StructureRef> = REGISTRY
            .structures
            .iter()
            .map(|(_, structure)| (structure.key.clone(), structure))
            .collect();

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

        let mut ring_positions = Vec::new();
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
                let positions =
                    generate_ring_positions(seed, *distance, *spread, *count, Some(&mut snap));
                ring_positions.push((key.clone(), positions));
            }
        }

        let template_pools: FxHashMap<_, _> = vanilla_template_pools()
            .into_iter()
            .map(|pool| (pool.key.clone(), pool))
            .collect();
        let templates: FxHashMap<_, _> = vanilla_templates().into_iter().collect();

        Self {
            seed,
            structure_sets,
            structure_data,
            ring_positions,
            template_pools,
            templates,
            structures: vanilla_structure_impls(),
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

    /// Generates structure starts for one chunk.
    pub fn create_structures(&self, chunk: &ChunkAccess, ctx: &mut dyn StructureGenerationContext) {
        let chunk_x = ctx.chunk_x();
        let chunk_z = ctx.chunk_z();

        for (set_key, set) in &self.structure_sets {
            let rings = self
                .ring_positions
                .iter()
                .find(|(key, _)| key == set_key)
                .map(|(_, positions)| positions.as_slice());

            if !set
                .placement
                .is_structure_chunk(self.seed, chunk_x, chunk_z, rings)
            {
                continue;
            }

            {
                let starts = chunk.structure_starts();
                if set.structures.iter().any(|entry| {
                    starts
                        .get(&entry.structure)
                        .is_some_and(|start| !start.pieces.is_empty())
                }) {
                    continue;
                }
            }

            if set.placement.is_excluded(
                self.seed,
                chunk_x,
                chunk_z,
                &self.structure_sets,
                &self.ring_positions,
            ) {
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
            chunk
                .structure_starts_mut()
                .insert(structure.key.clone(), start);
        }
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

        if let Some(structure_impl) = self.structures.get(&structure.structure_type) {
            let mut rng = LegacyRandom::from_seed(0);
            rng.set_large_feature_seed(self.seed, ctx.chunk_x(), ctx.chunk_z());
            return structure_impl
                .find_generation_point(ctx, structure, &mut rng)
                .map(|stub| (structure, stub));
        }

        tracing::warn!(
            "Unknown structure type {:?} for {}, using center biome check",
            structure.structure_type,
            structure.key
        );
        let surface_y = ctx.surface_y();
        let biome = ctx.biome_at(ctx.center_block_x(), surface_y, ctx.center_block_z());
        if structure.allowed_biomes.contains(&biome.key) {
            Some((
                structure,
                GenerationStub {
                    position: (ctx.center_block_x(), surface_y, ctx.center_block_z()),
                    pieces: Vec::new(),
                },
            ))
        } else {
            None
        }
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
    reg(
        "desert_pyramid",
        Box::new(SinglePieceStructure {
            size: (21, 15, 21),
            piece_id: "tedp",
            require_above_sea: true,
        }),
    );
    reg(
        "jungle_temple",
        Box::new(SinglePieceStructure {
            size: (12, 10, 15),
            piece_id: "tejp",
            require_above_sea: true,
        }),
    );
    reg(
        "swamp_hut",
        Box::new(SinglePieceStructure {
            size: (7, 7, 9),
            piece_id: "tesh",
            require_above_sea: false,
        }),
    );
    reg("buried_treasure", Box::new(BuriedTreasureStructure));
    reg("shipwreck", Box::new(ShipwreckStructure));
    reg("igloo", Box::new(IglooStructure));
    reg("ocean_ruin", Box::new(OceanRuinStructure));
    reg("stronghold", Box::new(StrongholdStructure));
    reg("ruined_portal", Box::new(RuinedPortalStructure));

    structures
}
