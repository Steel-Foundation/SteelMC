use super::{
    EnchantmentEffectsJson, ToShoutySnakeCase, TokenStream, generate_attribute_effects,
    generate_conditional_entity_effects, generate_conditional_value_effects,
    generate_crossbow_charging_sounds, generate_damage_immunity_effects,
    generate_optional_value_effect, generate_sound_event_refs, generate_targeted_entity_effects,
    generate_targeted_value_effects, quote,
};

pub(super) fn generate_enchantment_effects(
    name: &str,
    effects: &EnchantmentEffectsJson,
    statics: &mut TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let prefix = name.to_shouty_snake_case();
    let damage_protection = generate_conditional_value_effects(
        &format!("{prefix}_DAMAGE_PROTECTION"),
        &effects.damage_protection,
        statics,
        counter,
    );
    let damage_immunity = generate_damage_immunity_effects(
        &format!("{prefix}_DAMAGE_IMMUNITY"),
        &effects.damage_immunity,
        statics,
        counter,
    );
    let damage = generate_conditional_value_effects(
        &format!("{prefix}_DAMAGE"),
        &effects.damage,
        statics,
        counter,
    );
    let smash_damage_per_fallen_block = generate_conditional_value_effects(
        &format!("{prefix}_SMASH_DAMAGE_PER_FALLEN_BLOCK"),
        &effects.smash_damage_per_fallen_block,
        statics,
        counter,
    );
    let knockback = generate_conditional_value_effects(
        &format!("{prefix}_KNOCKBACK"),
        &effects.knockback,
        statics,
        counter,
    );
    let armor_effectiveness = generate_conditional_value_effects(
        &format!("{prefix}_ARMOR_EFFECTIVENESS"),
        &effects.armor_effectiveness,
        statics,
        counter,
    );
    let post_attack = generate_targeted_entity_effects(
        &format!("{prefix}_POST_ATTACK"),
        &effects.post_attack,
        statics,
        counter,
    );
    let post_piercing_attack = generate_conditional_entity_effects(
        &format!("{prefix}_POST_PIERCING_ATTACK"),
        &effects.post_piercing_attack,
        statics,
        counter,
    );
    let item_damage = generate_conditional_value_effects(
        &format!("{prefix}_ITEM_DAMAGE"),
        &effects.item_damage,
        statics,
        counter,
    );
    let equipment_drops = generate_targeted_value_effects(
        &format!("{prefix}_EQUIPMENT_DROPS"),
        &effects.equipment_drops,
        statics,
        counter,
    );
    let ammo_use = generate_conditional_value_effects(
        &format!("{prefix}_AMMO_USE"),
        &effects.ammo_use,
        statics,
        counter,
    );
    let projectile_piercing = generate_conditional_value_effects(
        &format!("{prefix}_PROJECTILE_PIERCING"),
        &effects.projectile_piercing,
        statics,
        counter,
    );
    let projectile_spawned = generate_conditional_entity_effects(
        &format!("{prefix}_PROJECTILE_SPAWNED"),
        &effects.projectile_spawned,
        statics,
        counter,
    );
    let projectile_spread = generate_conditional_value_effects(
        &format!("{prefix}_PROJECTILE_SPREAD"),
        &effects.projectile_spread,
        statics,
        counter,
    );
    let projectile_count = generate_conditional_value_effects(
        &format!("{prefix}_PROJECTILE_COUNT"),
        &effects.projectile_count,
        statics,
        counter,
    );
    let trident_return_acceleration = generate_conditional_value_effects(
        &format!("{prefix}_TRIDENT_RETURN_ACCELERATION"),
        &effects.trident_return_acceleration,
        statics,
        counter,
    );
    let fishing_time_reduction = generate_conditional_value_effects(
        &format!("{prefix}_FISHING_TIME_REDUCTION"),
        &effects.fishing_time_reduction,
        statics,
        counter,
    );
    let fishing_luck_bonus = generate_conditional_value_effects(
        &format!("{prefix}_FISHING_LUCK_BONUS"),
        &effects.fishing_luck_bonus,
        statics,
        counter,
    );
    let block_experience = generate_conditional_value_effects(
        &format!("{prefix}_BLOCK_EXPERIENCE"),
        &effects.block_experience,
        statics,
        counter,
    );
    let mob_experience = generate_conditional_value_effects(
        &format!("{prefix}_MOB_EXPERIENCE"),
        &effects.mob_experience,
        statics,
        counter,
    );
    let repair_with_xp = generate_conditional_value_effects(
        &format!("{prefix}_REPAIR_WITH_XP"),
        &effects.repair_with_xp,
        statics,
        counter,
    );
    let attributes = generate_attribute_effects(
        &format!("{prefix}_ATTRIBUTES"),
        &effects.attributes,
        statics,
        counter,
    );
    let crossbow_charge_time = generate_optional_value_effect(
        &format!("{prefix}_CROSSBOW_CHARGE_TIME"),
        &effects.crossbow_charge_time,
        statics,
        counter,
    );
    let crossbow_charging_sounds =
        generate_crossbow_charging_sounds(&effects.crossbow_charging_sounds);
    let trident_sound = generate_sound_event_refs(&effects.trident_sound);
    let trident_spin_attack_strength = generate_optional_value_effect(
        &format!("{prefix}_TRIDENT_SPIN_ATTACK_STRENGTH"),
        &effects.trident_spin_attack_strength,
        statics,
        counter,
    );

    let hit_block = !effects.hit_block.is_empty();
    let location_changed = !effects.location_changed.is_empty();
    let tick = !effects.tick.is_empty();
    let prevent_equipment_drop = effects.prevent_equipment_drop.is_some();
    let prevent_armor_change = effects.prevent_armor_change.is_some();

    quote! {
        EnchantmentEffects {
            damage_protection: #damage_protection,
            damage_immunity: #damage_immunity,
            damage: #damage,
            smash_damage_per_fallen_block: #smash_damage_per_fallen_block,
            knockback: #knockback,
            armor_effectiveness: #armor_effectiveness,
            post_attack: #post_attack,
            post_piercing_attack: #post_piercing_attack,
            hit_block: #hit_block,
            item_damage: #item_damage,
            equipment_drops: #equipment_drops,
            location_changed: #location_changed,
            tick: #tick,
            ammo_use: #ammo_use,
            projectile_piercing: #projectile_piercing,
            projectile_spawned: #projectile_spawned,
            projectile_spread: #projectile_spread,
            projectile_count: #projectile_count,
            trident_return_acceleration: #trident_return_acceleration,
            fishing_time_reduction: #fishing_time_reduction,
            fishing_luck_bonus: #fishing_luck_bonus,
            block_experience: #block_experience,
            mob_experience: #mob_experience,
            repair_with_xp: #repair_with_xp,
            attributes: #attributes,
            crossbow_charge_time: #crossbow_charge_time,
            crossbow_charging_sounds: #crossbow_charging_sounds,
            trident_sound: #trident_sound,
            prevent_equipment_drop: #prevent_equipment_drop,
            prevent_armor_change: #prevent_armor_change,
            trident_spin_attack_strength: #trident_spin_attack_strength,
        }
    }
}
