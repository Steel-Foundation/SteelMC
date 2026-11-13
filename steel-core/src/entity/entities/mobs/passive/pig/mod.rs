//! Vanilla Pig entity: variants, saddles and carrot-on-a-stick steering.

use crate::entity::Arc;
use crate::entity::EntitySpawnReason;
use crate::entity::LivingTable;
use crate::entity::MobTable;
use crate::entity::SharedEntity;
use crate::entity::SpawnGroupData;
use crate::entity::ageable;
use crate::entity::living_entity;
use crate::entity::mob;
use crate::entity::prelude::*;
use crate::player::Player;
use crate::world::World;
use steel_registry::sound_event::SoundEventRef;
use steel_utils::types::InteractionHand;

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
use crate::entity::EntityTable;
use crate::entity::ageable::{Age, AgeableProps, AgeableTable};
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::animal::{self, AnimalProps, AnimalTable, Breeding};
use crate::entity::item_based_steering::ItemBasedSteering;
use crate::entity::variant::{self, Variants};
use crate::entity::{
    DefaultEntityPush, EntityBase, EntityProps, ItemSteerable, LivingEntityBase, LivingProps,
    MobBase, MobProps, SpawnRule,
};
use crate::inventory::equipment::EquipmentSlot;

/// Vanilla pig entity.
// TODO: `Pig.thunderHit` turns pigs into zombified piglins once lightning bolts and
// `ZombifiedPiglin` are ported.
#[entity_behavior(class = "Pig", spawn_rule)]
#[derive(Entity)]
#[entity(save = variant::save, load = variant::load)]
#[entity(key = "steel:entity/pig", constructors)]
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

impl AgeableTable for PigEntity {
    const AGEABLE: AgeableProps<Self> = AgeableProps {
        breed_offspring: variant::breed_offspring::<Self>,
    };
}

impl AnimalTable for PigEntity {
    const ANIMAL: AnimalProps<Self> = AnimalProps {
        is_food: |_, item_stack| Self::is_food(item_stack),
        play_eating_sound: |pig| pig.living().make_sound(Some(pig.sound_set().eat_sound)),
    };
}

impl PigEntity {
    /// Vanilla `SpawnPlacements` predicate: `Animal.checkAnimalSpawnRules`.
    pub(crate) const SPAWN_RULE: SpawnRule = |entity_type, level, spawn_reason, pos, random| {
        animal::check_animal_spawn_rules(entity_type, level, spawn_reason, pos, random)
    };

    /// `Mob`'s constructor registers goals before `Animal`'s sets its malus.
    fn construct(mob_base: &MobBase) {
        Self::register_goals(mob_base);
        animal::construct(mob_base);
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = PigEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            mob_base: MobBase::new(Self::construct),
            age: Age::new(),
            breeding: Breeding::new(),
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
    fn ride(&self, player: &Player, hand: InteractionHand) -> Option<InteractionResult> {
        let has_food = Self::is_food(player.inventory.lock().get_item_in_hand(hand));
        if has_food
            || !self.mob().is_saddled()
            || self.is_vehicle()
            || player.is_secondary_use_active()
        {
            return None;
        }

        if let Some(pig) = self
            .level()
            .and_then(|world| world.get_entity_by_id(self.id()))
        {
            player.start_riding(&pig);
        }
        Some(InteractionResult::Success)
    }

    /// `Pig.mobInteract` after `super` declined: equip a held saddle.
    fn put_on_saddle(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        let held = player.inventory.lock().get_item_in_hand(hand).clone();
        if self
            .living()
            .is_equippable_in_slot(&held, EquipmentSlot::Saddle)
        {
            return self
                .living()
                .interact_living_entity_with_equippable(player, hand);
        }
        InteractionResult::Pass
    }

    /// `Pig.tickRidden`: face where the rider looks and run the boost.
    /// `Pig.tickRidden`: after `super`, faces where the rider looks and ticks
    /// the boost.
    fn living_tick_ridden(&self, controller: &Player, ridden_input: DVec3) {
        living_entity::tick_ridden(self, controller, ridden_input);
        let (yaw, pitch) = controller.rotation();
        self.set_rotation((yaw, pitch * 0.5));
        self.base.set_old_yaw_to_current();
        let yaw = self.rotation().0;
        self.set_y_body_rot(yaw);
        self.set_y_head_rot(yaw);
        self.tick_boost();
    }
}

impl MobTable for PigEntity {
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |pig| Some(pig.sound_set().ambient_sound),
        mob_interact: Self::mob_interact,
        finalize_spawn: Self::mob_finalize_spawn,
        ..animal::mob_props()
    };
}

impl PigEntity {
    /// `Pig.mobInteract`: mounting a saddled pig comes before `super`,
    /// saddling after `super` declined.
    fn mob_interact(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        if let Some(result) = self.ride(player, hand) {
            return result;
        }
        let result = animal::mob_interact(self, player, hand);
        if result.consumes_action() {
            return result;
        }
        self.put_on_saddle(player, hand)
    }

    /// `Pig.finalizeSpawn` picks the variants before `super`.
    fn mob_finalize_spawn(
        &self,
        world: &Arc<World>,
        reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        variant::pick_spawn_variants(self, world);
        ageable::finalize_spawn(self, world, reason, group_data)
    }
}

impl LivingTable for PigEntity {
    const LIVING: LivingProps<Self> = LivingProps {
        hurt_sound: |pig, _| Some(pig.sound_set().hurt_sound),
        death_sound: |pig| Some(pig.sound_set().death_sound),
        ridden_input: |_, _, _| DVec3::new(0.0, 0.0, 1.0),
        ridden_speed: Self::living_ridden_speed,
        can_use_slot: Self::living_can_use_slot,
        can_dispenser_equip_into_slot: |pig, slot| {
            slot == EquipmentSlot::Saddle || mob::can_dispenser_equip_into_slot(pig, slot)
        },
        equip_sound: Self::living_equip_sound,
        tick_ridden: Self::living_tick_ridden,
        ..animal::living_props()
    };
}

impl PigEntity {
    fn living_ridden_speed(&self, _: &Player) -> f32 {
        let movement_speed = self
            .living()
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED);
        (movement_speed * 0.225 * f64::from(self.boost_factor())) as f32
    }

    /// The saddle slot is the pig's own; other slots fall to `super`.
    fn living_can_use_slot(&self, slot: EquipmentSlot) -> bool {
        if slot == EquipmentSlot::Saddle {
            return EntityMethods::is_alive(self) && !self.animal().ageable().is_baby();
        }
        living_entity::can_use_slot(self, slot)
    }

    fn living_equip_sound(&self, slot: EquipmentSlot, stack: &ItemStack) -> Option<SoundEventRef> {
        if slot == EquipmentSlot::Saddle {
            return Some(&sound_events::ENTITY_PIG_SADDLE);
        }
        living_entity::equip_sound(self, slot, stack)
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

impl EntityTable for PigEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        apply_implicit_item_components: Self::entity_apply_implicit_item_components,
        play_step_sound: Self::entity_play_step_sound,
        // `Pig.getControllingPassenger`: a rider holding a carrot on a stick
        // steers, else `super` decides.
        controlling_passenger: |pig| {
            pig.rider_with_carrot()
                .or_else(|| mob::controlling_passenger(pig))
        },
        ..animal::entity_props()
    };
}

impl PigEntity {
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

    /// `Pig.getControllingPassenger` before `super`: a rider holding a carrot
    /// on a stick steers a saddled pig.
    fn rider_with_carrot(&self) -> Option<SharedEntity> {
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
        None
    }
}

#[cfg(test)]
mod tests;
