//! Vanilla Pig entity: variants, saddles and carrot-on-a-stick steering.

use crate::entity::prelude::*;

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::data_components::vanilla_components::{PIG_SOUND_VARIANT, PIG_VARIANT};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::pig_sound_variant::{PigAge, PigSoundVariantRef, PigSoundVariantRegistry};
use steel_registry::pig_variant::{PigVariantRef, PigVariantRegistry};
use steel_registry::vanilla_entity_data::PigEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, RegistryReference, TaggedRegistryExt, sound_events, vanilla_attributes, vanilla_items,
};
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::InteractionResult;
use crate::entity::ageable::{self, Age, AgeableProps};
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::animal::{self, Breeding, BreedingProps};
use crate::entity::component::definition::MobDefinition;
use crate::entity::component::entries::MobInteractCtx;
use crate::entity::component::pipeline::{Flow, Stage, StageId};
use crate::entity::item_based_steering::ItemBasedSteering;
use crate::entity::variant::{self, Variants};
use crate::entity::{
    DefaultEntityPush, EntityBase, EntityProps, ItemSteerable, LivingEntityBase, LivingProps,
    MobBase, MobProps, SharedEntity, SpawnRule, mob,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::player::Player;

/// Vanilla pig entity.
// TODO: `Pig.thunderHit` turns pigs into zombified piglins once lightning bolts and
// `ZombifiedPiglin` are ported.
#[entity_behavior(class = "Pig", spawn_rule)]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = variant::save, load = variant::load)]
#[entity(key = "steel:entity/pig", constructors, definition = Self::define)]
pub struct PigEntity {
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
    steering: CheckedMutex<ItemBasedSteering>,
    #[entity(synced)]
    entity_data: CheckedMutex<PigEntityData>,
}

impl PigEntity {
    /// Vanilla `SpawnPlacements` predicate: `Animal.checkAnimalSpawnRules`.
    pub(crate) const SPAWN_RULE: SpawnRule = |entity_type, level, spawn_reason, pos, random| {
        animal::check_animal_spawn_rules(entity_type, level, spawn_reason, pos, random)
    };

    const AGEABLE: AgeableProps = AgeableProps {
        breed_offspring: variant::breed_offspring::<Self>,
    };
    const BREEDING: BreedingProps = BreedingProps {
        play_eating_sound: |pig| {
            if let Some(pig) = steel_utils::Downcast::downcast_ref::<Self>(pig) {
                pig.living().make_sound(Some(pig.sound_set().eat_sound));
            }
        },
        ..BreedingProps::new(Self::is_food)
    };
    const LIVING: LivingProps<Self> = LivingProps {
        hurt_sound: |pig, _| Some(pig.sound_set().hurt_sound),
        death_sound: |pig| Some(pig.sound_set().death_sound),
        can_use_slot: |pig, slot| {
            slot != EquipmentSlot::Saddle
                || (Entity::is_alive(pig) && !pig.animal().ageable().is_baby())
        },
        can_dispenser_equip_into_slot: |pig, slot| {
            slot == EquipmentSlot::Saddle || pig.mob().can_pick_up_loot()
        },
        equip_sound: |pig, slot, stack| {
            if slot == EquipmentSlot::Saddle {
                Some(&sound_events::ENTITY_PIG_SADDLE)
            } else {
                pig.living().equip_sound_default(slot, stack)
            }
        },
        tick_ridden: Self::living_tick_ridden,
        ridden_input: |_, _, _| DVec3::new(0.0, 0.0, 1.0),
        ridden_speed: |pig, _| {
            let movement_speed = pig
                .living()
                .attributes()
                .lock()
                .required_value(vanilla_attributes::MOVEMENT_SPEED);
            (movement_speed * 0.225 * f64::from(pig.boost_factor())) as f32
        },
        ..animal::living_props()
    };
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |pig| Some(pig.sound_set().ambient_sound),
        ..animal::mob_props()
    };
    const GOALS: StageId = StageId("pig.goals");
    const RIDE: StageId = StageId("pig.ride");
    const SADDLE: StageId = StageId("pig.saddle");

    /// Vanilla `Pig`: the animal chains with pig goals, mounting a saddled pig
    /// before feeding, saddling after it, and variant selection on spawn.
    fn define() -> MobDefinition<Self> {
        let mut definition = animal::definition::<Self>();
        // `Mob`'s constructor registers goals before `Animal`'s sets its malus.
        definition.construct = definition.construct.before(
            animal::PATHFINDING_MALUS,
            [Stage::effect(Self::GOALS, Self::register_goals)],
        );
        definition.mob_interact = definition
            .mob_interact
            .before(animal::MOB_INTERACT, [Stage::new(Self::RIDE, Self::ride)])
            .then([Stage::new(Self::SADDLE, Self::put_on_saddle)]);
        definition.finalize_spawn = definition.finalize_spawn.before(
            ageable::FINALIZE_SPAWN,
            [variant::pick_spawn_variants::<Self>()],
        );
        definition
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = PigEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &Self::LIVING, &mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            mob_base: MobBase::new(&Self::MOB, Self::definition()),
            age: Age::new(&Self::AGEABLE),
            breeding: Breeding::new(&Self::BREEDING),
            steering: CheckedMutex::new(ItemBasedSteering::new()),
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// `Pig.isFood`.
    #[must_use]
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::PIG_FOOD)
    }

    /// `Pig.registerGoals`.
    fn register_goals(mob_base: &MobBase) {
        let mut goal_selector = mob_base.goal_selector().lock();
        goal_selector.add_goal(0, FloatGoal::new(mob_base));
        goal_selector.add_goal(1, PanicGoal::new(1.25));
        goal_selector.add_goal(3, BreedGoal::new(1.0));
        goal_selector.add_goal(
            4,
            TemptGoal::new(
                1.2,
                |item_stack| item_stack.is(&vanilla_items::CARROT_ON_A_STICK),
                false,
            ),
        );
        goal_selector.add_goal(4, TemptGoal::new(1.2, Self::is_food, false));
        goal_selector.add_goal(5, FollowParentGoal::new(1.1));
        goal_selector.add_goal(6, WaterAvoidingRandomStrollGoal::new(1.0));
        goal_selector.add_goal(7, LookAtPlayerGoal::new(6.0));
        goal_selector.add_goal(8, RandomLookAroundGoal::new());
    }

    /// `Pig.getSoundSet`: the baby or adult sounds of the sound variant.
    fn sound_set(&self) -> &'static PigAge {
        let sound_variant = self.sound_variant();
        if self.animal().ageable().is_baby() {
            &sound_variant.baby_sounds
        } else {
            &sound_variant.adult_sounds
        }
    }

    /// `Pig.mobInteract` before `super`: a player without food mounts a saddled pig.
    fn ride(&self, ctx: &mut MobInteractCtx<'_>) -> Flow {
        let player = ctx.player;
        let has_food = Self::is_food(player.inventory.lock().get_item_in_hand(ctx.hand));
        if has_food
            || !self.mob().is_saddled()
            || self.is_vehicle()
            || player.is_secondary_use_active()
        {
            return Flow::Continue;
        }

        if let Some(pig) = self
            .level()
            .and_then(|world| world.get_entity_by_id(self.id()))
        {
            player.start_riding(&pig);
        }
        ctx.result = InteractionResult::Success;
        Flow::Stop
    }

    /// `Pig.mobInteract` after `super` declined: equip a held saddle.
    fn put_on_saddle(&self, ctx: &mut MobInteractCtx<'_>) -> Flow {
        let held = ctx
            .player
            .inventory
            .lock()
            .get_item_in_hand(ctx.hand)
            .clone();
        if self
            .living()
            .is_equippable_in_slot(&held, EquipmentSlot::Saddle)
        {
            ctx.result = self
                .living()
                .interact_living_entity_with_equippable(ctx.player, ctx.hand);
        }
        Flow::Continue
    }

    /// `Pig.tickRidden`: face where the rider looks and run the boost.
    fn living_tick_ridden(&self, controller: &Player, _ridden_input: DVec3) {
        let (yaw, pitch) = controller.rotation();
        self.set_rotation((yaw, pitch * 0.5));
        self.base.set_old_yaw_to_current();
        let yaw = self.rotation().0;
        self.set_y_body_rot(yaw);
        self.set_y_head_rot(yaw);
        self.tick_boost();
    }
}

