// Moved from entity/projectile/throwable.rs with the entities it depends on.

    #[test]
    fn bubble_column_affects_throwable_projectile_before_its_first_movement() {
        init_vanilla_registry();
        init_behaviors();

        let world_fixture = fresh_test_world("throwable_first_tick_bubble_column");
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));

        let bubble_pos = BlockPos::new(8, 65, 8);
        let initial_position = DVec3::new(8.5, 65.0, 8.5);
        let bubble_column = vanilla_blocks::BUBBLE_COLUMN
            .default_state()
            .set_value(&BlockStateProperties::DRAG, false);

        assert!(world.set_block(bubble_pos, bubble_column, UpdateFlags::UPDATE_NONE));

        let snowball = Arc::new(SnowballEntity::new(
            &vanilla_entities::SNOWBALL,
            1,
            initial_position,
            Arc::downgrade(world),
        ));
        world
            .try_add_entity(Arc::clone(&snowball) as SharedEntity)
            .expect("snowball should attach to the loaded chunk");

        assert!(snowball.is_first_tick());

        Arc::clone(&snowball).tick();

        assert!(!snowball.is_first_tick());
        assert!(
            snowball.position().y > initial_position.y,
            "the first-tick bubble-column effect should push the projectile upward before movement"
        );

        snowball
            .try_set_position(initial_position)
            .expect("snowball should return to its initial position");
        snowball.set_velocity(DVec3::ZERO);

        Arc::clone(&snowball).tick();

        assert!(
            snowball.position().y < initial_position.y,
            "after the first tick, the bubble-column effect should occur after movement"
        );
    }

    #[test]
    fn first_tick_bubble_column_does_not_clamp_projectile_velocity() {
        init_vanilla_registry();
        init_behaviors();

        let world_fixture = fresh_test_world("throwable_first_tick_bubble_column_velocity");
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));

        let bubble_pos = BlockPos::new(8, 65, 8);
        let initial_position = DVec3::new(8.5, 65.0, 8.5);

        for (open_above, drag_down, initial_y, expected_y) in [
            (true, false, 2.0, 2.1),
            (true, true, -2.0, -2.03),
            (false, false, 2.0, 2.06),
            (false, true, -2.0, -2.03),
        ] {
            let bubble_column = vanilla_blocks::BUBBLE_COLUMN
                .default_state()
                .set_value(&BlockStateProperties::DRAG, drag_down);

            assert!(world.set_block(bubble_pos, bubble_column, UpdateFlags::UPDATE_NONE,));

            let above_state = if open_above {
                vanilla_blocks::AIR.default_state()
            } else {
                vanilla_blocks::WATER.default_state()
            };

            // The block may already have this state from the previous case.
            world.set_block(bubble_pos.above(), above_state, UpdateFlags::UPDATE_NONE);

            let snowball = SnowballEntity::new(
                &vanilla_entities::SNOWBALL,
                1,
                initial_position,
                Arc::downgrade(world),
            );
            snowball.set_velocity(DVec3::new(0.25, initial_y, -0.25));

            assert!(snowball.is_first_tick());
            snowball.handle_first_tick_bubble_column();

            let expected = DVec3::new(0.25, expected_y, -0.25);
            let actual = snowball.velocity();

            assert!(
                (actual - expected).abs().max_element() < 1.0e-12,
                "open_above={open_above}, drag_down={drag_down}: \
                 expected {expected:?}, got {actual:?}"
            );
        }
    }

// Remaining test module (no tests left after the move)
#[cfg(test)]
mod tests {
    use glam::DVec3;
    use std::sync::Arc;

    use steel_registry::blocks::{
        block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
    };
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities};
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use crate::behavior::init_behaviors;
    use crate::entity::entities::;
    use crate::entity::{Entity, SharedEntity};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    use super::ThrowableProjectile as _;


}
