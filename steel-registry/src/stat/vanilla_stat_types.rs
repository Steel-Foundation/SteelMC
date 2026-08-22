//! This module defines all the stat types from Vanilla.

use crate::REGISTRY;
use crate::blocks::BlockRegistry;
use crate::entity_type::EntityTypeRegistry;
use crate::items::ItemRegistry;
use crate::stat::custom::CustomStatRegistry;
use crate::stat::registry::{StatType, StatTypeRegistry};
use steel_utils::Identifier;

pub static BLOCK_MINED: StatType<BlockRegistry> =
    StatType::new(Identifier::vanilla_static("mined"));

pub static ITEM_CRAFTED: StatType<ItemRegistry> =
    StatType::new(Identifier::vanilla_static("crafted"));
pub static ITEM_USED: StatType<ItemRegistry> = StatType::new(Identifier::vanilla_static("used"));
pub static ITEM_BROKEN: StatType<ItemRegistry> =
    StatType::new(Identifier::vanilla_static("broken"));
pub static ITEM_PICKED_UP: StatType<ItemRegistry> =
    StatType::new(Identifier::vanilla_static("picked_up"));
pub static ITEM_DROPPED: StatType<ItemRegistry> =
    StatType::new(Identifier::vanilla_static("dropped"));

pub static ENTITY_KILLED: StatType<EntityTypeRegistry> =
    StatType::new(Identifier::vanilla_static("killed"));
pub static ENTITY_KILLED_BY: StatType<EntityTypeRegistry> =
    StatType::new(Identifier::vanilla_static("killed_by"));

pub static CUSTOM: StatType<CustomStatRegistry> =
    StatType::new(Identifier::vanilla_static("custom"));

/// Registers all vanilla stat types.
///
/// IMPORTANT: The registration order MUST match vanilla's Stats.java exactly,
/// as the component's network ID is determined by its registration order.
pub fn register_vanilla_stat_types(registry: &mut StatTypeRegistry) {
    // 0: mined
    registry.register(&BLOCK_MINED, || &REGISTRY.blocks);
    // 1: crafted
    registry.register(&ITEM_CRAFTED, || &REGISTRY.items);
    // 2: used
    registry.register(&ITEM_USED, || &REGISTRY.items);
    // 3: broken
    registry.register(&ITEM_BROKEN, || &REGISTRY.items);
    // 4: picked_up
    registry.register(&ITEM_PICKED_UP, || &REGISTRY.items);
    // 5: dropped
    registry.register(&ITEM_DROPPED, || &REGISTRY.items);
    // 6: killed
    registry.register(&ENTITY_KILLED, || &REGISTRY.entity_types);
    // 7: killed_by
    registry.register(&ENTITY_KILLED_BY, || &REGISTRY.entity_types);
    // 8: custom
    registry.register(&CUSTOM, || &REGISTRY.custom_stats);
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
