//! Crossbow item behavior.
//!
//! Crossbows load their ammo while the use key is held and fire the loaded
//! projectiles on the next use action.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use glam::DVec3;
use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::ChargedProjectiles;
use steel_registry::data_components::vanilla_components::CHARGED_PROJECTILES;
use steel_registry::enchantment_effect::CrossbowChargingSounds;
use steel_registry::item_stack::ItemStack;
use steel_registry::item_stack_template::ItemStackTemplate;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{sound_events, vanilla_entities, vanilla_items};
use steel_utils::Downcast as _;
use steel_utils::types::InteractionHand;

use crate::behavior::item::{ItemBehavior, ProjectileWeaponItem};
use crate::behavior::{InteractionResult, ItemUseAnimation, UseItemContext};
use crate::enchantment_helper;
use crate::entity::entities::{ArrowEntity, FireworkRocketEntity, SpectralArrowEntity};
use crate::entity::projectile::{AbstractArrow as _, Projectile};
use crate::entity::{Entity as _, LivingEntity, LivingEntityRef, SharedEntity, next_entity_id};
use crate::player::Player;
use crate::world::World;

/// Full charge time in seconds.
const MAX_CHARGE_DURATION: f32 = 1.25;

const DEFAULT_RANGE: i32 = 8;

const MAX_USE_DURATION: i32 = 72_000;

const START_SOUND_PERCENT: f32 = 0.2;

const MID_SOUND_PERCENT: f32 = 0.5;

const ARROW_POWER: f32 = 3.15;

const FIREWORK_POWER: f32 = 1.6;

/// Durability consumed when a firework rocket is loaded.
const FIREWORK_DURABILITY_USE: i32 = 3;

/// Pitch jitter applied to every shot after the first.
const SHOT_PITCH_RANDOM_RANGE: f32 = 0.5;

/// Divisor base of the same jitter.
const SHOT_PITCH_RANDOM_BASE: f32 = 1.8;

/// Added to odd shot indices.
const SHOT_PITCH_ODD_OFFSET: f32 = 0.63;

/// Added to even shot indices.
const SHOT_PITCH_EVEN_OFFSET: f32 = 0.43;

static DEFAULT_CHARGING_SOUNDS: CrossbowChargingSounds = CrossbowChargingSounds {
    start: Some(&sound_events::ITEM_CROSSBOW_LOADING_START),
    mid: Some(&sound_events::ITEM_CROSSBOW_LOADING_MIDDLE),
    end: Some(&sound_events::ITEM_CROSSBOW_LOADING_END),
};

/// Vanilla spawns a crossbow firework at the shooter's eye minus this offset.
const FIREWORK_EYE_OFFSET: f64 = 0.15;

/// Vanilla keeps the loading sound flags on the singleton item instance, so
/// Steel stores them in atomics shared by every crossbow stack.
#[item_behavior(class = "CrossbowItem")]
pub struct CrossbowItem {
    #[json_arg(value)]
    start_sound_played: AtomicBool,
    #[json_arg(value)]
    mid_load_sound_played: AtomicBool,
}

impl CrossbowItem {
    /// Creates a crossbow behavior with the given initial loading sound state.
    #[must_use]
    pub const fn new(start_sound_played: bool, mid_load_sound_played: bool) -> Self {
        Self {
            start_sound_played: AtomicBool::new(start_sound_played),
            mid_load_sound_played: AtomicBool::new(mid_load_sound_played),
        }
    }

    fn get_shooting_power(projectiles: &ChargedProjectiles) -> f32 {
        if projectiles
            .items()
            .iter()
            .any(|item| item.item().key == vanilla_items::FIREWORK_ROCKET.key)
        {
            FIREWORK_POWER
        } else {
            ARROW_POWER
        }
    }

