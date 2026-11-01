//! Tests against actual extracted vanilla states.
use super::*;
use crate::blocks::shapes::VoxelShape;
use crate::{vanilla_blocks, vanilla_items};
use text_components::TextComponent;

type GeometryCase = (
    &'static str,
    BlockRef,
    &'static [(&'static str, &'static str)],
);

#[test]
fn selected_geometry_matches_every_extracted_shape_channel() -> Result<(), String> {
    let mut registry = Registry::new_vanilla();
    let cases: &[GeometryCase] = &[
        ("cube", &vanilla_blocks::OAK_PLANKS, &[]),
        ("invisible", &vanilla_blocks::BARRIER, &[]),
        ("no_collision", &vanilla_blocks::STRUCTURE_VOID, &[]),
        (
            "bottom_slab",
            &vanilla_blocks::OAK_SLAB,
            &[("type", "bottom")],
        ),
        ("top_slab", &vanilla_blocks::OAK_SLAB, &[("type", "top")]),
        (
            "double_slab",
            &vanilla_blocks::OAK_SLAB,
            &[("type", "double")],
        ),
        (
            "stairs",
            &vanilla_blocks::OAK_STAIRS,
            &[("facing", "east"), ("half", "top"), ("shape", "inner_left")],
        ),
        (
            "fence",
            &vanilla_blocks::OAK_FENCE,
            &[("north", "true"), ("east", "true")],
        ),
        (
            "wall",
            &vanilla_blocks::COBBLESTONE_WALL,
            &[("west", "tall"), ("south", "low")],
        ),
        (
            "trapdoor",
            &vanilla_blocks::IRON_TRAPDOOR,
            &[("open", "true"), ("facing", "west")],
        ),
        ("snow", &vanilla_blocks::SNOW, &[("layers", "3")]),
    ];
    for &(name, geometry, properties) in cases {
        let carrier = registry
            .blocks
            .state_id_from_block_defaulted_properties(geometry, properties.iter().copied())
            .ok_or("invalid test carrier state")?;
        let block = registry.register_client_block(
            Identifier::new("geometry_test", name),
            carrier,
            &vanilla_blocks::OAK_PLANKS,
            None,
            Vec::new(),
        )?;
        let custom = registry.blocks.get_default_state_id(block);
        assert_eq!(
            registry.blocks.get_static_collision_shape(custom),
            registry.blocks.get_static_collision_shape(carrier),
            "{name}"
        );
        assert_eq!(
            registry.blocks.get_static_support_shape(custom),
            registry.blocks.get_static_support_shape(carrier),
            "{name}"
        );
        assert_eq!(
            registry.blocks.get_static_outline_shape(custom),
            registry.blocks.get_static_outline_shape(carrier),
            "{name}"
        );
        assert_eq!(
            registry.blocks.get_occlusion_shape(custom),
            registry.blocks.get_occlusion_shape(carrier),
            "{name}"
        );
        assert_eq!(
            registry.blocks.get_static_interaction_shape(custom),
            registry.blocks.get_static_interaction_shape(carrier),
            "{name}"
        );
        assert_eq!(
            registry.blocks.get_static_visual_shape(custom),
            registry.blocks.get_static_visual_shape(carrier),
            "{name}"
        );
        assert_eq!(
            registry.blocks.get_light_properties(custom),
            registry.blocks.get_light_properties(carrier),
            "{name}"
        );
        assert_eq!(
            registry.blocks.is_suffocating(custom),
            registry.blocks.is_suffocating(carrier),
            "{name}"
        );
        assert_eq!(
            registry.blocks.is_static_redstone_conductor(custom),
            registry.blocks.is_static_redstone_conductor(carrier),
            "{name}"
        );
        assert_eq!(
            block.config.destroy_time,
            vanilla_blocks::OAK_PLANKS.config.destroy_time
        );
        let representation = registry
            .client_content
            .block(block)
            .ok_or("missing custom geometry")?;
        assert_eq!(representation.carrier, carrier);
        assert_eq!(representation.material, Some(&vanilla_blocks::OAK_PLANKS));
        if name == "no_collision" {
            assert_eq!(
                registry.blocks.get_static_collision_shape(custom),
                VoxelShape::EMPTY
            );
        }
    }
    Ok(())
}

#[test]
fn unsupported_state_geometry_leaves_no_registration() -> Result<(), String> {
    let mut registry = Registry::new_vanilla();
    for (name, geometry, properties) in [
        (
            "wet_slab",
            &vanilla_blocks::OAK_SLAB,
            vec![("waterlogged", "true")],
        ),
        ("contextual", &vanilla_blocks::POWDER_SNOW, vec![]),
        ("air", &vanilla_blocks::AIR, vec![]),
        ("block_entity", &vanilla_blocks::BEACON, vec![]),
    ] {
        let key = Identifier::new("geometry_test", name);
        let state = registry
            .blocks
            .state_id_from_block_defaulted_properties(geometry, properties)
            .ok_or("invalid rejection test state")?;
        let count = registry.blocks.next_state_id;
        assert!(
            registry
                .register_client_block(
                    key.clone(),
                    state,
                    &vanilla_blocks::OAK_PLANKS,
                    None,
                    Vec::new()
                )
                .is_err()
        );
        assert!(registry.blocks.by_key(&key).is_none());
        assert_eq!(registry.blocks.next_state_id, count);
    }
    Ok(())
}

fn item(path: &'static str) -> Item {
    Item::new(
        Identifier::new_static("client_test", path),
        TextComponent::from("Test item".to_owned()),
        None,
    )
}

#[test]
fn rejected_item_or_carrier_does_not_leave_a_partial_block_registration() {
    let mut registry = Registry::new_vanilla();
    assert!(
        registry
            .register_client_item(item("token"), &vanilla_items::PAPER)
            .is_ok()
    );
    let counts = (
        registry.blocks.len(),
        registry.items.len(),
        registry.loot_tables.len(),
    );
    for (path, carrier, item) in [
        ("duplicate", &vanilla_blocks::OAK_PLANKS, item("token")),
        ("barrier", &vanilla_blocks::BARRIER, item("barrier_item")),
        ("block_entity", &vanilla_blocks::BEACON, item("beacon_item")),
    ] {
        let key = Identifier::new_static("client_test", path);
        assert!(
            registry
                .register_client_block_item(BlockItemRegistration {
                    key: key.clone(),
                    carrier: registry.blocks.get_default_state_id(carrier),
                    material: carrier,
                    displays: Vec::new(),
                    item,
                    item_fallback: &vanilla_items::PAPER,
                    drop_self: true,
                })
                .is_err()
        );
        assert!(registry.blocks.by_key(&key).is_none());
        assert_eq!(
            (
                registry.blocks.len(),
                registry.items.len(),
                registry.loot_tables.len()
            ),
            counts
        );
    }
}
