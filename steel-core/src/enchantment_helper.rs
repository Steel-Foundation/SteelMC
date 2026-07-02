use steel_registry::enchantment_effect::{
    DamageSourcePredicate, EnchantmentEffectComponent, EnchantmentEffectRequirements,
    EnchantmentEntityEffect, EnchantmentEntityTarget, EnchantmentTarget, EntityPredicate,
    EntityTypePredicate, EntityTypeSpecificPredicate, EntityVehiclePredicate, MobEffectSelection,
};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::{REGISTRY, RegistryExt, TaggedRegistryExt, vanilla_entities};

use crate::entity::damage::DamageSource;
use crate::entity::{Entity, LivingEntity, MobEffectInstance};
use crate::inventory::equipment::EquipmentSlot;

#[derive(Debug, Clone, Copy)]
pub(crate) struct EnchantmentDamageContext<'a> {
    this_entity_type: EntityTypeRef,
    attacker_entity_type: Option<EntityTypeRef>,
    direct_attacker_entity_type: Option<EntityTypeRef>,
    damage_source: &'a DamageSource,
}

impl<'a> EnchantmentDamageContext<'a> {
    #[must_use]
    pub(crate) const fn new(
        this_entity_type: EntityTypeRef,
        attacker_entity_type: Option<EntityTypeRef>,
        direct_attacker_entity_type: Option<EntityTypeRef>,
        damage_source: &'a DamageSource,
    ) -> Self {
        Self {
            this_entity_type,
            attacker_entity_type,
            direct_attacker_entity_type,
            damage_source,
        }
    }

    const fn entity_type(self, target: EnchantmentEntityTarget) -> Option<EntityTypeRef> {
        match target {
            EnchantmentEntityTarget::This => Some(self.this_entity_type),
            EnchantmentEntityTarget::Attacker => self.attacker_entity_type,
            EnchantmentEntityTarget::DirectAttacker => self.direct_attacker_entity_type,
        }
    }
}

pub(crate) struct EnchantmentPostAttackContext<'a> {
    victim: &'a dyn Entity,
    attacker: Option<&'a dyn Entity>,
    direct_attacker: Option<&'a dyn Entity>,
    damage_source: &'a DamageSource,
}

impl<'a> EnchantmentPostAttackContext<'a> {
    #[must_use]
    pub(crate) const fn new(
        victim: &'a dyn Entity,
        attacker: Option<&'a dyn Entity>,
        direct_attacker: Option<&'a dyn Entity>,
        damage_source: &'a DamageSource,
    ) -> Self {
        Self {
            victim,
            attacker,
            direct_attacker,
            damage_source,
        }
    }

    fn damage_context(&self) -> EnchantmentDamageContext<'a> {
        EnchantmentDamageContext::new(
            self.victim.entity_type(),
            self.attacker.map(Entity::entity_type),
            self.direct_attacker.map(Entity::entity_type),
            self.damage_source,
        )
    }

    fn affected_entity(&self, target: EnchantmentTarget) -> Option<&'a dyn Entity> {
        match target {
            EnchantmentTarget::Attacker => self.attacker,
            EnchantmentTarget::DamagingEntity => self.direct_attacker,
            EnchantmentTarget::Victim => Some(self.victim),
        }
    }
}

pub(crate) fn modify_damage(
    item: &ItemStack,
    context: &EnchantmentDamageContext<'_>,
    damage: f32,
) -> f32 {
    apply_value_effects(item, EnchantmentEffectComponent::Damage, context, damage)
}

pub(crate) fn modify_knockback(
    item: &ItemStack,
    context: &EnchantmentDamageContext<'_>,
    knockback: f32,
) -> f32 {
    apply_value_effects(
        item,
        EnchantmentEffectComponent::Knockback,
        context,
        knockback,
    )
}

pub(crate) fn modify_smash_damage_per_fallen_block(
    item: &ItemStack,
    context: &EnchantmentDamageContext<'_>,
    damage: f32,
) -> f32 {
    apply_value_effects(
        item,
        EnchantmentEffectComponent::SmashDamagePerFallenBlock,
        context,
        damage,
    )
}

pub(crate) fn is_immune_to_damage<V: LivingEntity + ?Sized>(
    victim: &V,
    damage_source: &DamageSource,
) -> bool {
    let world = victim.level();
    let attacker_entity_type = damage_source
        .causing_entity_id
        .and_then(|entity_id| world.as_ref()?.get_entity_by_id(entity_id))
        .map(|entity| entity.entity_type());
    let direct_attacker_entity_type = damage_source
        .direct_entity_id
        .and_then(|entity_id| world.as_ref()?.get_entity_by_id(entity_id))
        .map(|entity| entity.entity_type());
    let context = EnchantmentDamageContext::new(
        victim.entity_type(),
        attacker_entity_type,
        direct_attacker_entity_type,
        damage_source,
    );

    for slot in EquipmentSlot::ALL {
        let mut slot_matches = false;
        victim.with_equipment_slot(slot, &mut |item| {
            slot_matches = item_damage_immunity_matches(item, slot, &context);
        });
        if slot_matches {
            return true;
        }
    }

    false
}

