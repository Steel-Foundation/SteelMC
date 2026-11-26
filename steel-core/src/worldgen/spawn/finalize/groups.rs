use steel_registry::{
    REGISTRY,
    entity_variant::{HorseMarkings, HorseVariant, LlamaVariant, PandaGene, ParrotVariant},
    vanilla_entities as entities,
};
use steel_utils::random::Random as _;
use steel_utils::value_providers::UniformIntProvider;

use crate::entity::entities::mobs::passive::{ChickenEntity, CowEntity, PigEntity, SheepEntity};
use crate::entity::{AgeableMobGroupData, Entity, EntityRegistry};

use super::{
    RunSpawnGroup, Sample, SpawnOverrides, choose, equine, required_variant, strider,
    typed_initializer, variants, with_ageable_group,
};

const ORDINARY_AGE: AgeableMobGroupData = AgeableMobGroupData::with_should_spawn_baby(true);
const ADULT_ONLY: AgeableMobGroupData = AgeableMobGroupData::with_should_spawn_baby(false);
const BABY_AFTER_FIRST: AgeableMobGroupData = AgeableMobGroupData::with_baby_spawn_chance(1.0);
const PANDA_AGE: AgeableMobGroupData = AgeableMobGroupData::with_baby_spawn_chance(0.2);
const ABSTRACT_HORSE_AGE: AgeableMobGroupData = AgeableMobGroupData::with_baby_spawn_chance(0.2);
const FOX_ADULT_GROUP_MEMBERS: i32 = 2;
const GOAT_LONG_JUMP_COOLDOWN_TICKS: UniformIntProvider = UniformIntProvider {
    min_inclusive: 600,
    max_inclusive: 1200,
};
const GOAT_RAM_COOLDOWN_TICKS: UniformIntProvider = UniformIntProvider {
    min_inclusive: 600,
    max_inclusive: 6000,
};
const GOAT_SCREAMING_CHANCE: f64 = 0.02;
const GOAT_MISSING_HORN_CHANCE: f32 = 0.1;

pub(crate) fn register_worldgen_spawn_groups(registry: &mut EntityRegistry) {
    registry.register_worldgen_spawn_group(&entities::ARMADILLO, ordinary_group);
    registry.register_worldgen_spawn_group(&entities::CAMEL, camel_group);
    registry.register_worldgen_spawn_group(&entities::CHICKEN, chicken_group);
    registry.register_worldgen_spawn_group(&entities::COW, cow_group);
    registry.register_worldgen_spawn_group(&entities::DONKEY, donkey_group);
    registry.register_worldgen_spawn_group(&entities::FOX, fox_group);
    registry.register_worldgen_spawn_group(&entities::FROG, frog_group);
    registry.register_worldgen_spawn_group(&entities::GOAT, goat_group);
    registry.register_worldgen_spawn_group(&entities::HORSE, horse_group);
    registry.register_worldgen_spawn_group(&entities::LLAMA, llama_group);
    registry.register_worldgen_spawn_group(&entities::MOOSHROOM, ordinary_group);
    registry.register_worldgen_spawn_group(&entities::PANDA, panda_group);
    registry.register_worldgen_spawn_group(&entities::PARROT, parrot_group);
    registry.register_worldgen_spawn_group(&entities::PIG, pig_group);
    registry.register_worldgen_spawn_group(&entities::POLAR_BEAR, baby_after_first_group);
    registry.register_worldgen_spawn_group(&entities::RABBIT, rabbit_group);
    registry.register_worldgen_spawn_group(&entities::SHEEP, sheep_group);
    registry.register_worldgen_spawn_group(&entities::STRIDER, strider::spawn_group);
    registry.register_worldgen_spawn_group(&entities::TURTLE, turtle_group);
    registry.register_worldgen_spawn_group(&entities::WOLF, wolf_group);
}

fn ordinary_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, (), |(), _, _| {
        Sample::common(SpawnOverrides::default())
    });
}

fn baby_after_first_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, BABY_AFTER_FIRST, (), |(), _, _| {
        Sample::common(SpawnOverrides::default())
    });
}

fn camel_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ABSTRACT_HORSE_AGE, (), |(), _, _| {
        Sample::shadow_only(SpawnOverrides::default())
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

fn donkey_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ABSTRACT_HORSE_AGE, (), |(), _, context| {
        Sample::common(SpawnOverrides {
            equine_attributes: Some(equine::EquineAttributes::sample(context.random, false)),
            ..SpawnOverrides::default()
        })
    });
}

fn fox_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ADULT_ONLY, (), |(), ageable, context| {
        Sample::shadow_only(SpawnOverrides {
            is_baby: ageable.group_size() >= FOX_ADULT_GROUP_MEMBERS,
            main_hand_item: variants::fox_mouth_item(context.random),
            ..SpawnOverrides::default()
        })
    });
}

fn frog_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, (), |(), _, context| {
        variants::sample_frog(context.biome, context.random);
        Sample::shadow_only(SpawnOverrides::default())
    });
}

fn goat_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, (), |(), _, context| {
        let random = &mut *context.random;
        let _long_jump = GOAT_LONG_JUMP_COOLDOWN_TICKS.sample(random);
        let _ram = GOAT_RAM_COOLDOWN_TICKS.sample(random);
        let _screaming = random.next_f64() < GOAT_SCREAMING_CHANCE;
        let _missing_left_horn =
            (random.next_f32() < GOAT_MISSING_HORN_CHANCE).then(|| random.next_bool());
        Sample::shadow_only(SpawnOverrides::default())
    });
}

fn horse_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, None, |variant, _, context| {
        let random = &mut *context.random;
        variant.get_or_insert_with(|| choose(&HorseVariant::VALUES, random));
        let _markings = choose(&HorseMarkings::VALUES, random);
        Sample::shadow_only(SpawnOverrides {
            equine_attributes: Some(equine::EquineAttributes::sample(random, true)),
            ..SpawnOverrides::default()
        })
    });
}

fn llama_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, None, |variant, _, context| {
        let random = &mut *context.random;
        let _strength = equine::llama_strength(random);
        variant.get_or_insert_with(|| choose(&LlamaVariant::VALUES, random));
        Sample::shadow_only(SpawnOverrides {
            equine_attributes: Some(equine::EquineAttributes::sample(random, false)),
            ..SpawnOverrides::default()
        })
    });
}

fn panda_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, PANDA_AGE, (), |(), _, context| {
        let _main_gene = PandaGene::sample(context.random);
        let _hidden_gene = PandaGene::sample(context.random);
        Sample::shadow_only(SpawnOverrides::default())
    });
}

fn parrot_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ADULT_ONLY, (), |(), _, context| {
        let _variant = choose(&ParrotVariant::VALUES, context.random);
        Sample::shadow_only(SpawnOverrides::default())
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

fn rabbit_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, BABY_AFTER_FIRST, (), |(), _, context| {
        let _variant = variants::rabbit_variant(context.biome, context.random);
        Sample::shadow_only(SpawnOverrides::default())
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

fn turtle_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, (), |(), _, _| {
        Sample::shadow_only(SpawnOverrides::default())
    });
}

fn wolf_group(run: &mut RunSpawnGroup<'_>) {
    with_ageable_group(run, ORDINARY_AGE, None, |variant, ageable, context| {
        variants::sample_wolf(variant, ageable, context.biome, context.random);
        Sample::shadow_only(SpawnOverrides::default())
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
