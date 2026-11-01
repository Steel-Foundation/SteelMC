use super::*;
use crate::content::{BlockProperties, BlockProperty, BlockStateDefinition};
use steel_registry::vanilla_blocks;
use steel_utils::BlockStateId;

fn definition(properties: &[(&str, &str)]) -> BlockDefinition {
    BlockDefinition::new("geometry_test:slab")
        .properties(BlockProperties::of_full_copy("minecraft:oak_planks"))
        .vanilla_geometry(BlockStateDefinition {
            block: "minecraft:oak_slab".into(),
            properties: properties
                .iter()
                .map(|&(name, value)| BlockProperty {
                    name: name.into(),
                    value: value.into(),
                })
                .collect(),
        })
}

#[test]
fn native_definition_resolves_properties_over_vanilla_defaults() -> Result<(), String> {
    let mut registry = Registry::new_vanilla();
    let state = register_block(&mut registry, definition(&[("type", "top")]))?;
    let block = registry
        .blocks
        .by_state_id(BlockStateId(state.id))
        .ok_or("missing block")?;
    let selected = registry
        .client_content
        .block(block)
        .ok_or("missing representation")?
        .carrier;
    assert_eq!(
        registry.blocks.get_property_str(selected, "type"),
        Some("top")
    );
    assert_eq!(
        registry.blocks.get_property_str(selected, "waterlogged"),
        Some("false")
    );
    assert_eq!(
        registry
            .client_content
            .block(block)
            .ok_or("missing representation")?
            .material,
        Some(&vanilla_blocks::OAK_PLANKS)
    );
    Ok(())
}

#[test]
fn invalid_native_properties_do_not_register_partial_content() {
    for properties in [
        vec![("type", "sideways")],
        vec![("missing", "true")],
        vec![("type", "top"), ("type", "bottom")],
        vec![("waterlogged", "true")],
    ] {
        let mut registry = Registry::new_vanilla();
        let count = registry.blocks.next_state_id;
        assert!(register_block(&mut registry, definition(&properties)).is_err());
        assert_eq!(registry.blocks.next_state_id, count);
        assert!(
            registry
                .blocks
                .by_key(&Identifier::new_static("geometry_test", "slab"))
                .is_none()
        );
    }
}

mod definitions;

mod horizontal;
