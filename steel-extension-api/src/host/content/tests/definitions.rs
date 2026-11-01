use super::super::*;
use crate::content::{BlockProperties, ItemAppearance, ItemDefinition, ItemDisplay, VoxelShape};
use steel_registry::blocks::shapes::{BooleanOp, join_is_not_empty};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockStateId, axis::Axis};

#[test]
fn independent_properties_and_equivalent_box_unions_project_without_a_template()
-> Result<(), String> {
    let mut registry = Registry::new_vanilla();
    let definition = BlockDefinition::new("defined:box")
        .properties(BlockProperties::new().strength(4.0, 9.0).sound("WOOD"))
        .shape(
            VoxelShape::box_shape(0.0, 0.0, 0.0, 8.0, 16.0, 16.0)
                .or(VoxelShape::box_shape(8.0, 0.0, 0.0, 16.0, 16.0, 16.0)),
        );
    let state = register_block(&mut registry, definition)?;
    let block = registry
        .blocks
        .by_state_id(BlockStateId(state.id))
        .ok_or("missing independent block")?;
    let projected = registry
        .client_content
        .block(block)
        .ok_or("missing projection")?;
    assert!(projected.material.is_none());
    assert_eq!(block.config.destroy_time, 4.0);
    assert_eq!(block.config.explosion_resistance, 9.0);
    assert_eq!(
        projected.carrier,
        registry
            .blocks
            .get_default_state_id(&vanilla_blocks::BARRIER)
    );
    assert!(!join_is_not_empty(
        block.get_collision_shape(0),
        registry
            .blocks
            .get_static_collision_shape(projected.carrier),
        BooleanOp::NotSame
    ));
    Ok(())
}

fn vertical(key: &str) -> BlockDefinition {
    BlockDefinition::new(key).shape(VoxelShape::box_shape(0.0, 0.0, 8.0, 16.0, 16.0, 16.0))
}

#[test]
fn unsupported_collision_requires_explicit_approximation_without_orphan_items() -> Result<(), String>
{
    let mut registry = Registry::new_vanilla();
    let item = ItemDefinition {
        key: "defined:vertical".into(),
        name: "Vertical".into(),
        max_stack_size: 64,
        appearance: ItemAppearance {
            item: "minecraft:stone_slab".into(),
            model: None.into(),
            profile: None.into(),
        },
    };
    let count = registry.blocks.next_state_id;
    assert!(
        register_block_item(
            &mut registry,
            BlockItemDefinition {
                block: vertical("defined:vertical"),
                item,
                drop_self: true
            }
        )
        .is_err()
    );
    let key = Identifier::new_static("defined", "vertical");
    assert!(registry.blocks.by_key(&key).is_none());
    assert!(registry.items.by_key(&key).is_none());
    assert_eq!(registry.blocks.next_state_id, count);
    let state = register_block(
        &mut registry,
        vertical("defined:vertical").allow_approximation(),
    )?;
    let block = registry
        .blocks
        .by_state_id(BlockStateId(state.id))
        .ok_or("missing vertical")?;
    let projection = registry
        .client_content
        .block(block)
        .ok_or("missing projection")?;
    let client = registry
        .blocks
        .get_static_collision_shape(projection.carrier);
    assert!(
        !client.is_empty(),
        "a solid subset must beat a non-colliding carrier"
    );
    assert!(join_is_not_empty(
        block.get_collision_shape(0),
        client,
        BooleanOp::NotSame
    ));
    assert!(!join_is_not_empty(
        client,
        block.get_collision_shape(0),
        BooleanOp::OnlyFirst
    ));
    assert_eq!(block.get_collision_shape(0).min(Axis::Z), 0.5);
    Ok(())
}

#[test]
fn malformed_shapes_transforms_and_properties_fail_before_registration() {
    let invalid = vec![
        BlockDefinition::new("defined:invalid").shape(VoxelShape::box_shape(
            0.0,
            0.0,
            0.0,
            f64::NAN,
            16.0,
            16.0,
        )),
        BlockDefinition::new("defined:invalid")
            .shape(VoxelShape::box_shape(16.0, 0.0, 0.0, 0.0, 16.0, 16.0)),
        BlockDefinition::new("defined:invalid")
            .properties(BlockProperties::new().strength(f32::INFINITY, 1.0)),
        BlockDefinition::new("defined:invalid")
            .properties(BlockProperties::new().sound("UNKNOWN_SOUND_TYPE")),
        BlockDefinition::new("defined:invalid").displays([ItemDisplay {
            appearance: ItemAppearance {
                item: "minecraft:stone".into(),
                model: None.into(),
                profile: None.into(),
            },
            translation: [0.5; 3],
            scale: [1.0; 3],
            rotation: [0.0; 4],
        }]),
    ];
    let mut registry = Registry::new_vanilla();
    let count = registry.blocks.next_state_id;
    for definition in invalid {
        assert!(register_block(&mut registry, definition).is_err());
        assert_eq!(registry.blocks.next_state_id, count);
        assert!(
            registry
                .blocks
                .by_key(&Identifier::new_static("defined", "invalid"))
                .is_none()
        );
    }
}
