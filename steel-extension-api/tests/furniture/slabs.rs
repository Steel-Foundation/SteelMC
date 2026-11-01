use std::{
    error::Error,
    f32::consts::{FRAC_PI_2, PI},
};

use glam::{DVec3, Quat};
use steel_core::{entity::Entity, physics::WorldCollisionProvider};
use steel_registry::{
    REGISTRY, RegistryExt,
    blocks::{
        block_state_ext::BlockStateExt,
        shapes::{BooleanOp, join_is_not_empty},
    },
    client_content::client_block_state,
    data_components::vanilla_components::ITEM_MODEL,
    item_stack::ItemStack,
    vanilla_items,
};
use steel_utils::{BlockPos, Identifier, WorldAabb, axis::Axis};

use super::{fixture::Fixture, helpers};

pub(super) fn check(fixture: &Fixture, pack: bool) -> Result<(), Box<dyn Error>> {
    let mut materials = 0;
    for (_, source) in REGISTRY.blocks.iter() {
        if !REGISTRY.client_content.is_vanilla_block(source)
            || REGISTRY
                .blocks
                .state_id_from_block_defaulted_properties(
                    source,
                    [("type", "double"), ("waterlogged", "false")],
                )
                .is_none()
        {
            continue;
        }
        materials += 1;
        let name = source
            .key
            .path
            .strip_suffix("_slab")
            .ok_or("unexpected slab path")?;
        let key = Identifier::new("furniture", format!("{name}_vertical_slab"));
        let block = REGISTRY
            .blocks
            .by_key(&key)
            .ok_or("missing material variant")?;
        let state = block.default_state();
        let shape = state.get_static_collision_shape();
        assert_eq!(shape.min(Axis::Z), 0.0);
        assert_eq!(shape.max(Axis::Z), 0.5);
        assert_eq!(shape.min(Axis::Y), 0.0);
        assert_eq!(shape.max(Axis::Y), 1.0);
        let client = client_block_state(state).get_static_collision_shape();
        assert!(!client.is_empty());
        assert!(!join_is_not_empty(client, shape, BooleanOp::OnlyFirst));
        let model = REGISTRY
            .client_content
            .block(block)
            .ok_or("missing model")?;
        assert_eq!(model.material, Some(source));
        let part = model.displays.first().ok_or("missing slab display")?;
        let rotation = Quat::from_rotation_y(PI)
            * if pack {
                Quat::IDENTITY
            } else {
                Quat::from_rotation_x(-FRAC_PI_2)
            };
        assert!(part.rotation.abs_diff_eq(rotation, 1e-6));
        let item = REGISTRY.items.by_block(block);
        let expected = if pack {
            Some(key)
        } else {
            REGISTRY.items.by_block(source).components.get(ITEM_MODEL)
        };
        assert_eq!(item.components.get(ITEM_MODEL), expected);
        assert_eq!(
            part.item.get(ITEM_MODEL),
            item.components.get(ITEM_MODEL).as_ref()
        );
    }
    assert!(materials > 0);
    let pos = BlockPos::new(8, 65, 8);
    for name in [
        "spruce_vertical_slab",
        "stone_vertical_slab",
        "cut_copper_vertical_slab",
        "vertical_slab",
    ] {
        fixture
            .player
            .try_set_position(DVec3::new(6.5, 65.0, 8.5))?;
        helpers::place(fixture, name, pos)?;
        let state = fixture.world.get_block_state(pos);
        let collision = WorldCollisionProvider::new(&fixture.world);
        assert!(collision.has_entity_context_collision(
            WorldAabb::new(8.1, 65.1, 8.6, 8.9, 65.9, 8.9),
            65.1,
            false
        ));
        assert!(!collision.has_entity_context_collision(
            WorldAabb::new(8.1, 65.1, 8.1, 8.9, 65.9, 8.4),
            65.1,
            false
        ));
        helpers::interact(fixture, pos);
        assert_eq!(fixture.world.get_block_state(pos), state);
        let loaded = helpers::saved(fixture)?;
        assert_eq!(loaded.chunk.get_block_state(pos), state);
        fixture
            .player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::NETHERITE_PICKAXE));
        helpers::mine(fixture, pos);
        assert!(fixture.world.get_block_state(pos).is_air());
    }
    Ok(())
}
