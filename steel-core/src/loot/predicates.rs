use steel_registry::{
    REGISTRY, RegistryExt, TaggedRegistryExt, equipment::EquipmentSlot, item_stack::ItemStack,
};
use steel_utils::Identifier;

use super::{DamageSourceInfo, EntityRef};
use crate::entity::Entity;

pub(super) fn damage_has_tag(source: DamageSourceInfo<'_>, tag: &Identifier) -> bool {
    if let Some(kind) = source
        .damage_type
        .and_then(|key| REGISTRY.damage_types.by_key(key))
    {
        return REGISTRY.damage_types.is_in_tag(kind, tag);
    }
    source.tags.contains(tag)
}

pub(super) fn item_enchantment_matches(
    item: &ItemStack,
    key: &Identifier,
    tag: bool,
    min: i32,
    max: i32,
) -> bool {
    if !tag {
        let level = item.get_enchantment_level(key);
        return level > 0 && level >= min && level <= max;
    }
    item.get_enchantments().is_some_and(|enchantments| {
        enchantments.iter().any(|(key_id, level)| {
            let level = *level as i32;
            level > 0
                && level >= min
                && level <= max
                && REGISTRY
                    .enchantments
                    .by_key(key_id)
                    .is_some_and(|enchantment| REGISTRY.enchantments.is_in_tag(enchantment, key))
        })
    })
}

pub(super) fn equipment_matches(
    entity: EntityRef<'_>,
    slot: EquipmentSlot,
    predicate: impl Fn(&ItemStack) -> bool,
) -> bool {
    if let Some(equipment) = entity.equipment {
        return predicate(equipment.get(slot));
    }
    let Some(living) = entity.entity.and_then(Entity::as_living_entity) else {
        return false;
    };
    let mut matches = false;
    living.with_equipment_slot(slot, &mut |item| matches = predicate(item));
    matches
}
