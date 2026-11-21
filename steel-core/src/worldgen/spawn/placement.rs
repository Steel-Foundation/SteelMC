use glam::DVec3;
use steel_registry::{
    REGISTRY,
    blocks::{
        block_state_ext::BlockStateExt as _,
        properties::BlockStateProperties,
        shapes::{OffsetVoxelShape, is_offset_shape_full_block},
    },
    entity_type::EntityTypeRef,
    fluid::FluidStateExt as _,
    vanilla_biome_tags::BiomeTag,
    vanilla_block_tags::BlockTag as tags,
    vanilla_blocks, vanilla_entities,
};
use steel_utils::{BlockPos, SectionPos, WorldAabb, types::Difficulty};

use crate::{
    behavior::{BLOCK_BEHAVIORS, BlockCollisionContext},
    chunk::heightmap::HeightmapType::{self, MotionBlocking, MotionBlockingNoLeaves},
    physics::collision::has_spawn_block_collision,
    world::{LevelReader as _, World},
    worldgen::region::WorldGenRegion,
};

use self::PlacementKind::{InLava, NoRestrictions, OnGround};

const ANIMAL_SPAWN_MIN_BRIGHTNESS: u8 = 9;
const TURTLE_SPAWN_HEIGHT_ABOVE_SEA: i32 = 4;

pub(super) enum Attempt {
    WalkWithoutCandidate,
    RetryAtSamePosition,
    Candidate { position: DVec3, can_finalize: bool },
}

#[derive(Clone, Copy)]
enum PlacementKind {
    OnGround,
    InLava,
    NoRestrictions,
}

#[derive(Clone, Copy)]
enum SpawnRule {
    Animal,
    Armadillo,
    Camel,
    Fox,
    Frog,
    Goat,
    Mooshroom,
    Parrot,
    PolarBear,
    Rabbit,
    Turtle,
    Strider,
    Wolf,
}

const GROUND_SPAWN_RULES: &[(EntityTypeRef, SpawnRule)] = &[
    (&vanilla_entities::ARMADILLO, SpawnRule::Armadillo),
    (&vanilla_entities::CAMEL, SpawnRule::Camel),
    (&vanilla_entities::CHICKEN, SpawnRule::Animal),
    (&vanilla_entities::COW, SpawnRule::Animal),
    (&vanilla_entities::DONKEY, SpawnRule::Animal),
    (&vanilla_entities::FROG, SpawnRule::Frog),
    (&vanilla_entities::GOAT, SpawnRule::Goat),
    (&vanilla_entities::HORSE, SpawnRule::Animal),
    (&vanilla_entities::LLAMA, SpawnRule::Animal),
    (&vanilla_entities::MOOSHROOM, SpawnRule::Mooshroom),
    (&vanilla_entities::PIG, SpawnRule::Animal),
    (&vanilla_entities::POLAR_BEAR, SpawnRule::PolarBear),
    (&vanilla_entities::RABBIT, SpawnRule::Rabbit),
    (&vanilla_entities::SHEEP, SpawnRule::Animal),
    (&vanilla_entities::TURTLE, SpawnRule::Turtle),
    (&vanilla_entities::WOLF, SpawnRule::Wolf),
];

pub(super) struct Placement {
    entity_type: EntityTypeRef,
    kind: PlacementKind,
    heightmap: HeightmapType,
    rule: SpawnRule,
}

impl Placement {
    pub(super) fn for_entity(entity_type: EntityTypeRef) -> Option<Self> {
        let (kind, heightmap, rule) = if entity_type == &vanilla_entities::FOX {
            (NoRestrictions, MotionBlockingNoLeaves, SpawnRule::Fox)
        } else if entity_type == &vanilla_entities::PANDA {
            (NoRestrictions, MotionBlockingNoLeaves, SpawnRule::Animal)
        } else if entity_type == &vanilla_entities::STRIDER {
            (InLava, MotionBlockingNoLeaves, SpawnRule::Strider)
        } else if entity_type == &vanilla_entities::PARROT {
            (OnGround, MotionBlocking, SpawnRule::Parrot)
        } else {
            let &(_, rule) = GROUND_SPAWN_RULES
                .iter()
                .find(|(entity, _)| *entity == entity_type)?;

            (OnGround, MotionBlockingNoLeaves, rule)
        };

        Some(Self {
            entity_type,
            kind,
            heightmap,
            rule,
        })
    }

