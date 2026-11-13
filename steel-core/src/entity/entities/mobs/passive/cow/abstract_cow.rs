//! Vanilla `AbstractCow`: what cows and mooshrooms share on top of `Animal`.

use crate::player::Player;
use steel_registry::cow_sound_variant::CowSoundVariant;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, sound_events, vanilla_cow_sound_variants, vanilla_items,
};
use steel_utils::types::InteractionHand;

use crate::behavior::{ITEM_BEHAVIORS, InteractionResult};
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::animal::{self, Parts};
use crate::entity::prelude::*;
use crate::entity::{EntityProps, LivingProps, MobBase, MobProps};
use crate::player::player_inventory::Prediction;

/// `AbstractCow`'s constructor chain: `Mob`'s constructor registers the cow
/// goals before `Animal`'s sets its malus.
pub fn construct(mob_base: &MobBase) {
    register_goals(mob_base);
    animal::construct(mob_base);
}

/// `AbstractCow.getSoundSet`: the classic cow sounds, unless a type picks others.
const CLASSIC_SOUNDS: &CowSoundVariant = &vanilla_cow_sound_variants::CLASSIC;

/// `AbstractCow`'s changes to the animal living defaults.
#[must_use]
pub const fn living_props<E: Parts>() -> LivingProps<E> {
    LivingProps {
        sound_volume: |_| 0.4,
        hurt_sound: |_, _| Some(CLASSIC_SOUNDS.hurt_sound),
        death_sound: |_| Some(CLASSIC_SOUNDS.death_sound),
        ..animal::living_props()
    }
}

/// `AbstractCow`'s changes to the animal mob defaults.
#[must_use]
pub const fn mob_props<E: Parts>() -> MobProps<E> {
    MobProps {
        ambient_sound: |_| Some(CLASSIC_SOUNDS.ambient_sound),
        mob_interact: |cow, player, hand| {
            if let Some(result) = milk(cow, player, hand) {
                return result;
            }
            (const { animal::mob_props::<E>().mob_interact })(cow, player, hand)
        },
        ..animal::mob_props()
    }
}

/// `AbstractCow`'s changes to the mob entity hooks.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        play_step_sound: |cow, _, _| cow.play_sound(CLASSIC_SOUNDS.step_sound, 0.15, 1.0),
        ..animal::entity_props()
    }
}

/// `AbstractCow.isFood`.
#[must_use]
pub fn is_food(item_stack: &ItemStack) -> bool {
    REGISTRY
        .items
        .is_in_tag(item_stack.item(), &ItemTag::COW_FOOD)
}

/// `AbstractCow.registerGoals`.
fn register_goals(mob_base: &MobBase) {
    let mut goal_selector = mob_base.goal_selector().lock();
    goal_selector.add_goal(0, FloatGoal::new(mob_base));
    goal_selector.add_goal(1, PanicGoal::new(2.0));
    goal_selector.add_goal(2, BreedGoal::new(1.0));
    goal_selector.add_goal(3, TemptGoal::new(1.25, is_food, false));
    goal_selector.add_goal(4, FollowParentGoal::new(1.25));
    goal_selector.add_goal(5, WaterAvoidingRandomStrollGoal::new(1.0));
    goal_selector.add_goal(6, LookAtPlayerGoal::new(6.0));
    goal_selector.add_goal(7, RandomLookAroundGoal::new());
}

/// `AbstractCow.mobInteract` before `super`: an adult fills a bucket with milk.
fn milk<E: Parts>(cow: &E, player: &Player, hand: InteractionHand) -> Option<InteractionResult> {
    let holds_bucket = player
        .inventory
        .lock()
        .get_item_in_hand(hand)
        .is(&vanilla_items::BUCKET);
    if !holds_bucket || cow.animal().ageable().is_baby() {
        return None;
    }

    player.play_sound(&sound_events::ENTITY_COW_MILK, 1.0, 1.0);
    let has_infinite_materials = player.living().has_infinite_materials();
    let overflow = player.inventory.lock().apply_filled_result(
        hand,
        ITEM_BEHAVIORS.default_instance(&vanilla_items::MILK_BUCKET),
        has_infinite_materials,
        true,
    );
    if !overflow.is_empty() {
        let _ = player.drop_item(overflow, false, Prediction::Predicted);
    }
    Some(InteractionResult::Success)
}
