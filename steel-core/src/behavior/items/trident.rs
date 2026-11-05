//! Trident item behavior.
//!
//! Tridents are thrown on release, or propel the player with a riptide spin
//! attack when the riptide enchantment applies and the player is wet.

use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::sound_events;
use steel_registry::stat::vanilla_stat_types;
use steel_utils::types::InteractionHand;

use crate::behavior::item::ItemBehavior;
use crate::behavior::{InteractionResult, ItemUseAnimation, UseItemContext};
use crate::enchantment_helper;
use crate::entity::entities::ThrownTridentEntity;
use crate::entity::projectile::{AbstractArrow as _, ArrowPickup, spawn_projectile};
use crate::entity::{Entity as _, LivingEntity, LivingEntityRef, SharedEntity, next_entity_id};
use crate::physics::MoverType;
use crate::player::Player;
use crate::world::World;

const THROW_THRESHOLD_TIME: i32 = 10;

const MAX_USE_DURATION: i32 = 72_000;

const PROJECTILE_SHOOT_POWER: f32 = 2.5;

/// Uncertainty applied when the trident is shot from the player's rotation.
const PROJECTILE_UNCERTAINTY: f32 = 1.0;

/// Spawn offset below the owner's eye.
const THROWN_TRIDENT_SPAWN_EYE_OFFSET: f64 = 0.1;

const SPIN_ATTACK_DURATION: i32 = 20;

const SPIN_ATTACK_DAMAGE: f32 = 8.0;

/// Upward lift applied to a grounded player when riptide starts.
const SPIN_ATTACK_GROUND_LIFT: f32 = 1.199_999_9;

/// Trident behavior: throws on release, or starts a riptide spin attack.
#[item_behavior(class = "TridentItem")]
pub struct TridentItem;

impl ItemBehavior for TridentItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let weapon = context.inv.with_item(|item| item.clone());
        if weapon.next_damage_will_break() {
            return InteractionResult::Fail;
        }

        if enchantment_helper::get_trident_spin_attack_strength(&weapon) > 0.0
            && !context.player.is_in_water_or_rain()
        {
            return InteractionResult::Fail;
        }

        context.player.start_using_item(context.hand);
        InteractionResult::Consume
    }

    fn get_use_animation(&self, _stack: &ItemStack) -> ItemUseAnimation {
        ItemUseAnimation::Trident
    }

    fn get_use_duration(&self, _stack: &ItemStack, _user: &dyn LivingEntity) -> i32 {
        MAX_USE_DURATION
    }

    fn release_using(
        &self,
        stack: &mut ItemStack,
        world: &Arc<World>,
        user: LivingEntityRef<'_>,
        time_left: i32,
    ) -> bool {
        let Some(player) = user.entity().as_player() else {
            return false;
        };
        let time_held = self.get_use_duration(stack, user.living()) - time_left;
        if time_held < THROW_THRESHOLD_TIME {
            return false;
        }

        let riptide_strength = enchantment_helper::get_trident_spin_attack_strength(stack);
        let can_riptide = player.is_in_water_or_rain() && !player.is_passenger();
        if riptide_strength > 0.0 && !can_riptide {
            return false;
        }

        if stack.next_damage_will_break() {
            return false;
        }

        let sound = enchantment_helper::get_trident_sound(stack)
            .unwrap_or(&sound_events::ITEM_TRIDENT_THROW);
        player.award_stat(&vanilla_stat_types::ITEM_USED, stack.item());
        stack.hurt_without_breaking(1, player.has_infinite_materials());

        if riptide_strength == 0.0 {
            spawn_thrown_trident(stack, world, player, user.entity(), sound);
        } else {
            let hand = player
                .active_item_use_hand()
                .unwrap_or(InteractionHand::MainHand);
            start_riptide(
                stack,
                world,
                player,
                user.entity(),
                hand,
                riptide_strength,
                sound,
            );
        }

        true
    }
}

/// Consumes one trident and spawns it along the player's look direction.
fn spawn_thrown_trident(
    stack: &mut ItemStack,
    world: &Arc<World>,
    player: &Player,
    owner: &SharedEntity,
    sound: SoundEventRef,
) {
    let mut thrown_item = stack.consume_and_return(1, player.has_infinite_materials());
    let position = player.position();
    let trident = ThrownTridentEntity::from_owner(
        &thrown_item,
        owner,
        DVec3::new(
            position.x,
            player.get_eye_y() - THROWN_TRIDENT_SPAWN_EYE_OFFSET,
            position.z,
        ),
        next_entity_id(),
        Arc::downgrade(world),
    );
    if player.has_infinite_materials() {
        trident.arrow_base().set_pickup(ArrowPickup::CreativeOnly);
    }

    let projectile: SharedEntity = Arc::new(trident);
    let spawned = spawn_projectile(world, projectile, &mut thrown_item, |projectile| {
        let (yaw, pitch) = player.rotation();
        projectile.shoot_from_rotation(
            player,
            pitch,
            yaw,
            0.0,
            PROJECTILE_SHOOT_POWER,
            PROJECTILE_UNCERTAINTY,
        );
    });

    if let Some(spawned) = spawned {
        world.play_sound_at(
            sound,
            SoundSource::Players,
            spawned.position(),
            1.0,
            1.0,
            None,
        );
    }
}

/// Pushes the player along their look direction and starts the spin attack.
fn start_riptide(
    stack: &ItemStack,
    world: &Arc<World>,
    player: &Player,
    player_entity: &SharedEntity,
    hand: InteractionHand,
    riptide_strength: f32,
    sound: SoundEventRef,
) {
    player.push_impulse(riptide_direction(player.rotation(), riptide_strength));
    player.start_auto_spin_attack(SPIN_ATTACK_DURATION, SPIN_ATTACK_DAMAGE, stack, hand);
    if player.on_ground() {
        Arc::clone(player_entity).move_entity(
            MoverType::SelfMovement,
            DVec3::new(0.0, f64::from(SPIN_ATTACK_GROUND_LIFT), 0.0),
        );
    }

    world.play_sound_at(
        sound,
        SoundSource::Players,
        player.position(),
        1.0,
        1.0,
        None,
    );
}

/// The player's look vector scaled to `riptide_strength`.
fn riptide_direction((yaw, pitch): (f32, f32), riptide_strength: f32) -> DVec3 {
    let yaw = f64::from(yaw.to_radians());
    let pitch = f64::from(pitch.to_radians());
    let x = -yaw.sin() * pitch.cos();
    let y = -pitch.sin();
    let z = yaw.cos() * pitch.cos();
    let distance = (x * x + y * y + z * z).sqrt();
    let strength = f64::from(riptide_strength);
    DVec3::new(
        x * strength / distance,
        y * strength / distance,
        z * strength / distance,
    )
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::riptide_direction;

    fn assert_close(actual: DVec3, expected: DVec3) {
        assert!(
            (actual - expected).length() < 1.0e-6,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn riptide_uses_the_players_look_direction() {
        assert_close(
            riptide_direction((0.0, 0.0), 3.0),
            DVec3::new(0.0, 0.0, 3.0),
        );
        assert_close(
            riptide_direction((90.0, 0.0), 3.0),
            DVec3::new(-3.0, 0.0, 0.0),
        );
        assert_close(
            riptide_direction((0.0, -90.0), 3.0),
            DVec3::new(0.0, 3.0, 0.0),
        );
    }
}
