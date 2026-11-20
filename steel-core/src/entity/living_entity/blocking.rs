use std::f32::consts::PI;

use steel_registry::data_components::vanilla_components::BLOCKS_ATTACKS;

use super::{
    DVec3, DamageSource, InteractionHand, ItemStack, Living, Parts, VEC3_NORMALIZE_EPSILON, World,
    vanilla_damage_type_tags,
};
use crate::behavior::ITEM_BEHAVIORS;
use crate::entity::blocks_attacks;
use crate::entity::prelude::*;

const PARTIAL_BLOCK_KNOCKBACK: f64 = 0.5;

impl Living<'_> {
    /// Returns the blocking item once its use delay has elapsed.
    #[must_use]
    pub fn item_blocking_with(&self) -> Option<(InteractionHand, ItemStack)> {
        if !self.is_using_item() {
            return None;
        }
        let active = self.living_base().active_item_use()?;
        let mut blocking_item = None;
        self.with_equipment_slot(active.hand().into(), &mut |item| {
            if item.item() == active.item()
                && let Some(blocks_attacks) = item.get(BLOCKS_ATTACKS)
            {
                blocking_item = Some((item.clone(), blocks_attacks.block_delay_ticks()));
            }
        });
        let (item, block_delay_ticks) = blocking_item?;
        let duration = ITEM_BEHAVIORS
            .get_behavior(item.item())
            .get_use_duration(&item, self);
        (duration - active.remaining_ticks() >= block_delay_ticks).then_some((active.hand(), item))
    }

    /// Reduces incoming damage with the active blocking item.
    pub(super) fn apply_item_blocking(
        &self,
        world: &World,
        source: &DamageSource,
        damage: f32,
    ) -> f32 {
        if damage <= 0.0 {
            return 0.0;
        }
        let Some((hand, blocking_with)) = self.item_blocking_with() else {
            return 0.0;
        };
        let Some(blocks_attacks) = blocking_with.get(BLOCKS_ATTACKS) else {
            return 0.0;
        };
        if blocks_attacks.is_bypassed_by(source.damage_type) {
            return 0.0;
        }
        // TODO: Piercing arrows bypass blocking once AbstractArrow exists.

        let angle = source
            .source_position()
            .map_or(f64::from(PI), |source_position| {
                let view = self.calculate_view_vector(0.0, self.y_head_rot());
                let delta = source_position - self.position();
                let horizontal = DVec3::new(delta.x, 0.0, delta.z);
                let length = horizontal.length();
                let to_source = if length < VEC3_NORMALIZE_EPSILON {
                    DVec3::ZERO
                } else {
                    horizontal / length
                };
                to_source.dot(view).acos()
            });
        let damage_blocked =
            blocks_attacks.resolve_blocked_damage(source.damage_type, damage, angle);
        blocks_attacks::hurt_blocking_item(
            blocks_attacks,
            &blocking_with,
            self,
            hand,
            damage_blocked,
        );
        if damage_blocked > 0.0
            && !source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_PROJECTILE)
            && let Some(attacker) = source
                .direct_entity()
                .and_then(|entity| entity.as_living_entity())
        {
            self.block_using_item(world, &attacker, source, damage, damage_blocked >= damage);
        }
        damage_blocked
    }

    /// Returns the block-disable duration of the weapon when it is the active item.
    #[must_use]
    pub fn seconds_to_disable_blocking(&self) -> f32 {
        if self.is_spectator() {
            return 0.0;
        }
        let active = self
            .living_base()
            .active_item_use()
            .filter(|_| self.is_using_item());
        if let Some(active) = active
            && (active.hand() != InteractionHand::MainHand
                || self.as_player().is_some_and(|player| {
                    !player
                        .inventory
                        .lock()
                        .holds_use_item(InteractionHand::MainHand)
                }))
        {
            return 0.0;
        }
        let mut seconds = 0.0;
        self.with_weapon_item(&mut |item| {
            seconds = item
                .filter(|item| active.is_none_or(|active| active.item() == item.item()))
                .and_then(ItemStack::get_weapon)
                .map_or(0.0, |weapon| weapon.disable_blocking_for_seconds);
        });
        seconds
    }
}

/// Notifies the attacker that the defender blocked the hit.
pub fn block_using_item<E: Parts>(
    entity: &E,
    _world: &World,
    attacker: &Living<'_>,
    source: &DamageSource,
    damage: f32,
    fully_blocked: bool,
) {
    attacker.blocked_by_item(&entity.living(), source, damage, fully_blocked);
}

/// Applies the extra knockback a defender receives from a partial block.
pub fn blocked_by_item<E: Parts>(
    entity: &E,
    defender: &Living<'_>,
    _source: &DamageSource,
    _damage: f32,
    fully_blocked: bool,
) {
    if !fully_blocked {
        let delta = defender.position() - entity.living().position();
        defender.knockback(PARTIAL_BLOCK_KNOCKBACK, delta.x, delta.z);
    }
}
