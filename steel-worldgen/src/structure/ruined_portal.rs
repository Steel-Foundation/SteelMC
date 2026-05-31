//! Ruined portal. Mirrors vanilla's `RuinedPortalStructure.findGenerationPoint`
//! RNG consumption and persists the selected template-backed piece.

use std::sync::LazyLock;

use steel_registry::biome::{BiomeRef, TemperatureModifier};
use steel_registry::structure::{
    LiquidSettingsData, RuinedPortalPlacementData, RuinedPortalSetupData, StructureConfigData,
    StructureData,
};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::{Random, RandomSource};
use steel_utils::{BoundingBox, Direction, Identifier, Rotation};
use steel_worldgen::noise::PerlinSimplexNoise;

use crate::structure::{
    GenerationStub, RuinedPortalProperties, Structure, StructureBlockIgnore,
    StructureGenerationContext, StructureMirror, StructurePiece, StructurePiecePayload,
    TemplateMarkerHandling, TemplatePieceData, TemplatePlacementAdjustment, TemplatePlacementClip,
    TemplatePostProcess, TemplateProcessorList,
};

static TEMPERATURE_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(1234));
    PerlinSimplexNoise::new(&mut random, &[0])
});

static FROZEN_TEMPERATURE_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(3456));
    PerlinSimplexNoise::new(&mut random, &[-2, -1, 0])
});

static BIOME_INFO_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(2345));
    PerlinSimplexNoise::new(&mut random, &[0])
});

const PORTAL_TEMPLATES: [&str; 10] = [
    "ruined_portal/portal_1",
    "ruined_portal/portal_2",
    "ruined_portal/portal_3",
    "ruined_portal/portal_4",
    "ruined_portal/portal_5",
    "ruined_portal/portal_6",
    "ruined_portal/portal_7",
    "ruined_portal/portal_8",
    "ruined_portal/portal_9",
    "ruined_portal/portal_10",
];

const GIANT_PORTAL_TEMPLATES: [&str; 3] = [
    "ruined_portal/giant_portal_1",
    "ruined_portal/giant_portal_2",
    "ruined_portal/giant_portal_3",
];

/// Terrain query operations needed by the ruined portal generation.
pub enum TerrainQuery {
    /// Get surface height at (x, z). Returns first solid Y from top.
    SurfaceHeight {
        /// Block X.
        x: i32,
        /// Block Z.
        z: i32,
        /// `true` for `OCEAN_FLOOR_WG`, `false` for `WORLD_SURFACE_WG`.
        ocean_floor: bool,
    },
    /// Check if block at (x, y, z) is opaque for the selected heightmap.
    IsOpaque {
        /// Block X.
        x: i32,
        /// Block Y.
        y: i32,
        /// Block Z.
        z: i32,
        /// `true` for `OCEAN_FLOOR_WG`, `false` for `WORLD_SURFACE_WG`.
        ocean_floor: bool,
    },
}

/// Result of a terrain query.
pub enum TerrainResult {
    /// Surface height result.
    Height(i32),
    /// Block opacity result.
    Opaque(bool),
}

/// Result of ruined portal generation point computation.
pub struct PortalResult {
    /// Biome check position `(block_x, block_y, block_z)`.
    pub biome_check_pos: (i32, i32, i32),
    /// Bounding box of the placed portal piece.
    pub bounding_box: BoundingBox,
    /// Selected template id.
    pub template_id: Identifier,
    /// Template size in blocks.
    pub template_size: [i32; 3],
    /// Template rotation.
    pub rotation: Rotation,
    /// Template mirror.
    pub mirror: StructureMirror,
    /// Template rotation pivot in local coordinates.
    pub rotation_pivot: (i32, i32, i32),
    /// Vertical placement selected by the setup.
    pub vertical_placement: RuinedPortalPlacementData,
    /// Ruined portal properties before the cold biome adjustment.
    pub properties: RuinedPortalProperties,
    /// Whether vanilla evaluates coldness for this setup.
    pub can_be_cold: bool,
}

