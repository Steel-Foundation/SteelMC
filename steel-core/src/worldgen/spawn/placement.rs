use glam::DVec3;
use steel_registry::{
    REGISTRY,
    blocks::{
        block_state_ext::BlockStateExt as _,
        properties::BlockStateProperties,
        shapes::{OffsetVoxelShape, is_offset_shape_full_block},
    },
    entity_type::{EntityTypeRef, SpawnPlacementHeightmap, SpawnPlacementKind},
    fluid::FluidStateExt as _,
    vanilla_block_tags::BlockTag as tags,
    vanilla_blocks,
};
use steel_utils::{BlockPos, SectionPos, WorldAabb, types::Difficulty};

use crate::{
    behavior::{BLOCK_BEHAVIORS, BlockCollisionContext},
    chunk::heightmap::HeightmapType::{self, MotionBlocking, MotionBlockingNoLeaves},
    entity::ENTITIES,
    physics::collision::has_spawn_block_collision,
    world::{LevelReader as _, World, is_redstone_conductor},
    worldgen::region::WorldGenRegion,
};

use SpawnPlacementKind::{InLava, InWater, NoRestrictions, OnGround};

use super::SpawnSite;
use super::rules::WorldgenSpawnRule;

pub(super) enum Attempt {
    WalkWithoutCandidate,
    RetryAtSamePosition,
    Candidate { position: DVec3, can_finalize: bool },
}

pub(super) struct Placement {
    entity_type: EntityTypeRef,
    kind: SpawnPlacementKind,
    heightmap: HeightmapType,
    rule: &'static WorldgenSpawnRule,
}

impl Placement {
    pub(super) fn for_entity(entity_type: EntityTypeRef) -> Option<Self> {
        let placement = entity_type.spawn_placement?;
        let heightmap = match placement.heightmap {
            SpawnPlacementHeightmap::MotionBlocking => MotionBlocking,
            SpawnPlacementHeightmap::MotionBlockingNoLeaves => MotionBlockingNoLeaves,
        };

        let rule = ENTITIES.worldgen_spawn_rule(entity_type)?;

        Some(Self {
            entity_type,
            kind: placement.kind,
            heightmap,
            rule,
        })
    }

    pub(super) fn try_position(
        &self,
        region: &mut WorldGenRegion<'_>,
        world: &World,
        biome_zoom_seed: i64,
        x: i32,
        z: i32,
    ) -> Attempt {
        let pos = self.top_non_colliding_pos(region, world.dimension_type.has_ceiling, x, z);
        if !self.entity_type.summonable || !self.is_spawn_position_ok(region, world, pos) {
            return Attempt::WalkWithoutCandidate;
        }

        let center = region.center();
        let min_x = SectionPos::section_to_block_coord(center.0.x, 0);
        let min_z = SectionPos::section_to_block_coord(center.0.y, 0);
        let width = self.entity_type.dimensions.width;
        let position = DVec3::new(
            clamp_spawn_coordinate(x, min_x, width),
            f64::from(pos.y()),
            clamp_spawn_coordinate(z, min_z, width),
        );

        let half_width = f64::from(self.entity_type.dimensions.half_width());
        let box_at_spawn = WorldAabb::new(
            position.x - half_width,
            position.y,
            position.z - half_width,
            position.x + half_width,
            position.y + f64::from(self.entity_type.dimensions.height),
            position.z + half_width,
        );

        let site = SpawnSite {
            entity_type: self.entity_type,
            pos: BlockPos::from(position),
        };
        if has_spawn_block_collision(region, box_at_spawn)
            || (!self.entity_type.allowed_in_peaceful && world.difficulty() == Difficulty::Peaceful)
            || !self.rule.check(region, biome_zoom_seed, site)
        {
            return Attempt::RetryAtSamePosition;
        }

        let can_finalize = self.check_walk_target_value(region, site.pos)
            && (self.rule.allows_liquid || !region.contains_any_liquid(box_at_spawn));

        Attempt::Candidate {
            position,
            can_finalize,
        }
    }