pub(crate) fn do_post_attack_effects_from_item(
    item: &ItemStack,
    context: &EnchantmentPostAttackContext<'_>,
) {
    let mut item = item.copy_with_count(item.count());
    let _ = apply_post_attack_effects(
        &mut item,
        Some(EquipmentSlot::MainHand),
        EnchantmentTarget::Attacker,
        context,
    );
}

pub(crate) fn do_post_attack_effects_with_item_source(
    victim: &dyn Entity,
    source: &ItemStack,
    context: &EnchantmentPostAttackContext<'_>,
) {
    if let Some(living_victim) = victim.as_living_entity() {
        for slot in EquipmentSlot::ALL {
            let mut item_broke = false;
            living_victim.with_equipment_slot_mut(slot, &mut |item| {
                item_broke =
                    apply_post_attack_effects(item, Some(slot), EnchantmentTarget::Victim, context);
            });
            if item_broke {
                living_victim.on_equipped_item_broken(slot);
            }
        }
    }

    let mut source = source.copy_with_count(source.count());
    let _ = apply_post_attack_effects(
        &mut source,
        Some(EquipmentSlot::MainHand),
        EnchantmentTarget::Attacker,
        context,
    );
}

fn apply_value_effects(
    item: &ItemStack,
    component: EnchantmentEffectComponent,
    context: &EnchantmentDamageContext<'_>,
    input: f32,
) -> f32 {
    let Some(enchantments) = item.get_enchantments() else {
        return input;
    };

    let mut value = input;
    for (key, level) in enchantments.iter() {
        if *level == 0 {
            continue;
        }
        let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
            continue;
        };
        let level = *level as i32;

        for effect in enchantment.effects.value_effects(component) {
            if !requirements_match(effect.requirements, context) {
                continue;
            }
            if let Some(updated) = effect.effect.process_without_random(level, value) {
                value = updated;
            }
        }

        let Some(effect) = enchantment.effects.single_value_effect(component) else {
            continue;
        };
        if let Some(updated) = effect.process_without_random(level, value) {
            value = updated;
        }
    }

    value
}

fn item_damage_immunity_matches(
    item: &ItemStack,
    slot: EquipmentSlot,
    context: &EnchantmentDamageContext<'_>,
) -> bool {
    let Some(enchantments) = item.get_enchantments() else {
        return false;
    };

    for (key, level) in enchantments.iter() {
        if *level == 0 {
            continue;
        }
        let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
            continue;
        };
        if !enchantment.matching_slot(slot) {
            continue;
        }
        if enchantment
            .effects
            .damage_immunity
            .iter()
            .any(|effect| requirements_match(effect.requirements, context))
        {
            return true;
        }
    }

    false
}

fn apply_post_attack_effects(
    item: &mut ItemStack,
    slot: Option<EquipmentSlot>,
    enchanted_target: EnchantmentTarget,
    context: &EnchantmentPostAttackContext<'_>,
) -> bool {
    let Some(enchantments) = item.get_enchantments().cloned() else {
        return false;
    };
    let damage_context = context.damage_context();
    let mut enchanted_item_broke = false;

    for (key, level) in enchantments.iter() {
        if *level == 0 {
            continue;
        }
        let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
            continue;
        };
        if slot.is_some_and(|slot| !enchantment.matching_slot(slot)) {
            continue;
        }
        let level = *level as i32;

        for effect in enchantment.effects.post_attack {
            if effect.enchanted != enchanted_target {
                continue;
            }
            let Some(enchanted_entity) = context.affected_entity(enchanted_target) else {
                continue;
            };
            let requirements_match =
                post_attack_requirements_match(effect.requirements, &damage_context, level);
            if !requirements_match {
                continue;
            }
            let Some(affected_entity) = context.affected_entity(effect.affected) else {
                continue;
            };
            enchanted_item_broke |= apply_entity_effect(
                &effect.effect,
                level,
                affected_entity,
                enchanted_entity,
                item,
            );
        }
    }

    enchanted_item_broke
}

pub(crate) fn do_post_piercing_attack_effects(user: &dyn LivingEntity) {
    let mut item_stack = ItemStack::empty();
    user.with_equipment_slot(EquipmentSlot::MainHand, &mut |stack| {
        item_stack = stack.copy_with_count(stack.count());
    });
    let enchantments = item_stack.get_enchantments().cloned();
    let Some(enchantments) = enchantments else {
        return;
    };

    for (key, level) in enchantments.iter() {
        if *level == 0 {
            continue;
        }
        let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
            continue;
        };
        if !enchantment.matching_slot(EquipmentSlot::MainHand) {
            continue;
        }

        let level = *level as i32;
        for effect in enchantment.effects.post_piercing_attack {
            if !entity_requirements_match(effect.requirements, user) {
                continue;
            }
            if apply_post_piercing_entity_effect(&effect.effect, level, user) {
                user.on_equipped_item_broken(EquipmentSlot::MainHand);
            }
        }
    }
}

