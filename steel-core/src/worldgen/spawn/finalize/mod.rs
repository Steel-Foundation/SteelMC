//! groupdependent init and independent of steel entity factories

mod equine;
mod strider;
mod variants;

use steel_registry::biome::BiomeRef;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::entity_variant::{
    FoxVariant, HorseVariant, LlamaVariant, ParrotVariant, RabbitVariant,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_game_rules::{SPAWN_MOBS, SPAWN_MONSTERS};
use steel_registry::wolf_variant::WolfVariantRef;
use steel_registry::{REGISTRY, vanilla_entities as entities, vanilla_items};
use steel_utils::BlockPos;
use steel_utils::random::{Random, RandomSource};
use steel_utils::value_providers::UniformIntProvider;

use crate::entity::entities::mobs::passive::SheepEntity;
use crate::entity::{
    AgeableMobGroupData, Entity, HorseMarkings, MobSpawnState, PandaGene,
    WorldgenSpawnProperties as Properties,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::world::World;

use self::equine::EquineAttributes;

const FOX_ADULT_GROUP_MEMBERS: i32 = 2;
const PANDA_BABY_CHANCE: f32 = 0.2;
const ABSTRACT_HORSE_DEFAULT_BABY_CHANCE: f32 = 0.2;
const ALWAYS_BABY_AFTER_FIRST: f32 = 1.0;
const CAMEL_STAND_UP_TICKS: i64 = 52;
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

#[derive(Clone, Copy)]
enum Species {
    Plain,
    Camel,
    Chicken,
    Cow,
    Donkey,
    Fox(Option<FoxVariant>),
    Frog,
    Goat,
    Horse(Option<HorseVariant>),
    Llama(Option<LlamaVariant>),
    Panda,
    Parrot,
    Pig,
    Rabbit(Option<RabbitVariant>),
    Sheep,
    Strider,
    Turtle,
    Wolf(Option<WolfVariantRef>),
}

pub(super) struct SpawnGroup {
    entity_type: EntityTypeRef,
    species: Species,
    ageable: AgeableMobGroupData,
}

impl SpawnGroup {
    pub(super) fn new(entity_type: EntityTypeRef) -> Option<Self> {
        let ordinary_age = AgeableMobGroupData::with_should_spawn_baby(true);
        let adult_only = AgeableMobGroupData::with_should_spawn_baby(false);
        let baby_after_first = AgeableMobGroupData::with_baby_spawn_chance(ALWAYS_BABY_AFTER_FIRST);
        let abstract_horse_age =
            AgeableMobGroupData::with_baby_spawn_chance(ABSTRACT_HORSE_DEFAULT_BABY_CHANCE);
        let panda_age = AgeableMobGroupData::with_baby_spawn_chance(PANDA_BABY_CHANCE);

        let (species, ageable) = match entity_type {
            t if t == &entities::ARMADILLO || t == &entities::MOOSHROOM => {
                (Species::Plain, ordinary_age)
            }
            t if t == &entities::POLAR_BEAR => (Species::Plain, baby_after_first),
            t if t == &entities::CAMEL => (Species::Camel, abstract_horse_age),
            t if t == &entities::CHICKEN => (Species::Chicken, ordinary_age),
            t if t == &entities::COW => (Species::Cow, ordinary_age),
            t if t == &entities::DONKEY => (Species::Donkey, abstract_horse_age),
            t if t == &entities::FOX => (Species::Fox(None), adult_only),
            t if t == &entities::FROG => (Species::Frog, ordinary_age),
            t if t == &entities::GOAT => (Species::Goat, ordinary_age),
            t if t == &entities::HORSE => (Species::Horse(None), ordinary_age),
            t if t == &entities::LLAMA => (Species::Llama(None), ordinary_age),
            t if t == &entities::PANDA => (Species::Panda, panda_age),
            t if t == &entities::PARROT => (Species::Parrot, adult_only),
            t if t == &entities::PIG => (Species::Pig, ordinary_age),
            t if t == &entities::RABBIT => (Species::Rabbit(None), baby_after_first),
            t if t == &entities::SHEEP => (Species::Sheep, ordinary_age),
            t if t == &entities::STRIDER => (Species::Strider, ordinary_age),
            t if t == &entities::TURTLE => (Species::Turtle, ordinary_age),
            t if t == &entities::WOLF => (Species::Wolf(None), ordinary_age),
            _ => return None,
        };

        Some(Self {
            entity_type,
            species,
            ageable,
        })
    }

    pub(super) fn finalize(
        &mut self,
        biome: BiomeRef,
        pos: BlockPos,
        world: &World,
        random: &mut RandomSource,
    ) -> SpawnState {
        let mut overrides = SpawnOverrides::default();
        let mut passenger = None;

        overrides.properties = match &mut self.species {
            Species::Plain => Properties::None,
            Species::Camel => Properties::Camel {
                last_pose_change_tick: (world.game_time() - CAMEL_STAND_UP_TICKS - 1).max(0),
            },
            Species::Chicken => Properties::Chicken {
                variant: REGISTRY
                    .chicken_variants
                    .select_spawn_variant(biome, random),
                sound_variant: required_variant(
                    REGISTRY.chicken_sound_variants.pick_random(random),
                ),
            },
            Species::Cow => Properties::Cow {
                variant: REGISTRY.cow_variants.select_spawn_variant(biome, random),
                sound_variant: required_variant(REGISTRY.cow_sound_variants.pick_random(random)),
            },
            Species::Donkey => {
                overrides.equine_attributes = Some(EquineAttributes::sample(random, false));
                Properties::None
            }
            Species::Fox(group_variant) => {
                let variant = *group_variant.get_or_insert_with(|| variants::fox_variant(biome));
                overrides.is_baby = self.ageable.group_size() >= FOX_ADULT_GROUP_MEMBERS;
                overrides.main_hand_item = variants::fox_mouth_item(random);

                Properties::Fox(variant)
            }
            Species::Frog => variants::frog_properties(biome, random),
            Species::Goat => Properties::Goat {
                long_jump_cooldown_ticks: GOAT_LONG_JUMP_COOLDOWN_TICKS.sample(random),
                ram_cooldown_ticks: GOAT_RAM_COOLDOWN_TICKS.sample(random),
                screaming: random.next_f64() < GOAT_SCREAMING_CHANCE,
                missing_left_horn: (random.next_f32() < GOAT_MISSING_HORN_CHANCE)
                    .then(|| random.next_bool()),
            },
            Species::Horse(group_variant) => {
                let variant =
                    *group_variant.get_or_insert_with(|| choose(&HorseVariant::VALUES, random));
                let markings = choose(&HorseMarkings::VALUES, random);
                overrides.equine_attributes = Some(EquineAttributes::sample(random, true));

                Properties::Horse { variant, markings }
            }
            Species::Llama(group_variant) => {
                let strength = equine::llama_strength(random);
                let variant =
                    *group_variant.get_or_insert_with(|| choose(&LlamaVariant::VALUES, random));
                overrides.equine_attributes = Some(EquineAttributes::sample(random, false));

                Properties::Llama { variant, strength }
            }
            Species::Panda => Properties::Panda {
                main_gene: PandaGene::sample(random),
                hidden_gene: PandaGene::sample(random),
            },
            Species::Parrot => Properties::Parrot(choose(&ParrotVariant::VALUES, random)),
            Species::Pig => Properties::Pig {
                variant: REGISTRY.pig_variants.select_spawn_variant(biome, random),
                sound_variant: required_variant(REGISTRY.pig_sound_variants.pick_random(random)),
            },
            Species::Rabbit(group_variant) => {
                let variant = variants::rabbit_variant(biome, random);

                Properties::Rabbit(*group_variant.get_or_insert(variant))
            }
            Species::Sheep => Properties::Sheep(SheepEntity::random_sheep_color(biome, random)),
            Species::Strider => {
                let difficulty = world.worldgen_difficulty();
                let spawning_monsters =
                    world.get_game_rule(&SPAWN_MOBS) && world.get_game_rule(&SPAWN_MONSTERS);
                (self.ageable, passenger, overrides.saddled) =
                    strider::sample_passenger(self.ageable, difficulty, spawning_monsters, random);

                Properties::None
            }
            Species::Turtle => Properties::Turtle { home: pos },
            Species::Wolf(group_variant) => {
                variants::wolf_properties(group_variant, &mut self.ageable, biome, random)
            }
        };

        overrides.is_baby |= self.ageable.finalize_ageable_spawn(|| random.next_f32());

        SpawnState {
            data: SpawnData {
                entity_type: self.entity_type,
                mob: MobSpawnState::sample(random),
                overrides,
            },
            passenger,
        }
    }
}

fn choose<T: Copy>(values: &[T], random: &mut RandomSource) -> T {
    values[random.next_i32_bounded(values.len() as i32) as usize]
}

fn required_variant<T>(variant: Option<T>) -> T {
    match variant {
        Some(variant) => variant,
        None => panic!("worldgen spawn sound variant registry must be populated"),
    }
}

#[derive(Debug)]
pub(super) struct SpawnState {
    pub data: SpawnData,
    pub passenger: Option<SpawnData>,
}

#[derive(Debug)]
pub(super) struct SpawnData {
    pub entity_type: EntityTypeRef,
    mob: MobSpawnState,
    overrides: SpawnOverrides,
}

#[derive(Debug, Default)]
struct SpawnOverrides {
    is_baby: bool,
    properties: Properties,
    equine_attributes: Option<EquineAttributes>,
    main_hand_item: Option<ItemRef>,
    head_item: Option<ItemRef>,
    saddled: bool,
    can_pick_up_loot: Option<bool>,
}

impl SpawnData {
    pub(super) fn apply(&self, entity: &dyn Entity) {
        let Some(mob) = entity.as_mob() else {
            panic!(
                "worldgen spawn factory for {} must create a mob",
                self.entity_type.key
            );
        };

        let overrides = &self.overrides;
        if let Some(attributes) = overrides.equine_attributes {
            attributes.apply(mob);
        }

        if overrides.is_baby {
            if let Some(ageable) = entity.as_ageable_mob() {
                ageable.set_age(ageable.get_baby_start_age());
            } else {
                mob.set_baby(true);
            }
        }

        self.mob.apply(mob);
        if let Some(can_pick_up_loot) = overrides.can_pick_up_loot {
            mob.set_can_pick_up_loot(can_pick_up_loot);
        }

        if let Some(item) = overrides.main_hand_item {
            mob.living_base()
                .equipment()
                .lock()
                .set(EquipmentSlot::MainHand, ItemStack::new(item));
        }

        if let Some(item) = overrides.head_item {
            mob.living_base()
                .equipment()
                .lock()
                .set(EquipmentSlot::Head, ItemStack::new(item));
            mob.set_equipment_drop_chance(EquipmentSlot::Head, 0.0);
        }

        if overrides.saddled {
            mob.living_base().equipment().lock().set(
                EquipmentSlot::Saddle,
                ItemStack::new(&vanilla_items::SADDLE),
            );
            mob.set_guaranteed_drop(EquipmentSlot::Saddle);
        }

        entity.apply_worldgen_spawn_properties(overrides.properties);
    }
}
