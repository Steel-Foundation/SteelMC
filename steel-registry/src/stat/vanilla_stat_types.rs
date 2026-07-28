//! This module defines all the stat types from Vanilla.

use crate::REGISTRY;
use crate::blocks::BlockRegistry;
use crate::entity_type::EntityTypeRegistry;
use crate::items::ItemRegistry;
use crate::stat::custom::CustomStatRegistry;
use crate::stat::registry::{StatType, StatTypeRegistry};
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

/// Creates a Vanilla stat type from its given name. This name is used for its identifier and
/// display name.
macro_rules! vanilla_stat_type {
    ($name: literal) => {
        StatType::new(Identifier::vanilla_static($name), None)
    };
    ($name: literal, $translation_name: ident) => {
        StatType::new(
            Identifier::vanilla_static($name),
            Some(TextComponent::translated(
                translations::$translation_name.msg(),
            )),
        )
    };
}

// Only the block and item stat types have an actual zero-argument translation for their names.
pub const BLOCK_MINED: StatType<BlockRegistry> =
    vanilla_stat_type!("mined", STAT_TYPE_MINECRAFT_BROKEN);

pub const ITEM_CRAFTED: StatType<ItemRegistry> =
    vanilla_stat_type!("crafted", STAT_TYPE_MINECRAFT_CRAFTED);
pub const ITEM_USED: StatType<ItemRegistry> = vanilla_stat_type!("used", STAT_TYPE_MINECRAFT_USED);
pub const ITEM_BROKEN: StatType<ItemRegistry> =
    vanilla_stat_type!("broken", STAT_TYPE_MINECRAFT_BROKEN);
pub const ITEM_PICKED_UP: StatType<ItemRegistry> =
    vanilla_stat_type!("picked_up", STAT_TYPE_MINECRAFT_PICKED_UP);
pub const ITEM_DROPPED: StatType<ItemRegistry> =
    vanilla_stat_type!("dropped", STAT_TYPE_MINECRAFT_DROPPED);

pub const ENTITY_KILLED: StatType<EntityTypeRegistry> = vanilla_stat_type!("killed");
pub const ENTITY_KILLED_BY: StatType<EntityTypeRegistry> = vanilla_stat_type!("killed_by");

pub const CUSTOM: StatType<CustomStatRegistry> = vanilla_stat_type!("custom");

/// Registers all vanilla stat types.
///
/// IMPORTANT: The registration order MUST match vanilla's Stats.java exactly,
/// as the component's network ID is determined by its registration order.
pub fn register_vanilla_stat_types(registry: &mut StatTypeRegistry) {
    // 0: mined
    registry.register(BLOCK_MINED, || &REGISTRY.blocks);

    // 1: crafted
    registry.register(ITEM_CRAFTED, || &REGISTRY.items);
    // 2: used
    registry.register(ITEM_USED, || &REGISTRY.items);
    // 3: broken
    registry.register(ITEM_BROKEN, || &REGISTRY.items);
    // 4: picked_up
    registry.register(ITEM_PICKED_UP, || &REGISTRY.items);
    // 5: dropped
    registry.register(ITEM_DROPPED, || &REGISTRY.items);

    // 6: killed
    registry.register(ENTITY_KILLED, || &REGISTRY.entity_types);
    // 7: killed_by
    registry.register(ENTITY_KILLED_BY, || &REGISTRY.entity_types);

    // 8: custom
    registry.register(CUSTOM, || &REGISTRY.custom_stats);
}

#[cfg(test)]
mod tests {
    use crate::RegistryExt;
    use crate::stat::StatTypeRegistry;
    use crate::stat::vanilla_stat_types::register_vanilla_stat_types;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct ExtractedStatTypeEntry {
        id: usize,
        key: String,
    }

    #[test]
    fn registry_matches_extracted_stat_types() {
        let entries: Vec<ExtractedStatTypeEntry> =
            serde_json::from_str(include_str!("../../build_assets/stat_types.json"))
                .expect("extracted stat types should be valid");

        let mut registry = StatTypeRegistry::new();
        register_vanilla_stat_types(&mut registry);

        assert_eq!(entries.len(), 9);
        assert_eq!(registry.len(), entries.len());

        for (expected_id, entry) in entries.into_iter().enumerate() {
            assert_eq!(
                entry.id, expected_id,
                "the IDs of stat type {} don't match",
                entry.key
            );

            let stat_entry = registry
                .by_id(entry.id)
                .unwrap_or_else(|| panic!("missing stat type registry ID {}", entry.id));

            assert_eq!(stat_entry.key.to_string(), entry.key);
        }
    }
}