fn apply_entity_effect(
    effect: &EnchantmentEntityEffect,
    level: i32,
    entity: &dyn Entity,
    enchanted_entity: &dyn Entity,
    enchanted_item: &mut ItemStack,
) -> bool {
    if !entity_effect_is_supported(effect) {
        return false;
    }

    apply_supported_entity_effect(effect, level, entity, enchanted_entity, enchanted_item)
}

fn entity_effect_is_supported(effect: &EnchantmentEntityEffect) -> bool {
    match effect {
        EnchantmentEntityEffect::AllOf(effects) => effects
            .iter()
            .all(|effect| entity_effect_is_supported(effect)),
        EnchantmentEntityEffect::ApplyExhaustion { .. }
        | EnchantmentEntityEffect::ApplyImpulse { .. }
        | EnchantmentEntityEffect::PlaySound { .. }
        | EnchantmentEntityEffect::Unsupported { .. } => false,
        EnchantmentEntityEffect::ChangeItemDamage { .. }
        | EnchantmentEntityEffect::DamageEntity { .. }
        | EnchantmentEntityEffect::Ignite { .. } => true,
        EnchantmentEntityEffect::ApplyMobEffect { to_apply, .. } => {
            matches!(to_apply, MobEffectSelection::Single(_))
        }
    }
}

fn apply_supported_entity_effect(
    effect: &EnchantmentEntityEffect,
    level: i32,
    entity: &dyn Entity,
    enchanted_entity: &dyn Entity,
    enchanted_item: &mut ItemStack,
) -> bool {
    match effect {
        EnchantmentEntityEffect::AllOf(effects) => {
            let mut enchanted_item_broke = false;
            for effect in *effects {
                enchanted_item_broke |= apply_supported_entity_effect(
                    effect,
                    level,
                    entity,
                    enchanted_entity,
                    enchanted_item,
                );
            }
            enchanted_item_broke
        }
        EnchantmentEntityEffect::ChangeItemDamage { amount } => {
            let amount = amount.calculate(level) as i32;
            let has_infinite_materials = enchanted_entity
                .as_living_entity()
                .is_some_and(LivingEntity::has_infinite_materials);
            enchanted_item.hurt_and_break(amount, has_infinite_materials)
        }
        EnchantmentEntityEffect::DamageEntity {
            min_damage,
            max_damage,
            damage_type,
        } => {
            let min_damage = min_damage.calculate(level);
            let max_damage = max_damage.calculate(level);
            let damage = random_between(min_damage, max_damage);
            let source = DamageSource::environment(damage_type)
                .with_causing_entity(enchanted_entity.id())
                .with_direct_entity(enchanted_entity.id());
            entity.hurt(&source, damage);
            false
        }
        EnchantmentEntityEffect::Ignite { duration } => {
            let ticks = (duration.calculate(level) * 20.0).floor() as i32;
            entity.ignite_for_ticks(ticks);
            false
        }
        EnchantmentEntityEffect::ApplyMobEffect {
            to_apply: MobEffectSelection::Single(effect),
            min_duration,
            max_duration,
            min_amplifier,
            max_amplifier,
        } => {
            let Some(living) = entity.as_living_entity() else {
                return false;
            };
            let min_duration = min_duration.calculate(level);
            let max_duration = max_duration.calculate(level);
            let min_amplifier = min_amplifier.calculate(level);
            let max_amplifier = max_amplifier.calculate(level);
            let duration_seconds = random_between(min_duration, max_duration);
            let amplifier = random_between(min_amplifier, max_amplifier);
            let duration_ticks = java_round(duration_seconds * 20.0);
            let amplifier = java_round(amplifier).max(0);
            living.add_mob_effect(MobEffectInstance::with_duration(
                effect,
                duration_ticks,
                amplifier,
            ));
            false
        }
        EnchantmentEntityEffect::ApplyMobEffect { .. }
        | EnchantmentEntityEffect::ApplyExhaustion { .. }
        | EnchantmentEntityEffect::ApplyImpulse { .. }
        | EnchantmentEntityEffect::PlaySound { .. }
        | EnchantmentEntityEffect::Unsupported { .. } => false,
    }
}

fn apply_post_piercing_entity_effect(
    effect: &EnchantmentEntityEffect,
    level: i32,
    user: &dyn LivingEntity,
) -> bool {
    if !post_piercing_entity_effect_is_supported(effect) {
        return false;
    }

    apply_supported_post_piercing_entity_effect(effect, level, user)
}

fn post_piercing_entity_effect_is_supported(effect: &EnchantmentEntityEffect) -> bool {
    match effect {
        EnchantmentEntityEffect::AllOf(effects) => effects
            .iter()
            .all(|effect| post_piercing_entity_effect_is_supported(effect)),
        EnchantmentEntityEffect::ChangeItemDamage { .. }
        | EnchantmentEntityEffect::ApplyExhaustion { .. }
        | EnchantmentEntityEffect::Ignite { .. } => true,
        EnchantmentEntityEffect::PlaySound { sounds, .. } => !sounds.is_empty(),
        EnchantmentEntityEffect::ApplyImpulse { direction, .. } => {
            direction.x == 0.0 && direction.y == 0.0
        }
        EnchantmentEntityEffect::ApplyMobEffect { to_apply, .. } => {
            matches!(to_apply, MobEffectSelection::Single(_))
        }
        EnchantmentEntityEffect::DamageEntity { .. }
        | EnchantmentEntityEffect::Unsupported { .. } => false,
    }
}

