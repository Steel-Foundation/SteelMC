//! Vanilla Sheep entity: wool color, shearing and eating grass.

mod color_spawn_rules;

use crate::entity::prelude::*;
use std::sync::Arc;
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::nbt::NbtValueInput as _;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::vanilla_components::{DYE, SHEEP_COLOR};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::recipe::{CraftingInput, vanilla_recipe_types};
use steel_registry::vanilla_entity_data::SheepEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    DyeColor, REGISTRY, TaggedRegistryExt, sound_events, vanilla_items, vanilla_loot_tables,
};
use steel_utils::Downcast as _;
use steel_utils::locks::CheckedMutex;
use steel_utils::random::legacy_random::LegacyRandom;

use crate::behavior::InteractionResult;
use crate::entity::ageable::{self, Age, Ageable, AgeableProps};
use crate::entity::ai::goal::{
    BreedGoal, EatBlockGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal,
    RandomLookAroundGoal, TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::animal::{self, Breeding, BreedingProps};
use crate::entity::component::definition::Definition;
use crate::entity::component::definition::{FinalizeSpawnCtx, MobInteractCtx};
use crate::entity::component::pipeline::{Flow, Stage, StageId};
use crate::entity::shearable::{self, Shearable};
use crate::entity::{
    DefaultEntityPush, EntityBase, EntityProps, LivingEntityBase, LivingProps, MobBase, MobProps,
    SharedEntity, SpawnRule, mob,
};
use crate::inventory::recipe_manager;
use crate::world::World;

/// Low 4 bits of the synced wool byte: the wool color id.
const COLOR_ID_MASK: i8 = 0x0F;
/// Bit 5 of the synced wool byte: sheared.
const SHEARED_BIT: i8 = 0x10;

/// Vanilla sheep entity.
#[entity_behavior(class = "Sheep", spawn_rule)]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_sheep, load = Self::load_sheep)]
#[entity(key = "steel:entity/sheep", constructors, definition = Self::define)]
pub struct SheepEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    living_base: LivingEntityBase,
    #[entity(component)]
    mob_base: MobBase,
    #[entity(component)]
    age: Age,
    #[entity(component)]
    breeding: Breeding,
    #[entity(synced)]
    entity_data: CheckedMutex<SheepEntityData>,
}

impl SheepEntity {
    /// Vanilla `SpawnPlacements` predicate: `Animal.checkAnimalSpawnRules`.
    pub(crate) const SPAWN_RULE: SpawnRule = |entity_type, level, spawn_reason, pos, random| {
        animal::check_animal_spawn_rules(entity_type, level, spawn_reason, pos, random)
    };

