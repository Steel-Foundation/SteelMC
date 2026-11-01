use std::error::Error;

use glam::DVec3;
use steel_core::entity::Entity;
use steel_registry::{
    REGISTRY,
    blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
    vanilla_dimension_types,
};
use steel_utils::{BlockPos, Direction};

use super::{fixture::Fixture, helpers};

pub(super) fn check() -> Result<(), Box<dyn Error>> {
    let mut fixture = Fixture::with_dimension(&vanilla_dimension_types::OVERWORLD, true)?;
    helpers::floor(&fixture);
    fixture.join_player()?;
    let _runtime = fixture.runtime.enter();
    let pos = BlockPos::new(8, 65, 8);
    let mut tick = 1;
    for name in ["chair", "oak_vertical_slab"] {
        for yaw in [0.0, 90.0, 180.0, 270.0] {
            fixture
                .player
                .try_set_position(DVec3::new(6.5, 65.0, 8.5))?;
            fixture.player.set_rotation((yaw, 0.0));
            helpers::place(&fixture, name, pos)?;
            let state = fixture.world.get_block_state(pos);
            assert_eq!(
                state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING),
                Some(Direction::from_yaw(yaw)),
                "{name} must follow the stair placement direction at yaw {yaw}"
            );
            let saved = helpers::saved(&fixture)?;
            assert_eq!(
                saved.chunk.get_block_state(pos),
                state,
                "facing must survive chunk storage"
            );
            if name == "chair" {
                helpers::interact(&fixture, pos);
                assert!(fixture.player.vehicle().is_some());
                let model = REGISTRY
                    .client_content
                    .block(state.get_block())
                    .ok_or("missing chair model")?;
                let cushion = model
                    .displays_for(state)
                    .first()
                    .ok_or("missing chair cushion")?;
                assert!(
                    (fixture.player.position().x
                        - f64::from(pos.x())
                        - f64::from(cushion.translation.x))
                    .abs()
                        < 1e-6
                );
                assert!(
                    (fixture.player.position().z
                        - f64::from(pos.z())
                        - f64::from(cushion.translation.z))
                    .abs()
                        < 1e-6
                );
                fixture.player.stop_riding();
            }
            assert!(fixture.world.destroy_block(pos, false));
            fixture.world.tick_game(tick, true);
            tick += 1;
        }
    }
    fixture.player.set_rotation((0.0, 0.0));
    Ok(())
}