    fn try_load_projectiles(&self, user: LivingEntityRef<'_>, stack: &mut ItemStack) -> bool {
        let Some(player) = user.entity().as_player() else {
            return false;
        };
        let Some((projectile, slot)) = player.get_projectile(stack) else {
            return false;
        };

        let drawn = self.draw(stack, &projectile, slot, player);
        if drawn.is_empty() {
            return false;
        }

        let templates = drawn
            .iter()
            .map(ItemStackTemplate::from_stack)
            .collect::<Result<Vec<_>, _>>();
        let Ok(templates) = templates else {
            log::warn!("crossbow drew projectiles that cannot be persisted");
            return false;
        };
        let Ok(charged) = ChargedProjectiles::new(templates) else {
            log::warn!("crossbow drew more projectiles than the charged-projectile limit");
            return false;
        };

        stack.set(CHARGED_PROJECTILES, charged);
        true
    }

    fn perform_shooting(
        &self,
        world: &Arc<World>,
        player: &Arc<Player>,
        hand: InteractionHand,
        weapon: &mut ItemStack,
        power: f32,
        uncertainty: f32,
    ) {
        let Some(charged) = weapon.get(CHARGED_PROJECTILES).cloned() else {
            return;
        };
        weapon.set(CHARGED_PROJECTILES, ChargedProjectiles::empty());
        if charged.items().is_empty() {
            return;
        }

        let projectiles: Vec<ItemStack> = charged
            .items()
            .iter()
            .map(ItemStackTemplate::create)
            .collect();
        let shooter: SharedEntity = Arc::<Player>::clone(player);
        self.shoot(
            world,
            player,
            &shooter,
            hand,
            weapon,
            &projectiles,
            power,
            uncertainty,
            true,
        );

        // TODO: Trigger SHOT_CROSSBOW once the advancement-criteria foundation
        // exists.
        player.award_stat(&vanilla_stat_types::ITEM_USED, weapon.item());
    }

    fn get_shot_pitch(index: usize) -> f32 {
        if index == 0 {
            return 1.0;
        }

        let range_decider = if index % 2 == 1 {
            SHOT_PITCH_ODD_OFFSET
        } else {
            SHOT_PITCH_EVEN_OFFSET
        };
        1.0 / (rand::random::<f32>() * SHOT_PITCH_RANDOM_RANGE + SHOT_PITCH_RANDOM_BASE)
            + range_decider
    }
}

impl ItemBehavior for CrossbowItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let mut weapon = context.inv.with_item(|item| item.clone());
        if let Some(charged) = weapon.get(CHARGED_PROJECTILES)
            && !charged.items().is_empty()
        {
            let power = Self::get_shooting_power(charged);
            self.perform_shooting(
                context.world,
                context.player,
                context.hand,
                &mut weapon,
                power,
                1.0,
            );
            context.inv.with_item(|item| *item = weapon);
            return InteractionResult::Consume;
        }

        if context.player.get_projectile(&weapon).is_none() {
            return InteractionResult::Fail;
        }

        self.start_sound_played.store(false, Ordering::Relaxed);
        self.mid_load_sound_played.store(false, Ordering::Relaxed);
        context.player.start_using_item(context.hand);
        InteractionResult::Consume
    }

    fn get_use_animation(&self, _stack: &ItemStack) -> ItemUseAnimation {
        ItemUseAnimation::Crossbow
    }

    fn get_use_duration(&self, _stack: &ItemStack, _user: &dyn LivingEntity) -> i32 {
        MAX_USE_DURATION
    }

    fn on_use_tick(
        &self,
        world: &Arc<World>,
        user: LivingEntityRef<'_>,
        stack: &mut ItemStack,
        ticks_remaining: i32,
    ) {
        let charging_sounds = enchantment_helper::get_crossbow_charging_sounds(stack)
            .unwrap_or(&DEFAULT_CHARGING_SOUNDS);
        let charge_duration = get_charge_duration(stack);
        let tick_percent = (self.get_use_duration(stack, user.living()) - ticks_remaining) as f32
            / charge_duration as f32;

        if tick_percent < START_SOUND_PERCENT {
            self.start_sound_played.store(false, Ordering::Relaxed);
            self.mid_load_sound_played.store(false, Ordering::Relaxed);
        }

        if tick_percent >= START_SOUND_PERCENT && !self.start_sound_played.load(Ordering::Relaxed) {
            self.start_sound_played.store(true, Ordering::Relaxed);
            play_charging_sound(
                world,
                user.entity(),
                SoundSource::Players,
                charging_sounds.start,
                0.5,
                1.0,
            );
        }

        if tick_percent >= MID_SOUND_PERCENT && !self.mid_load_sound_played.load(Ordering::Relaxed)
        {
            self.mid_load_sound_played.store(true, Ordering::Relaxed);
            play_charging_sound(
                world,
                user.entity(),
                SoundSource::Players,
                charging_sounds.mid,
                0.5,
                1.0,
            );
        }

        if tick_percent >= 1.0 && !is_charged(stack) && self.try_load_projectiles(user, stack) {
            play_charging_sound(
                world,
                user.entity(),
                user.entity().sound_source(),
                charging_sounds.end,
                1.0,
                1.0 / (rand::random::<f32>() * 0.5 + 1.0) + 0.2,
            );
        }
    }

    fn release_using(
        &self,
        stack: &mut ItemStack,
        _world: &Arc<World>,
        user: LivingEntityRef<'_>,
        time_left: i32,
    ) -> bool {
        let time_held = self.get_use_duration(stack, user.living()) - time_left;
        get_power_for_time(time_held, stack) >= 1.0 && is_charged(stack)
    }

    fn use_on_release(&self, stack: &ItemStack) -> bool {
        stack.is(&vanilla_items::CROSSBOW)
    }

    fn as_projectile_weapon(&self) -> Option<&dyn ProjectileWeaponItem> {
        Some(self)
    }
}