    const AGEABLE: AgeableProps = AgeableProps {
        breed_offspring: Self::breed_offspring,
    };
    const BREEDING: BreedingProps = BreedingProps::new(Self::is_food);
    const LIVING: LivingProps<Self> = LivingProps {
        hurt_sound: |_, _| Some(&sound_events::ENTITY_SHEEP_HURT),
        death_sound: |_| Some(&sound_events::ENTITY_SHEEP_DEATH),
        sheep_loot_state: |sheep| Some((sheep.color(), sheep.is_sheared())),
        ..animal::living_props()
    };
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |_| Some(&sound_events::ENTITY_SHEEP_AMBIENT),
        ..animal::mob_props()
    };
    const GOALS: StageId = StageId("sheep.goals");
    const SHEAR: StageId = StageId("sheep.shear");
    const PICK_COLOR: StageId = StageId("sheep.pick_color");
    const ATE: StageId = StageId("sheep.ate");

    /// Vanilla `Sheep`: the animal chains with sheep goals, shearing before
    /// feeding, and the spawn wool color before `AgeableMob.finalizeSpawn`.
    fn define() -> Definition<Self> {
        let mut definition = animal::definition::<Self>();
        // `Mob`'s constructor registers goals before `Animal`'s sets its malus.
        definition.construct = definition.construct.before(
            animal::PATHFINDING_MALUS,
            [Stage::effect(Self::GOALS, Self::register_goals)],
        );
        definition.mob_interact = definition.mob_interact.before(
            animal::MOB_INTERACT,
            [Stage::new(Self::SHEAR, Self::shear_by_player)],
        );
        definition.finalize_spawn = definition.finalize_spawn.before(
            ageable::FINALIZE_SPAWN,
            [Stage::new(Self::PICK_COLOR, Self::pick_spawn_color)],
        );
        definition.ate = definition
            .ate
            .then([Stage::effect(Self::ATE, Self::mob_ate)]);
        definition
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = SheepEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &Self::LIVING, &mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            mob_base: MobBase::new(&Self::MOB, Self::definition()),
            age: Age::new(&Self::AGEABLE),
            breeding: Breeding::new(&Self::BREEDING),
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// `Sheep.isFood`.
    #[must_use]
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::SHEEP_FOOD)
    }

    /// The wool color.
    #[must_use]
    pub fn color(&self) -> DyeColor {
        DyeColor::by_id(i32::from(
            *self.entity_data.lock().wool.get() & COLOR_ID_MASK,
        ))
    }

    /// Sets the wool color, keeping the sheared bit.
    pub fn set_color(&self, color: DyeColor) {
        let mut entity_data = self.entity_data.lock();
        let current = *entity_data.wool.get();
        entity_data
            .wool
            .set((current & !COLOR_ID_MASK) | ((color.id() & 15) as i8));
    }

    /// Whether the wool has been sheared off.
    #[must_use]
    pub fn is_sheared(&self) -> bool {
        (*self.entity_data.lock().wool.get() & SHEARED_BIT) != 0
    }

    /// Sets the sheared state, keeping the color bits.
    pub fn set_sheared(&self, sheared: bool) {
        let mut entity_data = self.entity_data.lock();
        let current = *entity_data.wool.get();
        let next = if sheared {
            current | SHEARED_BIT
        } else {
            current & !SHEARED_BIT
        };
        entity_data.wool.set(next);
    }

    /// `Sheep.registerGoals`.
    fn register_goals(mob_base: &MobBase) {
        let mut goal_selector = mob_base.goal_selector().lock();
        goal_selector.add_goal(0, FloatGoal::new(mob_base));
        goal_selector.add_goal(1, PanicGoal::new(1.25));
        goal_selector.add_goal(2, BreedGoal::new(1.0));
        goal_selector.add_goal(3, TemptGoal::new(1.1, Self::is_food, false));
        goal_selector.add_goal(4, FollowParentGoal::new(1.1));
        goal_selector.add_goal(5, EatBlockGoal::new());
        goal_selector.add_goal(6, WaterAvoidingRandomStrollGoal::new(1.0));
        goal_selector.add_goal(7, LookAtPlayerGoal::new(6.0));
        goal_selector.add_goal(8, RandomLookAroundGoal::new());
    }

    /// `Sheep.mobInteract` before `super`: shears always end the interaction.
    fn shear_by_player(&self, ctx: &mut MobInteractCtx<'_>) -> Flow {
        let holds_shears = ctx
            .player
            .inventory
            .lock()
            .get_item_in_hand(ctx.hand)
            .is(&vanilla_items::SHEARS);
        if !holds_shears {
            return Flow::Continue;
        }
        ctx.result = if self.ready_for_shearing() {
            shearable::shear_by_player(self, ctx.player, ctx.hand);
            InteractionResult::SuccessServer
        } else {
            InteractionResult::Consume
        };
        Flow::Stop
    }

    /// `Sheep.ate`: eating grass regrows the wool and speeds up a lamb's growth.
    fn mob_ate(&self) {
        self.set_sheared(false);
        let ageable = self.animal().ageable();
        if ageable.can_age_up() {
            ageable.age_up(60, false);
        }
    }

    /// `Sheep.finalizeSpawn` before `super`: the wool color for the biome.
    fn pick_spawn_color(&self, ctx: &mut FinalizeSpawnCtx<'_>) -> Flow {
        let biome = ctx.world.biome_at(self.block_position());
        let mut random = LegacyRandom::from_seed(rand::random());
        self.set_color(color_spawn_rules::sheep_color(biome, &mut random));
        Flow::Continue
    }

    /// `Sheep.getBreedOffspring`: a lamb with the parents' colors mixed.
    fn breed_offspring(
        parent: &Ageable<'_>,
        world: &Arc<World>,
        partner: &Ageable<'_>,
    ) -> Option<SharedEntity> {
        let offspring = parent.create_offspring(world)?;
        let color_of =
            |ageable: &Ageable<'_>| ageable.entity().downcast_ref::<Self>().map(Self::color);
        if let (Some(color1), Some(color2), Some(lamb)) = (
            color_of(parent),
            color_of(partner),
            offspring.downcast_ref::<Self>(),
        ) {
            lamb.set_color(mixed_color(color1, color2));
        }
        Some(offspring)
    }
}