    pub(super) fn try_position(
        &self,
        region: &WorldGenRegion<'_>,
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

        let block_pos = BlockPos::from(position);
        if has_spawn_block_collision(region, box_at_spawn)
            || (!self.entity_type.allowed_in_peaceful && world.difficulty() == Difficulty::Peaceful)
            || !self.check_spawn_rules(region, biome_zoom_seed, block_pos)
        {
            return Attempt::RetryAtSamePosition;
        }

        let can_finalize = self.check_walk_target_value(region, block_pos)
            && (matches!(self.rule, SpawnRule::Strider)
                || !region.contains_any_liquid(box_at_spawn));

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

        let immunity = match self.rule {
            SpawnRule::PolarBear => block.has_tag(&tags::POLAR_BEAR_IMMUNE_TO),
            SpawnRule::Fox => block.has_tag(&tags::FOX_IMMUNE_TO),
            _ => false,
        };

        if immunity {
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

    fn check_spawn_rules(
        &self,
        region: &WorldGenRegion<'_>,
        biome_zoom_seed: i64,
        pos: BlockPos,
    ) -> bool {
        let tag = match self.rule {
            SpawnRule::Animal => tags::ANIMALS_SPAWNABLE_ON,
            SpawnRule::Armadillo => tags::ARMADILLO_SPAWNABLE_ON,
            SpawnRule::Camel => tags::CAMELS_SPAWNABLE_ON,
            SpawnRule::Fox => tags::FOXES_SPAWNABLE_ON,
            SpawnRule::Frog => tags::FROGS_SPAWNABLE_ON,
            SpawnRule::Goat => tags::GOATS_SPAWNABLE_ON,
            SpawnRule::Mooshroom => tags::MOOSHROOMS_SPAWNABLE_ON,
            SpawnRule::Parrot => tags::PARROTS_SPAWNABLE_ON,
            SpawnRule::PolarBear => {
                let biome = super::biome_at(region, biome_zoom_seed, pos);
                if biome.has_tag(&BiomeTag::POLAR_BEARS_SPAWN_ON_ALTERNATE_BLOCKS) {
                    tags::POLAR_BEARS_SPAWNABLE_ON_ALTERNATE
                } else {
                    tags::ANIMALS_SPAWNABLE_ON
                }
            }
            SpawnRule::Rabbit => tags::RABBITS_SPAWNABLE_ON,
            SpawnRule::Turtle => {
                if pos.y() >= region.sea_level() + TURTLE_SPAWN_HEIGHT_ABOVE_SEA {
                    return false;
                }

                tags::SAND
            }
            SpawnRule::Strider => {
                let mut above = pos.above();
                while region.block_state(above).get_fluid_state().is_lava() {
                    above = above.above();
                }

                return region.block_state(above).is_air();
            }
            SpawnRule::Wolf => tags::WOLVES_SPAWNABLE_ON,
        };

        region.block_state(pos.below()).get_block().has_tag(&tag)
            && region.raw_brightness(pos, 0) >= ANIMAL_SPAWN_MIN_BRIGHTNESS
    }

    fn check_walk_target_value(&self, region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        match self.rule {
            SpawnRule::Strider | SpawnRule::Turtle => true,
            _ => {
                let preferred_ground = if matches!(self.rule, SpawnRule::Mooshroom) {
                    &vanilla_blocks::MYCELIUM
                } else {
                    &vanilla_blocks::GRASS_BLOCK
                };

                region.block_state(pos.below()).get_block() == preferred_ground
                    || region.pathfinding_cost_from_light_levels(pos) >= 0.0
            }
        }
    }
}

pub(super) fn clamp_spawn_coordinate(block: i32, origin: i32, width: f32) -> f64 {
    let origin = f64::from(origin);
    let width = f64::from(width);
    let chunk_width = f64::from(SectionPos::section_to_block_coord(1, 0));

    f64::from(block).clamp(origin + width, origin + chunk_width - width)
}
