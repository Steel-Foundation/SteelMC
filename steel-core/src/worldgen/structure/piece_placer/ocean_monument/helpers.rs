use super::*;
use crate::entity::Entity;

pub(super) fn generate_water_box(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
) {
    let water = vanilla_blocks::WATER.default_state();
    let air = vanilla_blocks::AIR.default_state();
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                let block = placer.block_at(x, y, z);
                if fill_keeps(block) {
                    continue;
                }

                let pos = placer.world_pos(x, y, z);
                if pos.y() >= placer.sea_level() && block != water {
                    placer.place_block(air, x, y, z);
                } else {
                    placer.place_block(water, x, y, z);
                }
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "matches vanilla OceanMonumentPiece.generateBoxOnFillOnly bounds"
)]
pub(super) fn generate_box_on_fill_only(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
    state: BlockStateId,
) {
    let water = vanilla_blocks::WATER.default_state();
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                if placer.block_at(x, y, z) == water {
                    placer.place_block(state, x, y, z);
                }
            }
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "direct port of vanilla OceanMonumentPiece.generateDefaultFloor"
)]
pub(super) fn generate_default_floor(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    xoff: i32,
    zoff: i32,
    down_opening: bool,
) {
    if down_opening {
        placer.generate_box(
            xoff,
            0,
            zoff,
            xoff + 2,
            0,
            zoff + 7,
            base_gray(),
            base_gray(),
            false,
        );
        placer.generate_box(
            xoff + 5,
            0,
            zoff,
            xoff + 7,
            0,
            zoff + 7,
            base_gray(),
            base_gray(),
            false,
        );
        placer.generate_box(
            xoff + 3,
            0,
            zoff,
            xoff + 4,
            0,
            zoff + 2,
            base_gray(),
            base_gray(),
            false,
        );
        placer.generate_box(
            xoff + 3,
            0,
            zoff + 5,
            xoff + 4,
            0,
            zoff + 7,
            base_gray(),
            base_gray(),
            false,
        );
        placer.generate_box(
            xoff + 3,
            0,
            zoff + 2,
            xoff + 4,
            0,
            zoff + 2,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + 3,
            0,
            zoff + 5,
            xoff + 4,
            0,
            zoff + 5,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + 2,
            0,
            zoff + 3,
            xoff + 2,
            0,
            zoff + 4,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + 5,
            0,
            zoff + 3,
            xoff + 5,
            0,
            zoff + 4,
            base_light(),
            base_light(),
            false,
        );
    } else {
        placer.generate_box(
            xoff,
            0,
            zoff,
            xoff + 7,
            0,
            zoff + 7,
            base_gray(),
            base_gray(),
            false,
        );
    }
}

pub(super) fn spawn_elder(placer: &mut ScatteredFeaturePlacer<'_, '_>, x: i32, y: i32, z: i32) {
    let pos = placer.world_pos(x, y, z);
    if !placer.clip().contains_blockpos(pos) {
        return;
    }

    let entity = Arc::new(RawEntity::new(
        next_entity_id(),
        DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()),
            f64::from(pos.z()) + 0.5,
        ),
        placer.weak_world(),
        &vanilla_entities::ELDER_GUARDIAN,
    ));
    entity.set_persistence_required();
    entity.snap_to(
        DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()),
            f64::from(pos.z()) + 0.5,
        ),
        0.0,
        0.0,
    );
    let _ = placer.add_fresh_entity(entity);
}

fn fill_keeps(state: BlockStateId) -> bool {
    let block = state.get_block();
    block == &vanilla_blocks::ICE
        || block == &vanilla_blocks::PACKED_ICE
        || block == &vanilla_blocks::BLUE_ICE
        || block == &vanilla_blocks::WATER
}

pub(super) const fn open(room: OceanMonumentRoomData, direction: Direction) -> bool {
    room.has_opening[direction_index(direction)]
}

const fn direction_index(direction: Direction) -> usize {
    match direction {
        Direction::Down => 0,
        Direction::Up => 1,
        Direction::North => 2,
        Direction::South => 3,
        Direction::West => 4,
        Direction::East => 5,
    }
}

pub(super) fn base_gray() -> BlockStateId {
    vanilla_blocks::PRISMARINE.default_state()
}

pub(super) fn base_light() -> BlockStateId {
    vanilla_blocks::PRISMARINE_BRICKS.default_state()
}

pub(super) fn base_black() -> BlockStateId {
    vanilla_blocks::DARK_PRISMARINE.default_state()
}

pub(super) fn dot_deco() -> BlockStateId {
    base_light()
}

pub(super) fn lamp() -> BlockStateId {
    vanilla_blocks::SEA_LANTERN.default_state()
}