impl ProjectileWeaponItem for CrossbowItem {
    fn is_supported_held_projectile(&self, stack: &ItemStack) -> bool {
        stack.item().has_tag(&ItemTag::ARROWS) || stack.is(&vanilla_items::FIREWORK_ROCKET)
    }

    fn get_default_projectile_range(&self) -> i32 {
        DEFAULT_RANGE
    }

    fn get_durability_use(&self, projectile: &ItemStack) -> i32 {
        if projectile.is(&vanilla_items::FIREWORK_ROCKET) {
            FIREWORK_DURABILITY_USE
        } else {
            1
        }
    }

    fn shoot_projectile(
        &self,
        shooter: &Player,
        projectile: &dyn Projectile,
        index: usize,
        power: f32,
        uncertainty: f32,
        angle: f32,
    ) {
        projectile.shoot(get_shot_direction(shooter, angle), power, uncertainty);
        shooter.get_world().play_sound_at(
            &sound_events::ITEM_CROSSBOW_SHOOT,
            SoundSource::Players,
            shooter.position(),
            1.0,
            Self::get_shot_pitch(index),
            None,
        );
    }

    fn create_projectile(
        &self,
        world: &Arc<World>,
        shooter: LivingEntityRef<'_>,
        weapon: &ItemStack,
        projectile: &mut ItemStack,
        is_crit: bool,
    ) -> Option<SharedEntity> {
        if projectile.is(&vanilla_items::FIREWORK_ROCKET) {
            let shooter_entity = shooter.entity();
            let position = shooter_entity.position();
            let rocket = FireworkRocketEntity::launched(
                &vanilla_entities::FIREWORK_ROCKET,
                next_entity_id(),
                DVec3::new(
                    position.x,
                    shooter_entity.get_eye_y() - FIREWORK_EYE_OFFSET,
                    position.z,
                ),
                Arc::downgrade(world),
                projectile.clone(),
            );
            rocket.set_owner_entity(Some(shooter_entity));
            rocket.set_shot_at_angle(true);
            return Some(Arc::new(rocket));
        }

        let entity = self.create_arrow_projectile(world, shooter, weapon, projectile, is_crit)?;
        if let Some(arrow) = entity.downcast_ref::<ArrowEntity>() {
            arrow
                .arrow_base()
                .set_sound_event(&sound_events::ITEM_CROSSBOW_HIT);
        } else if let Some(arrow) = entity.downcast_ref::<SpectralArrowEntity>() {
            arrow
                .arrow_base()
                .set_sound_event(&sound_events::ITEM_CROSSBOW_HIT);
        }
        Some(entity)
    }
}