/// Matches vanilla's `RuinedPortalStructure.findGenerationPoint`.
#[expect(
    clippy::too_many_lines,
    reason = "inlines vanilla's setup → size → rotation → mirror → placement pipeline"
)]
pub fn find_generation_point(
    rng: &mut LegacyRandom,
    chunk_x: i32,
    chunk_z: i32,
    setups: &[RuinedPortalSetupData],
    min_y: i32,
    templates: &mut dyn FnMut(&Identifier) -> Option<[i32; 3]>,
    terrain: &mut dyn FnMut(TerrainQuery) -> TerrainResult,
) -> Option<PortalResult> {
    if setups.is_empty() {
        return None;
    }

    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;

    // Weighted selection via nextFloat.
    let setup = if setups.len() > 1 {
        let total: f32 = setups.iter().map(|s| s.weight).sum();
        let mut pick = rng.next_f32();
        let mut chosen_idx = setups.len() - 1;
        for (i, s) in setups.iter().enumerate() {
            pick -= s.weight / total;
            if pick < 0.0 {
                chosen_idx = i;
                break;
            }
        }
        &setups[chosen_idx]
    } else {
        &setups[0]
    };

    // Vanilla `sample(rng, p)` short-circuits at 0.0/1.0; we keep the guard so
    // out-of-range values added later don't unexpectedly draw RNG.
    let air_pocket = if setup.air_pocket_probability <= 0.0 {
        false
    } else if setup.air_pocket_probability >= 1.0 {
        true
    } else {
        rng.next_f32() < setup.air_pocket_probability
    };

    // 5% giant, 95% regular.
    let template_id = if rng.next_f32() < 0.05 {
        let index = rng.next_i32_bounded(GIANT_PORTAL_TEMPLATES.len() as i32) as usize;
        Identifier::vanilla_static(GIANT_PORTAL_TEMPLATES[index])
    } else {
        let index = rng.next_i32_bounded(PORTAL_TEMPLATES.len() as i32) as usize;
        Identifier::vanilla_static(PORTAL_TEMPLATES[index])
    };
    let template_size = templates(&template_id)?;
    let [sx, sy, sz] = template_size;

    let rotation = Rotation::get_random(rng);
    let mirror = if rng.next_f32() < 0.5 {
        StructureMirror::None
    } else {
        StructureMirror::FrontBack
    };
    let mirror_front_back = mirror == StructureMirror::FrontBack;
    let pivot_x = sx / 2;
    let pivot_z = sz / 2;
    let bb = rotation.get_bounding_box_full(
        (base_x, 0, base_z),
        (sx, sy, sz),
        pivot_x,
        pivot_z,
        mirror_front_back,
    );
    // Vanilla's `BoundingBox.getCenter()` = min + (max - min + 1) / 2, which
    // differs from (min + max) / 2 for even spans due to integer rounding.
    let bb_center_x = bb.min_x + (bb.max_x - bb.min_x + 1) / 2;
    let bb_center_z = bb.min_z + (bb.max_z - bb.min_z + 1) / 2;
    let ocean_floor = matches!(setup.placement, RuinedPortalPlacementData::OnOceanFloor);
    let surface_y = match terrain(TerrainQuery::SurfaceHeight {
        x: bb_center_x,
        z: bb_center_z,
        ocean_floor,
    }) {
        TerrainResult::Height(h) => h,
        TerrainResult::Opaque(_) => unreachable!(),
    } - 1;

    let min_y_threshold = min_y + 15;
    let new_y = match setup.placement {
        RuinedPortalPlacementData::OnLandSurface | RuinedPortalPlacementData::OnOceanFloor => {
            surface_y
        }
        RuinedPortalPlacementData::Underground => {
            let max_y = surface_y - sy;
            if min_y_threshold < max_y {
                rng.next_i32_between(min_y_threshold, max_y)
            } else {
                max_y
            }
        }
        RuinedPortalPlacementData::InMountain => {
            let max_y = surface_y - sy;
            if 70 < max_y {
                rng.next_i32_between(70, max_y)
            } else {
                max_y
            }
        }
        RuinedPortalPlacementData::PartlyBuried => surface_y - sy + rng.next_i32_between(2, 8),
        RuinedPortalPlacementData::InNether => {
            if air_pocket {
                rng.next_i32_between(32, 100)
            } else if rng.next_f32() < 0.5 {
                rng.next_i32_between(27, 29)
            } else {
                rng.next_i32_between(29, 100)
            }
        }
    };

    // findSuitableY: scan down, break when ≥3 of 4 corners are opaque.
    let corners = [
        (bb.min_x, bb.min_z),
        (bb.max_x, bb.min_z),
        (bb.min_x, bb.max_z),
        (bb.max_x, bb.max_z),
    ];
    let mut projected_y = new_y;
    'scan: while projected_y > min_y_threshold {
        let mut solid_count = 0;
        for &(cx, cz) in &corners {
            if matches!(
                terrain(TerrainQuery::IsOpaque {
                    x: cx,
                    y: projected_y,
                    z: cz,
                    ocean_floor,
                }),
                TerrainResult::Opaque(true)
            ) {
                solid_count += 1;
                if solid_count == 3 {
                    break 'scan;
                }
            }
        }
        projected_y -= 1;
    }

    Some(PortalResult {
        biome_check_pos: (base_x, projected_y, base_z),
        bounding_box: rotation.get_bounding_box_full(
            (base_x, projected_y, base_z),
            (sx, sy, sz),
            pivot_x,
            pivot_z,
            mirror_front_back,
        ),
        template_id,
        template_size,
        rotation,
        mirror,
        rotation_pivot: (pivot_x, 0, pivot_z),
        vertical_placement: setup.placement,
        properties: RuinedPortalProperties {
            cold: false,
            mossiness: setup.mossiness,
            air_pocket,
            overgrown: setup.overgrown,
            vines: setup.vines,
            replace_with_blackstone: setup.replace_with_blackstone,
        },
        can_be_cold: setup.can_be_cold,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "ruined portal piece construction mirrors vanilla template-piece fields"
)]
fn make_ruined_portal_piece(
    template_id: Identifier,
    position: (i32, i32, i32),
    rotation: Rotation,
    mirror: StructureMirror,
    rotation_pivot: (i32, i32, i32),
    size: [i32; 3],
    vertical_placement: RuinedPortalPlacementData,
    properties: RuinedPortalProperties,
) -> StructurePiece {
    let mirror_front_back = mirror == StructureMirror::FrontBack;
    let bounding_box = rotation.get_bounding_box_full(
        position,
        (size[0], size[1], size[2]),
        rotation_pivot.0,
        rotation_pivot.2,
        mirror_front_back,
    );
    StructurePiece {
        piece_type: Identifier::new_static("minecraft", "rupo"),
        bounding_box,
        gen_depth: 0,
        orientation: Some(Direction::North),
        payload: StructurePiecePayload::Template(TemplatePieceData {
            template_id,
            template_position: position,
            rotation,
            mirror,
            rotation_pivot,
            block_ignore: if properties.air_pocket {
                StructureBlockIgnore::StructureBlock
            } else {
                StructureBlockIgnore::StructureAndAir
            },
            late_block_ignore: StructureBlockIgnore::None,
            processors: TemplateProcessorList::RuinedPortal {
                vertical_placement,
                properties,
            },
            liquid_settings: LiquidSettingsData::ApplyWaterlogging,
            marker_handling: TemplateMarkerHandling::Ignore,
            placement_adjustment: TemplatePlacementAdjustment::None,
            placement_clip:
                TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate,
            post_process: TemplatePostProcess::RuinedPortal,
        }),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    }
}

