use steel_registry::{REGISTRY, vanilla_entities as entities};

use crate::entity::entities::mobs::passive::{ChickenEntity, CowEntity, PigEntity, SheepEntity};
use crate::entity::{AgeableMobGroupData, Entity, EntityRegistry};

use super::{
    RunSpawnGroup, Sample, SpawnOverrides, required_variant, typed_initializer, with_ageable_group,
};

const ORDINARY_AGE: AgeableMobGroupData = AgeableMobGroupData::with_should_spawn_baby(true);

pub(crate) fn register_worldgen_spawn_groups(registry: &mut EntityRegistry) {
    registry.register_worldgen_spawn_group(&entities::CHICKEN, chicken_group);
    registry.register_worldgen_spawn_group(&entities::COW, cow_group);
    registry.register_worldgen_spawn_group(&entities::MOOSHROOM, ordinary_group);
    registry.register_worldgen_spawn_group(&entities::PIG, pig_group);
    registry.register_worldgen_spawn_group(&entities::SHEEP, sheep_group);
}

fn ordinary_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, (), |(), _, _| {
        Sample::common(SpawnOverrides::default())
    });
}

fn chicken_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, (), |(), _, context| {
        let variant = REGISTRY
            .chicken_variants
            .select_spawn_variant(context.biome, context.random);
        let sound = required_variant(REGISTRY.chicken_sound_variants.pick_random(context.random));
        Sample::typed(variant_initializer(
            variant,
            sound,
            ChickenEntity::set_variant,
            ChickenEntity::set_sound_variant,
        ))
    });
}

fn cow_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, (), |(), _, context| {
        let variant = REGISTRY
            .cow_variants
            .select_spawn_variant(context.biome, context.random);
        let sound = required_variant(REGISTRY.cow_sound_variants.pick_random(context.random));
        Sample::typed(variant_initializer(
            variant,
            sound,
            CowEntity::set_variant,
            CowEntity::set_sound_variant,
        ))
    });
}

fn pig_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, (), |(), _, context| {
        let variant = REGISTRY
            .pig_variants
            .select_spawn_variant(context.biome, context.random);
        let sound = required_variant(REGISTRY.pig_sound_variants.pick_random(context.random));
        Sample::typed(variant_initializer(
            variant,
            sound,
            PigEntity::set_variant,
            PigEntity::set_sound_variant,
        ))
    });
}

fn sheep_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, (), |(), _, context| {
        let color = SheepEntity::random_sheep_color(context.biome, context.random);
        Sample::typed(typed_initializer(move |sheep: &SheepEntity| {
            sheep.set_color(color);
        }))
    });
}

fn variant_initializer<E: Entity + steel_utils::DowncastType, V: Copy, S: Copy>(
    variant: Option<V>,
    sound: S,
    set_variant: fn(&E, V),
    set_sound: fn(&E, S),
) -> impl Fn(&dyn Entity) -> Result<(), super::SpawnInitError> {
    typed_initializer(move |entity| {
        if let Some(variant) = variant {
            set_variant(entity, variant);
        }
        set_sound(entity, sound);
    })
}