fn apply_supported_post_piercing_entity_effect(
    effect: &EnchantmentEntityEffect,
    level: i32,
    user: &dyn LivingEntity,
) -> bool {
    match effect {
        EnchantmentEntityEffect::AllOf(effects) => {
            let mut enchanted_item_broke = false;
            for effect in *effects {
                enchanted_item_broke |=
                    apply_supported_post_piercing_entity_effect(effect, level, user);
            }
            enchanted_item_broke
        }
        EnchantmentEntityEffect::ChangeItemDamage { amount } => {
            let amount = amount.calculate(level) as i32;
            let has_infinite_materials = user.has_infinite_materials();
            let mut item_broke = false;
            user.with_equipment_slot_mut(EquipmentSlot::MainHand, &mut |stack| {
                item_broke = stack.hurt_and_break(amount, has_infinite_materials);
            });
            item_broke
        }
        EnchantmentEntityEffect::ApplyExhaustion { amount } => {
            if let Some(player) = user.as_player() {
                player.cause_food_exhaustion(amount.calculate(level));
            }
            false
        }
        EnchantmentEntityEffect::ApplyImpulse {
            direction,
            coordinate_scale,
            magnitude,
        } => {
            let impulse = (user.look_angle() * direction.z)
                * *coordinate_scale
                * f64::from(magnitude.calculate(level));
            user.push_impulse(impulse);
            user.apply_post_impulse_grace_time(10);
            false
        }
        EnchantmentEntityEffect::PlaySound {
            sounds,
            volume,
            pitch,
        } => {
            let index = (level - 1).clamp(0, sounds.len() as i32 - 1) as usize;
            user.play_sound(sounds[index], *volume, *pitch);
            false
        }
        EnchantmentEntityEffect::Ignite { .. } | EnchantmentEntityEffect::ApplyMobEffect { .. } => {
            let mut ignored_item = ItemStack::empty();
            apply_supported_entity_effect(effect, level, user, user, &mut ignored_item)
        }
        EnchantmentEntityEffect::DamageEntity { .. }
        | EnchantmentEntityEffect::Unsupported { .. } => false,
    }
}

fn random_between(min: f32, max: f32) -> f32 {
    min + rand::random::<f32>() * (max - min)
}

fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

fn requirements_match(
    requirements: Option<&'static EnchantmentEffectRequirements>,
    context: &EnchantmentDamageContext<'_>,
) -> bool {
    let Some(requirements) = requirements else {
        return true;
    };

    matches!(requirements_state(requirements, context), Some(true))
}

fn post_attack_requirements_match(
    requirements: Option<&'static EnchantmentEffectRequirements>,
    context: &EnchantmentDamageContext<'_>,
    level: i32,
) -> bool {
    let Some(requirements) = requirements else {
        return true;
    };

    matches!(
        requirements_state_with_random(requirements, context, level),
        Some(true)
    )
}

fn requirements_state(
    requirements: &'static EnchantmentEffectRequirements,
    context: &EnchantmentDamageContext<'_>,
) -> Option<bool> {
    match requirements {
        EnchantmentEffectRequirements::AllOf(terms) => {
            let mut has_unknown = false;
            for term in *terms {
                match requirements_state(term, context) {
                    Some(true) => {}
                    Some(false) => return Some(false),
                    None => has_unknown = true,
                }
            }
            if has_unknown { None } else { Some(true) }
        }
        EnchantmentEffectRequirements::AnyOf(terms) => {
            let mut has_unknown = false;
            for term in *terms {
                match requirements_state(term, context) {
                    Some(true) => return Some(true),
                    Some(false) => {}
                    None => has_unknown = true,
                }
            }
            if has_unknown { None } else { Some(false) }
        }
        EnchantmentEffectRequirements::Inverted(term) => {
            requirements_state(term, context).map(|matched| !matched)
        }
        EnchantmentEffectRequirements::EntityProperties { entity, predicate } => context
            .entity_type(*entity)
            .and_then(|entity_type| entity_predicate_matches_type(predicate, entity_type)),
        EnchantmentEffectRequirements::DamageSourceProperties(predicate) => Some(
            damage_source_predicate_matches(predicate, context.damage_source),
        ),
        EnchantmentEffectRequirements::RandomChance { .. }
        | EnchantmentEffectRequirements::Unsupported { .. } => None,
    }
}