    fn top_non_colliding_pos(
        &self,
        region: &WorldGenRegion<'_>,
        has_ceiling: bool,
        x: i32,
        z: i32,
    ) -> BlockPos {
        let height = region.height_at(self.heightmap, x, z);
        let mut pos = BlockPos::new(x, height, z);
        if has_ceiling {
            pos = pos.below();
            while !region.block_state(pos).is_air() {
                pos = pos.below();
            }

            pos = pos.below();
            while region.block_state(pos).is_air() && pos.y() > region.min_y() {
                pos = pos.below();
            }
        }

        let below = pos.below();
        if matches!(self.kind, OnGround)
            && REGISTRY
                .blocks
                .is_land_pathfindable_for_spawn(region.block_state(below))
        {
            below
        } else {
            pos
        }
    }

    fn is_spawn_position_ok(
        &self,
        region: &WorldGenRegion<'_>,
        world: &World,
        pos: BlockPos,
    ) -> bool {
        match self.kind {
            NoRestrictions => true,
            _ if !world.is_block_within_world_border(pos) => false,
            InLava => region.block_state(pos).get_fluid_state().is_lava(),
            InWater => {
                let above = pos.above();
                region.block_state(pos).get_fluid_state().is_water()
                    && !is_redstone_conductor(region, region.block_state(above), above)
            }
            OnGround => {
                let below = pos.below();
                let below_state = region.block_state(below);

                BLOCK_BEHAVIORS
                    .get_behavior(below_state.get_block())
                    .is_valid_spawn(below_state, region, below, self.entity_type)
                    && self.is_valid_empty_spawn_block(region, pos)
                    && self.is_valid_empty_spawn_block(region, pos.above())
            }
        }
    }

    fn is_valid_empty_spawn_block(&self, region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        let state = region.block_state(pos);
        if state.is_air() {
            return true;
        }

        let block = state.get_block();
        if block.spawn_signal_source
            || state.has_fluid()
            || block.has_tag(&tags::PREVENT_MOB_SPAWNING_INSIDE)
        {
            return false;
        }

        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        let context = BlockCollisionContext::empty();
        let shape = OffsetVoxelShape::new(
            behavior.get_collision_shape(state, region, pos, context),
            behavior.get_collision_shape_offset(state, region, pos, context),
        );

        if is_offset_shape_full_block(shape) {
            return false;
        }

        if self
            .rule
            .immune_to
            .as_ref()
            .is_some_and(|tag| block.has_tag(tag))
        {
            return true;
        }

        let hazardous_fire = !self.entity_type.fire_immune
            && (block.has_tag(&tags::FIRE)
                || block == &vanilla_blocks::LAVA
                || block == &vanilla_blocks::MAGMA_BLOCK
                || block == &vanilla_blocks::LAVA_CAULDRON
                || (block.has_tag(&tags::CAMPFIRES)
                    && state.try_get_value(&BlockStateProperties::LIT) == Some(true)));

        !hazardous_fire
            && block != &vanilla_blocks::WITHER_ROSE
            && block != &vanilla_blocks::SWEET_BERRY_BUSH
            && block != &vanilla_blocks::CACTUS
            && block != &vanilla_blocks::POWDER_SNOW
    }

    fn check_walk_target_value(&self, region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        self.rule.preferred_ground.is_none_or(|ground| {
            region.block_state(pos.below()).get_block() == ground
                || region.pathfinding_cost_from_light_levels(pos) >= 0.0
        })
    }
}

pub(super) fn clamp_spawn_coordinate(block: i32, origin: i32, width: f32) -> f64 {
    let origin = f64::from(origin);
    let width = f64::from(width);
    let chunk_width = f64::from(SectionPos::section_to_block_coord(1, 0));

    f64::from(block).clamp(origin + width, origin + chunk_width - width)
}