fn get_charge_duration(crossbow: &ItemStack) -> i32 {
    let duration = enchantment_helper::modify_crossbow_charging_time(crossbow, MAX_CHARGE_DURATION);
    (duration * 20.0).floor() as i32
}

fn get_power_for_time(time_held: i32, crossbow: &ItemStack) -> f32 {
    let power = time_held as f32 / get_charge_duration(crossbow) as f32;
    power.min(1.0)
}

fn is_charged(stack: &ItemStack) -> bool {
    stack
        .get(CHARGED_PROJECTILES)
        .is_some_and(|projectiles| !projectiles.items().is_empty())
}

/// Rotates the view vector around the shooter's up vector by `angle` degrees.
fn get_shot_direction(shooter: &Player, angle: f32) -> DVec3 {
    let (yaw, pitch) = shooter.rotation();
    let view = shooter.look_angle();
    let up = shooter.calculate_view_vector(pitch - 90.0, yaw);
    let radians = f64::from(angle.to_radians());
    let (sin, cos) = radians.sin_cos();

    view * cos + up.cross(view) * sin + up * up.dot(view) * (1.0 - cos)
}

fn play_charging_sound(
    world: &Arc<World>,
    entity: &SharedEntity,
    source: SoundSource,
    sound: Option<SoundEventRef>,
    volume: f32,
    pitch: f32,
) {
    if let Some(sound) = sound {
        world.play_sound_at(sound, source, entity.position(), volume, pitch, None);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{
        init_vanilla_registry, sound_events, vanilla_enchantments, vanilla_items,
    };

    use super::{get_charge_duration, get_power_for_time};
    use crate::enchantment_helper;

    #[test]
    fn quick_charge_scales_charge_duration() {
        init_vanilla_registry();

        let mut crossbow = ItemStack::new(&vanilla_items::CROSSBOW);
        assert_eq!(get_charge_duration(&crossbow), 25);

        for (level, expected_ticks) in [(1, 20), (2, 15), (3, 10)] {
            crossbow.set_enchantments(
                &[(vanilla_enchantments::QUICK_CHARGE.key.clone(), level)],
                false,
            );
            assert_eq!(get_charge_duration(&crossbow), expected_ticks);
        }
    }

    #[test]
    fn quick_charge_selects_the_matching_charging_sounds() {
        init_vanilla_registry();

        let mut crossbow = ItemStack::new(&vanilla_items::CROSSBOW);
        assert!(enchantment_helper::get_crossbow_charging_sounds(&crossbow).is_none());

        crossbow.set_enchantments(
            &[(vanilla_enchantments::QUICK_CHARGE.key.clone(), 2)],
            false,
        );
        let sounds = enchantment_helper::get_crossbow_charging_sounds(&crossbow)
            .expect("quick charge defines charging sounds");
        assert!(
            sounds
                .start
                .is_some_and(|sound| sound.key == sound_events::ITEM_CROSSBOW_QUICK_CHARGE_2.key)
        );
        assert!(sounds.mid.is_none());
        assert!(
            sounds
                .end
                .is_some_and(|sound| sound.key == sound_events::ITEM_CROSSBOW_LOADING_END.key)
        );
    }

    #[test]
    fn power_for_time_clamps_at_full_charge() {
        init_vanilla_registry();

        let crossbow = ItemStack::new(&vanilla_items::CROSSBOW);
        assert!(get_power_for_time(24, &crossbow) < 1.0);
        assert_eq!(get_power_for_time(25, &crossbow), 1.0);
        assert_eq!(get_power_for_time(100, &crossbow), 1.0);
    }
}