fn requirements_state_with_random(
    requirements: &'static EnchantmentEffectRequirements,
    context: &EnchantmentDamageContext<'_>,
    level: i32,
) -> Option<bool> {
    match requirements {
        EnchantmentEffectRequirements::AllOf(terms) => {
            let mut has_unknown = false;
            for term in *terms {
                match requirements_state_with_random(term, context, level) {
                    Some(true) => {}
                    Some(false) => return Some(false),
                    None => has_unknown = true,
                }
            }
            if has_unknown { None } else { Some(true) }
        }
        EnchantmentEffectRequirements::AnyOf(terms) => {
            let mut has_unknown = false;
            for term in *terms {
                match requirements_state_with_random(term, context, level) {
                    Some(true) => return Some(true),
                    Some(false) => {}
                    None => has_unknown = true,
                }
            }
            if has_unknown { None } else { Some(false) }
        }
        EnchantmentEffectRequirements::Inverted(term) => {
            requirements_state_with_random(term, context, level).map(|matched| !matched)
        }
        EnchantmentEffectRequirements::EntityProperties { entity, predicate } => context
            .entity_type(*entity)
            .and_then(|entity_type| entity_predicate_matches_type(predicate, entity_type)),
        EnchantmentEffectRequirements::DamageSourceProperties(predicate) => Some(
            damage_source_predicate_matches(predicate, context.damage_source),
        ),
        EnchantmentEffectRequirements::RandomChance { chance } => {
            Some(rand::random::<f32>() < chance.calculate(level))
        }
        EnchantmentEffectRequirements::Unsupported { .. } => None,
    }
}

fn entity_requirements_match(
    requirements: Option<&'static EnchantmentEffectRequirements>,
    entity: &dyn Entity,
) -> bool {
    let Some(requirements) = requirements else {
        return true;
    };

    matches!(entity_requirements_state(requirements, entity), Some(true))
}

fn entity_requirements_state(
    requirements: &'static EnchantmentEffectRequirements,
    entity: &dyn Entity,
) -> Option<bool> {
    match requirements {
        EnchantmentEffectRequirements::AllOf(terms) => {
            let mut has_unknown = false;
            for term in *terms {
                match entity_requirements_state(term, entity) {
                    Some(true) => {}
                    Some(false) => return Some(false),
                    None => has_unknown = true,
                }
            }
            if has_unknown { None } else { Some(true) }
        }
        EnchantmentEffectRequirements::AnyOf(terms) => {
            let mut has_unknown = false;
            for term in *terms {
                match entity_requirements_state(term, entity) {
                    Some(true) => return Some(true),
                    Some(false) => {}
                    None => has_unknown = true,
                }
            }
            if has_unknown { None } else { Some(false) }
        }
        EnchantmentEffectRequirements::Inverted(term) => {
            entity_requirements_state(term, entity).map(|matched| !matched)
        }
        EnchantmentEffectRequirements::EntityProperties {
            entity: EnchantmentEntityTarget::This,
            predicate,
        } => entity_predicate_matches_entity(predicate, entity),
        EnchantmentEffectRequirements::EntityProperties { .. }
        | EnchantmentEffectRequirements::DamageSourceProperties(_)
        | EnchantmentEffectRequirements::RandomChance { .. }
        | EnchantmentEffectRequirements::Unsupported { .. } => None,
    }
}

fn entity_predicate_matches_type(
    predicate: &EntityPredicate,
    entity_type: EntityTypeRef,
) -> Option<bool> {
    if predicate.unsupported
        || !matches!(predicate.vehicle, EntityVehiclePredicate::Any)
        || predicate.flags.has_constraints()
    {
        return None;
    }

    let type_matches = entity_type_predicate_matches(&predicate.entity_type, entity_type)?;
    if !type_matches {
        return Some(false);
    }

    match &predicate.type_specific {
        EntityTypeSpecificPredicate::Any => Some(true),
        EntityTypeSpecificPredicate::Unsupported => None,
        EntityTypeSpecificPredicate::Player(player_predicate) => {
            if entity_type != &vanilla_entities::PLAYER {
                return Some(false);
            }
            if player_predicate.unsupported
                || !player_predicate.game_modes.is_empty()
                || player_predicate.food_level_min.is_some()
            {
                None
            } else {
                Some(true)
            }
        }
    }
}

fn entity_predicate_matches_entity(
    predicate: &EntityPredicate,
    entity: &dyn Entity,
) -> Option<bool> {
    if predicate.unsupported {
        return None;
    }

    if !entity_type_predicate_matches(&predicate.entity_type, entity.entity_type())? {
        return Some(false);
    }

    match predicate.vehicle {
        EntityVehiclePredicate::Any => {}
        EntityVehiclePredicate::Present => {
            if entity.vehicle().is_none() {
                return Some(false);
            }
        }
        EntityVehiclePredicate::Unsupported => return None,
    }

    if predicate.flags.unsupported {
        return None;
    }
    if let Some(expected) = predicate.flags.is_fall_flying {
        let is_fall_flying = entity
            .as_living_entity()
            .is_some_and(LivingEntity::is_fall_flying);
        if is_fall_flying != expected {
            return Some(false);
        }
    }
    if let Some(expected) = predicate.flags.is_in_water
        && entity.is_in_water() != expected
    {
        return Some(false);
    }

    match &predicate.type_specific {
        EntityTypeSpecificPredicate::Any => Some(true),
        EntityTypeSpecificPredicate::Unsupported => None,
        EntityTypeSpecificPredicate::Player(player_predicate) => {
            let Some(player) = entity.as_player() else {
                return Some(false);
            };
            if player_predicate.unsupported {
                return None;
            }
            if !player_predicate.game_modes.is_empty()
                && !player_predicate
                    .game_modes
                    .iter()
                    .any(|game_mode| *game_mode == player.game_mode())
            {
                return Some(false);
            }
            if let Some(min_food_level) = player_predicate.food_level_min
                && player.food_data.lock().food_level < min_food_level
            {
                return Some(false);
            }
            Some(true)
        }
    }
}

