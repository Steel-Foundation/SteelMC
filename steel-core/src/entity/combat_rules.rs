use crate::enchantment_helper::{self, EnchantmentDamageContext};
use crate::entity::LivingEntity;

use super::damage::DamageSource;

const MAX_ARMOR: f32 = 20.0;
const ARMOR_PROTECTION_DIVIDER: f32 = 25.0;
const BASE_ARMOR_TOUGHNESS: f32 = 2.0;
const MIN_ARMOR_RATIO: f32 = 0.2;

/// Returns vanilla `CombatRules.getDamageAfterAbsorb`.
pub(super) fn get_damage_after_absorb(
    victim: &(impl LivingEntity + ?Sized),
    damage: f32,
    source: &DamageSource,
    total_armor: f32,
    armor_toughness: f32,
) -> f32 {
    let toughness = BASE_ARMOR_TOUGHNESS + armor_toughness / 4.0;
    let real_armor =
        (total_armor - damage / toughness).clamp(total_armor * MIN_ARMOR_RATIO, MAX_ARMOR);
    let armor_fraction = real_armor / ARMOR_PROTECTION_DIVIDER;
    let mut modified_armor_fraction = armor_fraction;

    if let Some(world) = victim.level()
        && let Some(direct_entity) = source
            .direct_entity_id
            .and_then(|entity_id| world.get_entity_by_id(entity_id))
    {
        let context =
            EnchantmentDamageContext::from_damage_source(&world, victim.entity_type(), source);
        direct_entity.with_weapon_item(&mut |weapon| {
            if let Some(weapon) = weapon {
                modified_armor_fraction = enchantment_helper::modify_armor_effectiveness(
                    weapon,
                    &context,
                    armor_fraction,
                )
                .clamp(0.0, 1.0);
            }
        });
    }

    damage * (1.0 - modified_armor_fraction)
}

/// Returns vanilla `CombatRules.getDamageAfterMagicAbsorb`.
pub(super) fn get_damage_after_magic_absorb(damage: f32, total_magic_armor: f32) -> f32 {
    let real_armor = total_magic_armor.clamp(0.0, MAX_ARMOR);
    damage * (1.0 - real_armor / ARMOR_PROTECTION_DIVIDER)
}

#[cfg(test)]
mod tests {
    use super::get_damage_after_magic_absorb;

    #[test]
    fn magic_absorb_clamps_protection_to_vanilla_range() {
        assert_eq!(
            get_damage_after_magic_absorb(10.0, -1.0).to_bits(),
            10.0_f32.to_bits()
        );
        assert_eq!(
            get_damage_after_magic_absorb(10.0, 5.0).to_bits(),
            8.0_f32.to_bits()
        );
        assert_eq!(
            get_damage_after_magic_absorb(10.0, 25.0).to_bits(),
            (10.0_f32 * (1.0 - 20.0_f32 / 25.0)).to_bits()
        );
    }
}
