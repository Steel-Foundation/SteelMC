use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_events;
use steel_registry::stat::vanilla_stat_types;
use steel_utils::types::InteractionHand;

use crate::behavior::item::{ItemBehavior, ProjectileWeaponItem};
use crate::behavior::{InteractionResult, ItemUseAnimation, UseItemContext};
use crate::entity::projectile::Projectile;
use crate::entity::{Entity as _, LivingEntity, LivingEntityRef};
use crate::player::Player;
use crate::world::World;

/// Ticks needed for a full-power shot.
const MAX_DRAW_DURATION: i32 = 20;

/// Maximum projectile range.
const DEFAULT_RANGE: i32 = 15;

const MAX_USE_DURATION: i32 = 72_000;

/// Shots below this draw power are rejected.
const MIN_POWER: f32 = 0.1;

/// Draw power is clamped to this value.
const MAX_POWER: f32 = 1.0;

/// Draw power is multiplied by this when launching an arrow.
const ARROW_POWER_SCALE: f32 = 3.0;

const ARROW_UNCERTAINTY: f32 = 1.0;

const SOUND_PITCH_RANDOM_RANGE: f32 = 0.4;
const SOUND_PITCH_RANDOM_BASE: f32 = 1.2;
const SOUND_PITCH_POWER_SCALE: f32 = 0.5;

/// Draws ammo from the shooter's inventory and fires it.
#[item_behavior]
pub struct BowItem;

impl ItemBehavior for BowItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let stack = context.inv.with_item(|item| item.clone());
        let found_projectile = context.player.get_projectile(&stack).is_some();
        if !context.player.has_infinite_materials() && !found_projectile {
            return InteractionResult::Fail;
        }

        context.player.start_using_item(context.hand);
        InteractionResult::Consume
    }

    fn get_use_animation(&self, _stack: &ItemStack) -> ItemUseAnimation {
        ItemUseAnimation::Bow
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
        let Some((projectile, slot)) = player.get_projectile(stack) else {
            return false;
        };

        let time_held = self.get_use_duration(stack, user.living()) - time_left;
        let power = get_power_for_time(time_held);
        if power < MIN_POWER {
            return false;
        }

        let weapon_item = stack.item();
        let projectiles = self.draw(stack, &projectile, slot, player);
        if !projectiles.is_empty() {
            let hand = player
                .active_item_use_hand()
                .unwrap_or(InteractionHand::MainHand);
            self.shoot(
                world,
                player,
                user.entity(),
                hand,
                stack,
                &projectiles,
                power * ARROW_POWER_SCALE,
                ARROW_UNCERTAINTY,
                power >= MAX_POWER,
            );
        }

        let pitch = 1.0
            / (rand::random::<f32>() * SOUND_PITCH_RANDOM_RANGE + SOUND_PITCH_RANDOM_BASE)
            + power * SOUND_PITCH_POWER_SCALE;
        world.play_sound_at(
            &sound_events::ENTITY_ARROW_SHOOT,
            SoundSource::Players,
            player.position(),
            1.0,
            pitch,
            None,
        );
        player.award_stat(&vanilla_stat_types::ITEM_USED, weapon_item);
        true
    }

    fn as_projectile_weapon(&self) -> Option<&dyn ProjectileWeaponItem> {
        Some(self)
    }
}

impl ProjectileWeaponItem for BowItem {
    fn get_default_projectile_range(&self) -> i32 {
        DEFAULT_RANGE
    }

    fn shoot_projectile(
        &self,
        shooter: &Player,
        projectile: &dyn Projectile,
        _index: usize,
        power: f32,
        uncertainty: f32,
        angle: f32,
    ) {
        let (yaw, pitch) = shooter.rotation();
        projectile.shoot_from_rotation(shooter, pitch, yaw + angle, 0.0, power, uncertainty);
    }
}

#[must_use]
pub fn get_power_for_time(time_held: i32) -> f32 {
    let mut power = time_held as f32 / MAX_DRAW_DURATION as f32;
    power = (power * power + power * 2.0) / 3.0;
    power.min(MAX_POWER)
}