fn entity_type_predicate_matches(
    predicate: &EntityTypePredicate,
    entity_type: EntityTypeRef,
) -> Option<bool> {
    match predicate {
        EntityTypePredicate::Any => Some(true),
        EntityTypePredicate::Type(expected) => Some(entity_type.key == *expected),
        EntityTypePredicate::Tag(tag) => Some(REGISTRY.entity_types.is_in_tag(entity_type, tag)),
        EntityTypePredicate::Unsupported => None,
    }
}

fn damage_source_predicate_matches(
    predicate: &DamageSourcePredicate,
    damage_source: &DamageSource,
) -> bool {
    if let Some(is_direct) = predicate.is_direct
        && damage_source.is_direct() != is_direct
    {
        return false;
    }

    predicate
        .tags
        .iter()
        .all(|tag| damage_source.is(&tag.tag) == tag.expected)
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::data_components::vanilla_components::{ENCHANTMENTS, ItemEnchantments};
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::items::ItemRef;
    use steel_registry::{
        test_support::init_test_registry, vanilla_damage_types, vanilla_enchantments,
        vanilla_entities, vanilla_items, vanilla_mob_effects,
    };
    use steel_utils::Identifier;
    use steel_utils::locks::SyncMutex;

    use super::*;
    use crate::entity::{EntityBase, LivingEntity, LivingEntityBase};

    struct TestLivingEntity {
        base: EntityBase,
        living_base: LivingEntityBase,
        health: SyncMutex<f32>,
        broken_slots: SyncMutex<Vec<EquipmentSlot>>,
        entity_type: EntityTypeRef,
    }

    impl TestLivingEntity {
        fn new(id: i32, entity_type: EntityTypeRef) -> Self {
            Self {
                base: EntityBase::new(id, DVec3::ZERO, entity_type.dimensions, Weak::new()),
                living_base: LivingEntityBase::new(entity_type),
                health: SyncMutex::new(20.0),
                broken_slots: SyncMutex::new(Vec::new()),
                entity_type,
            }
        }

        fn equip(&self, slot: EquipmentSlot, stack: ItemStack) {
            self.living_base.equipment().lock().set(slot, stack);
        }

        fn broken_slots(&self) -> Vec<EquipmentSlot> {
            self.broken_slots.lock().clone()
        }
    }

    impl Entity for TestLivingEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn as_living_entity(&self) -> Option<&dyn LivingEntity> {
            Some(self)
        }
    }

    impl LivingEntity for TestLivingEntity {
        fn living_base(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&self, health: f32) {
            *self.health.lock() = health.clamp(0.0, self.get_max_health());
        }

        fn get_absorption_amount(&self) -> f32 {
            0.0
        }

        fn set_absorption_amount(&self, _amount: f32) {}

        fn on_equipped_item_broken(&self, slot: EquipmentSlot) {
            self.broken_slots.lock().push(slot);
        }
    }

    fn enchanted_item(item: ItemRef, enchantment: Identifier, level: u32) -> ItemStack {
        let mut enchantments = ItemEnchantments::empty();
        enchantments.set(enchantment, level);

        let mut stack = ItemStack::new(item);
        stack.set(ENCHANTMENTS, enchantments);
        stack
    }

    fn assert_f32_eq(actual: f32, expected: f32) {
        assert_eq!(
            actual.to_bits(),
            expected.to_bits(),
            "actual: {actual}, expected: {expected}"
        );
    }

    #[test]
    fn unsupported_requirement_does_not_match_through_inversion() {
        static UNSUPPORTED: EnchantmentEffectRequirements =
            EnchantmentEffectRequirements::Unsupported {
                condition: Identifier::vanilla_static("match_tool"),
            };
        static INVERTED: EnchantmentEffectRequirements =
            EnchantmentEffectRequirements::Inverted(&UNSUPPORTED);

        let damage_source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK);
        let context = EnchantmentDamageContext::new(
            &vanilla_entities::PLAYER,
            Some(&vanilla_entities::PLAYER),
            Some(&vanilla_entities::PLAYER),
            &damage_source,
        );

        assert!(!requirements_match(Some(&UNSUPPORTED), &context));
        assert!(!requirements_match(Some(&INVERTED), &context));
    }

    #[test]
    fn damage_enchantments_match_target_entity_tags() {
        init_test_registry();

        let stack = enchanted_item(
            &vanilla_items::ITEMS.diamond_sword,
            Identifier::vanilla_static("smite"),
            5,
        );
        let damage_source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK);
        let zombie_context = EnchantmentDamageContext::new(
            &vanilla_entities::ZOMBIE,
            Some(&vanilla_entities::PLAYER),
            Some(&vanilla_entities::PLAYER),
            &damage_source,
        );
        let spider_context = EnchantmentDamageContext::new(
            &vanilla_entities::SPIDER,
            Some(&vanilla_entities::PLAYER),
            Some(&vanilla_entities::PLAYER),
            &damage_source,
        );

        assert_f32_eq(modify_damage(&stack, &zombie_context, 7.0), 19.5);
        assert_f32_eq(modify_damage(&stack, &spider_context, 7.0), 7.0);
    }

    #[test]
    fn projectile_knockback_checks_direct_attacker_entity_tag() {
        init_test_registry();

        let stack = enchanted_item(
            &vanilla_items::ITEMS.bow,
            Identifier::vanilla_static("punch"),
            2,
        );
        let damage_source = DamageSource::environment(&vanilla_damage_types::ARROW);
        let melee_context = EnchantmentDamageContext::new(
            &vanilla_entities::ZOMBIE,
            Some(&vanilla_entities::PLAYER),
            Some(&vanilla_entities::PLAYER),
            &damage_source,
        );
        let arrow_context = EnchantmentDamageContext::new(
            &vanilla_entities::ZOMBIE,
            Some(&vanilla_entities::PLAYER),
            Some(&vanilla_entities::ARROW),
            &damage_source,
        );

        assert_f32_eq(modify_knockback(&stack, &melee_context, 0.0), 0.0);
        assert_f32_eq(modify_knockback(&stack, &arrow_context, 0.0), 2.0);
    }

    #[test]
    fn damage_source_properties_match_damage_type_tags() {
        init_test_registry();

        let stack = enchanted_item(
            &vanilla_items::ITEMS.diamond_sword,
            Identifier::vanilla_static("fire_protection"),
            4,
        );
        let fire_source = DamageSource::environment(&vanilla_damage_types::IN_FIRE);
        let fall_source = DamageSource::environment(&vanilla_damage_types::FALL);
        let fire_context =
            EnchantmentDamageContext::new(&vanilla_entities::PLAYER, None, None, &fire_source);
        let fall_context =
            EnchantmentDamageContext::new(&vanilla_entities::PLAYER, None, None, &fall_source);

        assert_f32_eq(
            apply_value_effects(
                &stack,
                EnchantmentEffectComponent::DamageProtection,
                &fire_context,
                0.0,
            ),
            8.0,
        );
        assert_f32_eq(
            apply_value_effects(
                &stack,
                EnchantmentEffectComponent::DamageProtection,
                &fall_context,
                0.0,
            ),
            0.0,
        );
    }

    #[test]
    fn damage_immunity_matches_equipment_slot_and_requirements() {
        init_test_registry();

        let boots = enchanted_item(
            &vanilla_items::ITEMS.leather_boots,
            Identifier::vanilla_static("frost_walker"),
            1,
        );
        let victim = TestLivingEntity::new(1, &vanilla_entities::PLAYER);
        victim.equip(EquipmentSlot::Feet, boots);

        assert!(is_immune_to_damage(
            &victim,
            &DamageSource::environment(&vanilla_damage_types::HOT_FLOOR)
        ));
        assert!(!is_immune_to_damage(
            &victim,
            &DamageSource::environment(&vanilla_damage_types::IN_FIRE)
        ));

        let helmet = enchanted_item(
            &vanilla_items::ITEMS.leather_helmet,
            Identifier::vanilla_static("frost_walker"),
            1,
        );
        let wrong_slot_victim = TestLivingEntity::new(2, &vanilla_entities::PLAYER);
        wrong_slot_victim.equip(EquipmentSlot::Head, helmet);

        assert!(!is_immune_to_damage(
            &wrong_slot_victim,
            &DamageSource::environment(&vanilla_damage_types::HOT_FLOOR)
        ));
    }

    #[test]
    fn post_attack_ignite_applies_to_direct_melee_victim() {
        init_test_registry();

        let attacker = TestLivingEntity::new(1, &vanilla_entities::PLAYER);
        let victim = TestLivingEntity::new(2, &vanilla_entities::ZOMBIE);
        let stack = enchanted_item(
            &vanilla_items::ITEMS.diamond_sword,
            Identifier::vanilla_static("fire_aspect"),
            2,
        );
        let damage_source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(attacker.id())
            .with_direct_entity(attacker.id());
        let context = EnchantmentPostAttackContext::new(
            &victim,
            Some(&attacker),
            Some(&attacker),
            &damage_source,
        );

        do_post_attack_effects_from_item(&stack, &context);

        assert_eq!(victim.remaining_fire_ticks(), 160);
    }

    #[test]
    fn post_attack_effects_match_enchantment_slot() {
        init_test_registry();

        let attacker = TestLivingEntity::new(1, &vanilla_entities::PLAYER);
        let victim = TestLivingEntity::new(2, &vanilla_entities::ZOMBIE);
        let mut stack = enchanted_item(
            &vanilla_items::ITEMS.diamond_sword,
            Identifier::vanilla_static("fire_aspect"),
            1,
        );
        let damage_source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(attacker.id())
            .with_direct_entity(attacker.id());
        let context = EnchantmentPostAttackContext::new(
            &victim,
            Some(&attacker),
            Some(&attacker),
            &damage_source,
        );

        apply_post_attack_effects(
            &mut stack,
            Some(EquipmentSlot::Head),
            EnchantmentTarget::Attacker,
            &context,
        );
        assert_eq!(victim.remaining_fire_ticks(), 0);

        apply_post_attack_effects(
            &mut stack,
            Some(EquipmentSlot::MainHand),
            EnchantmentTarget::Attacker,
            &context,
        );
        assert_eq!(victim.remaining_fire_ticks(), 80);
    }

    #[test]
    fn post_attack_change_item_damage_calls_equipped_break_hook() {
        init_test_registry();

        let attacker = TestLivingEntity::new(1, &vanilla_entities::ZOMBIE);
        let victim = TestLivingEntity::new(2, &vanilla_entities::PLAYER);
        let mut chestplate = enchanted_item(
            &vanilla_items::ITEMS.diamond_chestplate,
            Identifier::vanilla_static("thorns"),
            10,
        );
        chestplate.set_damage_value(chestplate.get_max_damage() - 1);
        victim.equip(EquipmentSlot::Chest, chestplate);

        let damage_source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(attacker.id())
            .with_direct_entity(attacker.id());
        let context = EnchantmentPostAttackContext::new(
            &victim,
            Some(&attacker),
            Some(&attacker),
            &damage_source,
        );
        let source = ItemStack::empty();

        do_post_attack_effects_with_item_source(&victim, &source, &context);

        let mut chestplate_broke = false;
        victim.with_equipment_slot(EquipmentSlot::Chest, &mut |stack| {
            chestplate_broke = stack.is_empty();
        });
        assert!(chestplate_broke);
        assert_eq!(victim.broken_slots(), vec![EquipmentSlot::Chest]);
    }

    #[test]
    fn lunge_post_piercing_requirements_and_effect_are_supported() {
        init_test_registry();

        let user = TestLivingEntity::new(1, &vanilla_entities::ZOMBIE);
        let effects = vanilla_enchantments::LUNGE.effects.post_piercing_attack;
        assert_eq!(effects.len(), 1);
        assert!(entity_requirements_match(effects[0].requirements, &user));
        assert!(post_piercing_entity_effect_is_supported(&effects[0].effect));
    }

    #[test]
    fn post_attack_ignite_skips_indirect_damage_source() {
        init_test_registry();

        let attacker = TestLivingEntity::new(1, &vanilla_entities::PLAYER);
        let direct_entity = TestLivingEntity::new(2, &vanilla_entities::PLAYER);
        let victim = TestLivingEntity::new(3, &vanilla_entities::ZOMBIE);
        let stack = enchanted_item(
            &vanilla_items::ITEMS.diamond_sword,
            Identifier::vanilla_static("fire_aspect"),
            2,
        );
        let damage_source = DamageSource::environment(&vanilla_damage_types::ARROW)
            .with_causing_entity(attacker.id())
            .with_direct_entity(direct_entity.id());
        let context = EnchantmentPostAttackContext::new(
            &victim,
            Some(&attacker),
            Some(&direct_entity),
            &damage_source,
        );

        do_post_attack_effects_from_item(&stack, &context);

        assert_eq!(victim.remaining_fire_ticks(), 0);
    }

    #[test]
    fn post_attack_mob_effect_matches_victim_predicate() {
        init_test_registry();

        let attacker = TestLivingEntity::new(1, &vanilla_entities::PLAYER);
        let spider = TestLivingEntity::new(2, &vanilla_entities::SPIDER);
        let zombie = TestLivingEntity::new(3, &vanilla_entities::ZOMBIE);
        let stack = enchanted_item(
            &vanilla_items::ITEMS.diamond_sword,
            Identifier::vanilla_static("bane_of_arthropods"),
            1,
        );
        let damage_source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(attacker.id())
            .with_direct_entity(attacker.id());
        let spider_context = EnchantmentPostAttackContext::new(
            &spider,
            Some(&attacker),
            Some(&attacker),
            &damage_source,
        );
        let zombie_context = EnchantmentPostAttackContext::new(
            &zombie,
            Some(&attacker),
            Some(&attacker),
            &damage_source,
        );

        do_post_attack_effects_from_item(&stack, &spider_context);
        do_post_attack_effects_from_item(&stack, &zombie_context);

        let Some(slowness) = spider.mob_effect(vanilla_mob_effects::SLOWNESS) else {
            panic!("bane of arthropods should apply slowness to spiders");
        };
        assert_eq!(slowness.duration(), 30);
        assert_eq!(slowness.amplifier(), 3);
        assert!(zombie.mob_effect(vanilla_mob_effects::SLOWNESS).is_none());
    }
}