/// `DyeColor.getMixedColor`: the color two dyes craft into, or either parent's
/// color at random when they do not combine.
fn mixed_color(color1: DyeColor, color2: DyeColor) -> DyeColor {
    crafted_mix(color1, color2).unwrap_or_else(|| {
        if rand::random::<bool>() {
            color1
        } else {
            color2
        }
    })
}

/// Crafts every dye of `color1` with every dye of `color2` in a 2x1 grid and
/// returns the first resulting dye color.
fn crafted_mix(color1: DyeColor, color2: DyeColor) -> Option<DyeColor> {
    let dyes_of = |color: DyeColor| {
        REGISTRY
            .items
            .iter()
            .filter_map(move |(_, item)| (item.components.get(DYE) == Some(color)).then_some(item))
    };
    for dye1 in dyes_of(color1) {
        for dye2 in dyes_of(color2) {
            let input = CraftingInput::new(2, 1, vec![ItemStack::new(dye1), ItemStack::new(dye2)]);
            let Some(recipe) = REGISTRY
                .recipes
                .find_match(&vanilla_recipe_types::CRAFTING, &input)
            else {
                continue;
            };
            if let Some(color) = recipe_manager::assemble_recipe(recipe, &input)
                .get(DYE)
                .copied()
            {
                return Some(color);
            }
        }
    }
    None
}

impl Shearable for SheepEntity {
    /// `Sheep.shear`: drops each sheared item one at a time with a small random push.
    fn shear(&self, world: &World, sound_source: SoundSource, tool: &ItemStack) {
        world.play_sound_at(
            &sound_events::ENTITY_SHEEP_SHEAR,
            sound_source,
            self.position(),
            1.0,
            1.0,
            None,
        );
        for drop in self
            .living()
            .shearing_loot(&vanilla_loot_tables::SHEARING_SHEEP, tool)
        {
            for _ in 0..drop.count() {
                let Some(item) = self.spawn_at_location(drop.copy_with_count(1), 1.0) else {
                    continue;
                };
                let jitter = DVec3::new(
                    f64::from((rand::random::<f32>() - rand::random::<f32>()) * 0.1),
                    f64::from(rand::random::<f32>() * 0.05),
                    f64::from((rand::random::<f32>() - rand::random::<f32>()) * 0.1),
                );
                item.set_velocity(item.velocity() + jitter);
            }
        }
        self.set_sheared(true);
    }

    fn ready_for_shearing(&self) -> bool {
        !self.is_sheared() && !self.animal().ageable().is_baby()
    }
}

impl DefaultEntityPush for SheepEntity {}

impl SheepEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        apply_implicit_item_components: |sheep, item_stack| {
            if let Some(color) = item_stack.get(SHEEP_COLOR) {
                sheep.set_color(*color);
            }
        },
        play_step_sound: |sheep, _, _| {
            sheep.play_sound(&sound_events::ENTITY_SHEEP_STEP, 0.15, 1.0);
        },
        ..mob::entity_props()
    };

    fn save_sheep(&self, nbt: &mut NbtCompound) {
        nbt.insert("Sheared", i8::from(self.is_sheared()));
        // `DyeColor.LEGACY_ID_CODEC`: the color id as a byte.
        nbt.insert("Color", self.color().id() as i8);
    }

    fn load_sheep(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_sheared(nbt.byte_value("Sheared").is_some_and(|value| value != 0));
        self.set_color(DyeColor::by_id(i32::from(
            nbt.get("Color").and_then(|tag| tag.codec_i8()).unwrap_or(0),
        )));
    }
}

#[cfg(test)]
mod tests;
