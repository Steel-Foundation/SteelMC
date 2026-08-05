use super::{Direction, Rotation};
use super::{
    grid::{SimpleGrid, is_house},
    placement::PlacementData,
    template::{MansionTemplatePiece, Mirror, add_piece, compose_rotation, relative},
};

pub(super) fn place_entrance(pieces: &mut Vec<MansionTemplatePiece>, data: &mut PlacementData) {
    let pos = relative(data.position, data.rotation, Direction::West, 9);
    add_piece(pieces, "entrance", pos, data.rotation, Mirror::None);
    data.position = relative(data.position, data.rotation, Direction::South, 16);
}

pub(super) fn traverse_wall_piece(
    pieces: &mut Vec<MansionTemplatePiece>,
    data: &mut PlacementData,
) {
    let pos = relative(data.position, data.rotation, Direction::East, 7);
    add_piece(pieces, data.wall_type, pos, data.rotation, Mirror::None);
    data.position = relative(data.position, data.rotation, Direction::South, 8);
}

pub(super) fn traverse_turn(pieces: &mut Vec<MansionTemplatePiece>, data: &mut PlacementData) {
    data.position = relative(data.position, data.rotation, Direction::South, -1);
    add_piece(
        pieces,
        "wall_corner",
        data.position,
        data.rotation,
        Mirror::None,
    );
    data.position = relative(data.position, data.rotation, Direction::South, -7);
    data.position = relative(data.position, data.rotation, Direction::West, -6);
    data.rotation = compose_rotation(data.rotation, Rotation::Clockwise90);
}

pub(super) fn traverse_inner_turn(
    _pieces: &mut Vec<MansionTemplatePiece>,
    data: &mut PlacementData,
) {
    data.position = relative(data.position, data.rotation, Direction::South, 6);
    data.position = relative(data.position, data.rotation, Direction::East, 8);
    data.rotation = compose_rotation(data.rotation, Rotation::CounterClockwise90);
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors vanilla's traverseOuterWalls signature"
)]
pub(super) fn traverse_outer_walls(
    pieces: &mut Vec<MansionTemplatePiece>,
    data: &mut PlacementData,
    grid: &SimpleGrid,
    initial_dir: Direction,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
) {
    let mut grid_x = start_x;
    let mut grid_y = start_y;
    let mut dir = initial_dir;
    let start_dir = dir;

    loop {
        let (dx, dz) = dir.offset_xz();
        if !is_house(grid, grid_x + dx, grid_y + dz) {
            traverse_turn(pieces, data);
            dir = dir.rotate_y_clockwise();
            if grid_x != end_x || grid_y != end_y || start_dir != dir {
                traverse_wall_piece(pieces, data);
            }
        } else if is_house(grid, grid_x + dx, grid_y + dz)
            && is_house(
                grid,
                grid_x + dx + dir.rotate_y_counter_clockwise().offset_vec().x,
                grid_y + dz + dir.rotate_y_counter_clockwise().offset_vec().z,
            )
        {
            traverse_inner_turn(pieces, data);
            grid_x += dx;
            grid_y += dz;
            dir = dir.rotate_y_counter_clockwise();
        } else {
            grid_x += dx;
            grid_y += dz;
            if grid_x != end_x || grid_y != end_y || start_dir != dir {
                traverse_wall_piece(pieces, data);
            }
        }

        if grid_x == end_x && grid_y == end_y && start_dir == dir {
            break;
        }
    }
}