fn cold_enough_to_snow(biome: BiomeRef, sea_level: i32, pos: (i32, i32, i32)) -> bool {
    biome_temperature(biome, sea_level, pos) < 0.15
}

fn biome_temperature(biome: BiomeRef, sea_level: i32, pos: (i32, i32, i32)) -> f32 {
    let modified_temperature = match biome.temperature_modifier {
        TemperatureModifier::None => biome.temperature,
        TemperatureModifier::Frozen => {
            let large = FROZEN_TEMPERATURE_NOISE
                .get_value(f64::from(pos.0) * 0.05, f64::from(pos.2) * 0.05)
                * 7.0;
            let edge = BIOME_INFO_NOISE.get_value(f64::from(pos.0) * 0.2, f64::from(pos.2) * 0.2);
            if large + edge < 0.3 {
                let small =
                    BIOME_INFO_NOISE.get_value(f64::from(pos.0) * 0.09, f64::from(pos.2) * 0.09);
                if small < 0.8 { 0.2 } else { biome.temperature }
            } else {
                biome.temperature
            }
        }
    };

    let snow_level = sea_level + 17;
    if pos.1 <= snow_level {
        return modified_temperature;
    }

    let value =
        TEMPERATURE_NOISE.get_value(f64::from(pos.0) / 8.0, f64::from(pos.2) / 8.0) as f32 * 8.0;
    modified_temperature - (value + pos.1 as f32 - snow_level as f32) * 0.05 / 40.0
}

/// Registered under `"minecraft:ruined_portal"` and its biome variants
/// (desert / jungle / mountain / ocean / swamp / nether). The terrain closure
/// creates a fresh aquifer + column cache per query since piece gen can probe
/// outside this chunk.
pub struct RuinedPortalStructure;