impl Variants for PigEntity {
    type VariantRegistry = PigVariantRegistry;
    type SoundVariantRegistry = PigSoundVariantRegistry;

    fn variant_registry() -> &'static PigVariantRegistry {
        &REGISTRY.pig_variants
    }

    fn sound_variant_registry() -> &'static PigSoundVariantRegistry {
        &REGISTRY.pig_sound_variants
    }

    fn variant(&self) -> PigVariantRef {
        self.entity_data.lock().variant.get().value()
    }

    fn set_variant(&self, variant: PigVariantRef) {
        self.entity_data
            .lock()
            .variant
            .set(RegistryReference::new(variant));
    }

    fn sound_variant(&self) -> PigSoundVariantRef {
        self.entity_data.lock().sound_variant.get().value()
    }

    fn set_sound_variant(&self, sound_variant: PigSoundVariantRef) {
        self.entity_data
            .lock()
            .sound_variant
            .set(RegistryReference::new(sound_variant));
    }
}

impl ItemSteerable for PigEntity {
    fn item_based_steering(&self) -> &CheckedMutex<ItemBasedSteering> {
        &self.steering
    }

    fn boost_time_total(&self) -> i32 {
        *self.entity_data.lock().boost_time.get()
    }

    fn set_boost_time_total(&self, boost_time_total: i32) {
        self.entity_data.lock().boost_time.set(boost_time_total);
    }
}

impl DefaultEntityPush for PigEntity {}

impl PigEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        apply_implicit_item_components: Self::entity_apply_implicit_item_components,
        play_step_sound: Self::entity_play_step_sound,
        controlling_passenger: Self::entity_controlling_passenger,
        ..mob::entity_props()
    };

    fn entity_apply_implicit_item_components(&self, item_stack: &ItemStack) {
        if let Some(variant) = item_stack.get(PIG_VARIANT) {
            self.set_variant(variant.value());
        }
        if let Some(sound_variant) = item_stack.get(PIG_SOUND_VARIANT) {
            self.set_sound_variant(sound_variant.value());
        }
    }

    fn entity_play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(self.sound_set().step_sound, 0.15, 1.0);
    }

    /// `Pig.getControllingPassenger`: a rider holding a carrot on a stick steers
    /// a saddled pig.
    fn entity_controlling_passenger(&self) -> Option<SharedEntity> {
        if self.mob().is_saddled()
            && let Some(passenger) = self.first_passenger()
            && passenger.as_player().is_some_and(|player| {
                player.living().is_holding(&mut |item_stack: &ItemStack| {
                    item_stack.is(&vanilla_items::CARROT_ON_A_STICK)
                })
            })
        {
            return Some(passenger);
        }
        self.mob().controlling_passenger_mob()
    }
}

#[cfg(test)]
mod tests;
