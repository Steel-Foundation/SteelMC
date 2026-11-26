use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt as _},
    fluid::FluidStateExt as _,
    vanilla_biome_tags::BiomeTag,
    vanilla_block_tags::BlockTag as tags,
    vanilla_blocks, vanilla_entities as entities,
};
use steel_utils::{BlockPos, Identifier};

use crate::{entity::EntityRegistry, world::LevelReader as _, worldgen::WorldGenRegion};

const ANIMAL_SPAWN_MIN_BRIGHTNESS: u8 = 9;
const TURTLE_SPAWN_HEIGHT_ABOVE_SEA: i32 = 4;

pub(crate) enum WorldgenSpawnPredicate {
    GroundTag(Identifier),
    Custom(fn(&WorldGenRegion<'_>, i64, BlockPos) -> bool),
}

pub(crate) struct WorldgenSpawnRule {
    predicate: WorldgenSpawnPredicate,
    pub(crate) immune_to: Option<Identifier>,
    pub(crate) preferred_ground: Option<BlockRef>,
    pub(crate) allows_liquid: bool,
}

impl WorldgenSpawnRule {
    pub(crate) const fn new(predicate: WorldgenSpawnPredicate) -> Self {
        Self {
            predicate,
            immune_to: None,
            preferred_ground: Some(&vanilla_blocks::GRASS_BLOCK),
            allows_liquid: false,
        }
    }

    pub(super) fn check(
        &self,
        region: &WorldGenRegion<'_>,
        biome_zoom_seed: i64,
        pos: BlockPos,
    ) -> bool {
        match &self.predicate {
            WorldgenSpawnPredicate::GroundTag(tag) => check_ground_tag(region, pos, tag),
            WorldgenSpawnPredicate::Custom(check) => check(region, biome_zoom_seed, pos),
        }
    }
}

pub(crate) fn register_worldgen_spawn_rules(registry: &mut EntityRegistry) {
    use WorldgenSpawnPredicate::{Custom, GroundTag};

    for entity in [
        &entities::CHICKEN,
        &entities::COW,
        &entities::DONKEY,
        &entities::HORSE,
        &entities::LLAMA,
        &entities::PANDA,
        &entities::PIG,
        &entities::SHEEP,
    ] {
        registry.register_worldgen_spawn_rule(
            entity,
            WorldgenSpawnRule::new(GroundTag(tags::ANIMALS_SPAWNABLE_ON)),
        );
    }

    registry.register_worldgen_spawn_rule(
        &entities::ARMADILLO,
        WorldgenSpawnRule::new(GroundTag(tags::ARMADILLO_SPAWNABLE_ON)),
    );
    registry.register_worldgen_spawn_rule(
        &entities::CAMEL,
        WorldgenSpawnRule::new(GroundTag(tags::CAMELS_SPAWNABLE_ON)),
    );
    registry.register_worldgen_spawn_rule(
        &entities::FOX,
        WorldgenSpawnRule {
            immune_to: Some(tags::FOX_IMMUNE_TO),
            ..WorldgenSpawnRule::new(GroundTag(tags::FOXES_SPAWNABLE_ON))
        },
    );
    registry.register_worldgen_spawn_rule(
        &entities::FROG,
        WorldgenSpawnRule::new(GroundTag(tags::FROGS_SPAWNABLE_ON)),
    );
    registry.register_worldgen_spawn_rule(
        &entities::GOAT,
        WorldgenSpawnRule::new(GroundTag(tags::GOATS_SPAWNABLE_ON)),
    );
    registry.register_worldgen_spawn_rule(
        &entities::MOOSHROOM,
        WorldgenSpawnRule {
            preferred_ground: Some(&vanilla_blocks::MYCELIUM),
            ..WorldgenSpawnRule::new(GroundTag(tags::MOOSHROOMS_SPAWNABLE_ON))
        },
    );
    registry.register_worldgen_spawn_rule(
        &entities::PARROT,
        WorldgenSpawnRule::new(GroundTag(tags::PARROTS_SPAWNABLE_ON)),
    );
    registry.register_worldgen_spawn_rule(
        &entities::POLAR_BEAR,
        WorldgenSpawnRule {
            immune_to: Some(tags::POLAR_BEAR_IMMUNE_TO),
            ..WorldgenSpawnRule::new(Custom(check_polar_bear))
        },
    );
    registry.register_worldgen_spawn_rule(
        &entities::RABBIT,
        WorldgenSpawnRule::new(GroundTag(tags::RABBITS_SPAWNABLE_ON)),
    );
    registry.register_worldgen_spawn_rule(
        &entities::STRIDER,
        WorldgenSpawnRule {
            preferred_ground: None,
            allows_liquid: true,
            ..WorldgenSpawnRule::new(Custom(check_strider))
        },
    );
    registry.register_worldgen_spawn_rule(
        &entities::TURTLE,
        WorldgenSpawnRule {
            preferred_ground: None,
            ..WorldgenSpawnRule::new(Custom(check_turtle))
        },
    );
    registry.register_worldgen_spawn_rule(
        &entities::WOLF,
        WorldgenSpawnRule::new(GroundTag(tags::WOLVES_SPAWNABLE_ON)),
    );
}

fn check_ground_tag(region: &WorldGenRegion<'_>, pos: BlockPos, tag: &Identifier) -> bool {
    region.block_state(pos.below()).get_block().has_tag(tag)
        && region.raw_brightness(pos, 0) >= ANIMAL_SPAWN_MIN_BRIGHTNESS
}

fn check_polar_bear(region: &WorldGenRegion<'_>, biome_zoom_seed: i64, pos: BlockPos) -> bool {
    let biome = super::biome_at(region, biome_zoom_seed, pos);
    let tag = if biome.has_tag(&BiomeTag::POLAR_BEARS_SPAWN_ON_ALTERNATE_BLOCKS) {
        &tags::POLAR_BEARS_SPAWNABLE_ON_ALTERNATE
    } else {
        &tags::ANIMALS_SPAWNABLE_ON
    };

    check_ground_tag(region, pos, tag)
}

fn check_turtle(region: &WorldGenRegion<'_>, _biome_zoom_seed: i64, pos: BlockPos) -> bool {
    pos.y() < region.sea_level() + TURTLE_SPAWN_HEIGHT_ABOVE_SEA
        && check_ground_tag(region, pos, &tags::SAND)
}

fn check_strider(region: &WorldGenRegion<'_>, _biome_zoom_seed: i64, pos: BlockPos) -> bool {
    let mut above = pos.above();
    while region.block_state(above).get_fluid_state().is_lava() {
        above = above.above();
    }

    region.block_state(above).is_air()
}