impl Structure for RuinedPortalStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let mut template_choices = Vec::with_capacity(
            PORTAL_TEMPLATES
                .len()
                .saturating_add(GIANT_PORTAL_TEMPLATES.len()),
        );
        for path in PORTAL_TEMPLATES.iter().chain(GIANT_PORTAL_TEMPLATES.iter()) {
            let id = Identifier::vanilla_static(path);
            let size = ctx.templates().get(&id)?.size;
            template_choices.push((id, size));
        }

        let mut terrain = |q: TerrainQuery| -> TerrainResult {
            match q {
                TerrainQuery::SurfaceHeight { x, z, ocean_floor } => {
                    TerrainResult::Height(ctx.terrain_surface_height(x, z, ocean_floor))
                }
                TerrainQuery::IsOpaque {
                    x,
                    y,
                    z,
                    ocean_floor,
                } => TerrainResult::Opaque(ctx.terrain_is_opaque(x, y, z, ocean_floor)),
            }
        };

        let StructureConfigData::RuinedPortal { setups } = &structure.config else {
            return None;
        };
        if setups.is_empty() {
            return None;
        }

        let result = find_generation_point(
            rng,
            ctx.chunk_x(),
            ctx.chunk_z(),
            setups,
            ctx.min_y(),
            &mut |id| {
                template_choices
                    .iter()
                    .find(|(template_id, _)| template_id == id)
                    .map(|(_, size)| *size)
            },
            &mut terrain,
        )?;

        let (bx, by, bz) = result.biome_check_pos;
        let biome = ctx.biome_at(bx, by, bz);
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let mut properties = result.properties;
        if result.can_be_cold {
            properties.cold = cold_enough_to_snow(biome, ctx.sea_level(), result.biome_check_pos);
        }

        Some(GenerationStub {
            position: result.biome_check_pos,
            pieces: vec![make_ruined_portal_piece(
                result.template_id,
                result.biome_check_pos,
                result.rotation,
                result.mirror,
                result.rotation_pivot,
                result.template_size,
                result.vertical_placement,
                properties,
            )],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruined_portal_piece_uses_template_payload_with_processors_and_postprocess() {
        let position = (64, 72, -32);
        let size = [11, 17, 16];
        let properties = RuinedPortalProperties {
            cold: true,
            mossiness: 0.8,
            air_pocket: false,
            overgrown: true,
            vines: true,
            replace_with_blackstone: false,
        };

        let piece = make_ruined_portal_piece(
            Identifier::vanilla_static("ruined_portal/giant_portal_1"),
            position,
            Rotation::Clockwise90,
            StructureMirror::FrontBack,
            (size[0] / 2, 0, size[2] / 2),
            size,
            RuinedPortalPlacementData::OnOceanFloor,
            properties,
        );

        assert_eq!(
            piece.piece_type,
            Identifier::new_static("minecraft", "rupo")
        );
        assert_eq!(piece.gen_depth, 0);
        assert_eq!(piece.orientation, Some(Direction::North));
        assert_eq!(
            piece.bounding_box,
            Rotation::Clockwise90.get_bounding_box_full(
                position,
                (size[0], size[1], size[2]),
                size[0] / 2,
                size[2] / 2,
                true,
            ),
        );

        let StructurePiecePayload::Template(data) = piece.payload else {
            panic!("ruined portal piece should be template-backed");
        };
        assert_eq!(
            data.template_id,
            Identifier::vanilla_static("ruined_portal/giant_portal_1")
        );
        assert_eq!(data.template_position, position);
        assert_eq!(data.rotation, Rotation::Clockwise90);
        assert_eq!(data.mirror, StructureMirror::FrontBack);
        assert_eq!(data.rotation_pivot, (size[0] / 2, 0, size[2] / 2));
        assert_eq!(data.block_ignore, StructureBlockIgnore::StructureAndAir);
        assert_eq!(data.late_block_ignore, StructureBlockIgnore::None);
        assert_eq!(
            data.processors,
            TemplateProcessorList::RuinedPortal {
                vertical_placement: RuinedPortalPlacementData::OnOceanFloor,
                properties,
            }
        );
        assert_eq!(data.liquid_settings, LiquidSettingsData::ApplyWaterlogging);
        assert_eq!(data.marker_handling, TemplateMarkerHandling::Ignore);
        assert_eq!(data.placement_adjustment, TemplatePlacementAdjustment::None);
        assert_eq!(
            data.placement_clip,
            TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate
        );
        assert_eq!(data.post_process, TemplatePostProcess::RuinedPortal);
    }
}
